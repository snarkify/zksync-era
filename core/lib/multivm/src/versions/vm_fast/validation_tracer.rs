use std::collections::HashSet;

use zk_evm_1_3_1::address_to_u256;
use zksync_types::{
    Address, ACCOUNT_CODE_STORAGE_ADDRESS, BOOTLOADER_ADDRESS, CONTRACT_DEPLOYER_ADDRESS,
    KECCAK256_PRECOMPILE_ADDRESS, L2_BASE_TOKEN_ADDRESS, MSG_VALUE_SIMULATOR_ADDRESS,
    SYSTEM_CONTEXT_ADDRESS, U256,
};
use zksync_vm2::{
    interface::{
        CallframeInterface, ExecutionStatus, GlobalStateInterface, Opcode::*, OpcodeType,
        ReturnType::*, Tracer,
    },
    ExecutionEnd,
};
use zksync_vm_interface::tracer::{ValidationParams, ViolatedValidationRule};

use super::utils::read_fat_pointer;

pub trait ValidationMode: Tracer + Default {
    const STOP_AFTER_VALIDATION: bool;
    fn account_validation_entered(&mut self);
    fn validation_exited(&mut self);
}

#[derive(Debug, Default)]
pub struct ValidationGasLimitOnly;
impl Tracer for ValidationGasLimitOnly {}
impl ValidationMode for ValidationGasLimitOnly {
    const STOP_AFTER_VALIDATION: bool = false;
    fn account_validation_entered(&mut self) {}
    fn validation_exited(&mut self) {}
}

/// Account abstraction exposes a chain to denial of service attacks because someone who fails to
/// authenticate does not pay for the failed transaction. Otherwise people could empty other's
/// wallets for free!
///
/// If some address repeatedly posts transactions that validate during preliminary checks but fail
/// to validate during the actual execution, that address is considered a spammer. However, when
/// the spam comes from multiple addresses, that doesn't work.
///
/// We want to ensure that a spammer has to pay for every account that fails validation. This is
/// achieved by limiting what the code of a custom account is allowed to do. If we allowed access
/// to things like time, a validation that fails in the sequencer could be crafted for free, so we
/// don't.
///
/// However, we want to give access to storage. A spammer has to pay for changing storage but
/// could change just one storage slot to invalidate transactions from many accounts. To prevent
/// that, we make sure that the storage slots accessed by different accounts are disjoint by only
/// allowing access to storage in the account itself and slots derived from the account's address.
///
/// Our rules are an extension of the rules are outlined in EIP-7562.
///
/// This tracer enforces the rules by checking what the code does at runtime, even though the
/// properties checked are supposed to always hold for a well-written custom account. Proving
/// that a contract adheres to the rules ahead of time would be challenging or even impossible,
/// considering that contracts that the code depends on may get upgraded.
#[derive(Debug, Default)]
pub struct ValidationTracer {
    in_validation: bool,
    add_return_value_to_allowed_slots: bool,

    slots_obtained_via_keccak: HashSet<U256>,
    trusted_addresses: HashSet<Address>,

    user_address: Address,
    trusted_storage: HashSet<(Address, U256)>,
    /// These location's values are added to [Self::trusted_addresses] to support upgradeable proxies.
    storage_containing_trusted_addresses: HashSet<(Address, U256)>,

    validation_error: Option<ViolatedValidationRule>,
}

impl ValidationMode for ValidationTracer {
    const STOP_AFTER_VALIDATION: bool = true;

    fn account_validation_entered(&mut self) {
        self.in_validation = true;
    }

    fn validation_exited(&mut self) {
        self.in_validation = false;
    }
}

impl Tracer for ValidationTracer {
    fn before_instruction<OP: OpcodeType, S: GlobalStateInterface>(&mut self, state: &mut S) {
        if !self.in_validation {
            return;
        }

        match OP::VALUE {
            // Out of gas once means out of gas for the whole validation, as the EIP forbids handling out of gas errors
            Ret(Panic) if state.current_frame().gas() == 0 => {
                self.set_error(ViolatedValidationRule::TookTooManyComputationalGas(0))
            }

            ContextMeta => self.set_error(ViolatedValidationRule::TouchedDisallowedContext),

            StorageRead => {
                let address = state.current_frame().address();
                let caller = state.current_frame().caller();

                // Can unwrap because the instruction pointer does not point to a panic instruction
                let pc = state.current_frame().program_counter().unwrap();
                let word = pc / 4;
                let part = pc % 4;
                let instruction =
                    state.current_frame().read_contract_code(word).0[3 - part as usize];
                let slot = state.read_register((instruction >> 16) as u8 & 0b1111).0;

                if self
                    .storage_containing_trusted_addresses
                    .contains(&(address, slot))
                {
                    self.trusted_addresses.insert(address);
                } else if !self.is_valid_storage_read(
                    address,
                    caller,
                    slot,
                    state.get_storage(address, slot),
                ) {
                    self.set_error(ViolatedValidationRule::TouchedDisallowedStorageSlots(
                        address, slot,
                    ))
                }
            }

            _ => {}
        }
    }

    fn after_instruction<OP: OpcodeType, S: GlobalStateInterface>(
        &mut self,
        state: &mut S,
    ) -> ExecutionStatus {
        if !self.in_validation {
            return ExecutionStatus::Running;
        }

        if self.validation_error.is_some() {
            return ExecutionStatus::Stopped(ExecutionEnd::Panicked);
        }

        match OP::VALUE {
            FarCall(_) => {
                // Intercept calls to keccak, whitelist storage slots corresponding to the hash
                let code_address = state.current_frame().code_address();
                if code_address == KECCAK256_PRECOMPILE_ADDRESS {
                    let calldata = read_fat_pointer(state, state.read_register(1).0);
                    if calldata.len() != 64 {
                        return ExecutionStatus::Running;
                    }

                    // Solidity mappings store values at the keccak256 hash of `key ++ slot_of_mapping`
                    let (key, mapping) = calldata.split_at(32);

                    let mapping_is_allowed =
                        self.slots_obtained_via_keccak.contains(&mapping.into());

                    if U256::from(key) == address_to_u256(&self.user_address) || mapping_is_allowed
                    {
                        self.add_return_value_to_allowed_slots = true;
                    }
                } else if code_address != self.user_address
                    && state
                        .get_storage(ACCOUNT_CODE_STORAGE_ADDRESS, address_to_u256(&code_address))
                        .is_zero()
                {
                    self.set_error(ViolatedValidationRule::CalledContractWithNoCode(
                        code_address,
                    ));
                    return ExecutionStatus::Stopped(ExecutionEnd::Panicked);
                }
            }
            Ret(kind) => {
                if self.add_return_value_to_allowed_slots && kind == Normal {
                    let return_value = read_fat_pointer(state, state.read_register(1).0);
                    self.slots_obtained_via_keccak
                        .insert(return_value.as_slice().into());
                }
                self.add_return_value_to_allowed_slots = false;
            }
            _ => {}
        }

        ExecutionStatus::Running
    }
}

impl ValidationTracer {
    pub fn new(params: ValidationParams) -> Self {
        let ValidationParams {
            user_address,
            trusted_slots,
            trusted_addresses,
            trusted_address_slots,
            ..
        } = params;
        Self {
            user_address,
            trusted_storage: trusted_slots,
            trusted_addresses,
            storage_containing_trusted_addresses: trusted_address_slots,

            ..Self::default()
        }
    }

    fn is_valid_storage_read(
        &self,
        address: Address,
        caller: Address,
        slot: U256,
        value: U256,
    ) -> bool {
        // allow reading own slots
        address == self.user_address
        // allow reading slot <own address>
        || slot == address_to_u256(&self.user_address)
        || self.slots_obtained_via_keccak.contains(&slot)
        // some storage locations are always allowed
        || self.trusted_addresses.contains(&address)
        || self.trusted_storage.contains(&(address, slot))
        // certain system contracts are allowed to transfer ETH
        || address == L2_BASE_TOKEN_ADDRESS
            && (caller == MSG_VALUE_SIMULATOR_ADDRESS
                || caller == CONTRACT_DEPLOYER_ADDRESS
                || caller == BOOTLOADER_ADDRESS)
        // allow getting chain_id
        || address == SYSTEM_CONTEXT_ADDRESS && slot == U256::zero()
        // allow reading code hashes of existing contracts
        || address == ACCOUNT_CODE_STORAGE_ADDRESS && !value.is_zero()
    }

    fn set_error(&mut self, error: ViolatedValidationRule) {
        if self.validation_error.is_none() {
            self.validation_error = Some(error);
        }
    }

    pub fn validation_error(&self) -> Option<ViolatedValidationRule> {
        self.validation_error.clone()
    }
}