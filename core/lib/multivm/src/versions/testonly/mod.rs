//! Reusable tests and tooling for low-level VM testing.
//!
//! # How it works
//!
//! - [`TestedVm`] defines test-specific VM extensions. It's currently implemented for the latest legacy VM
//!   (`vm_latest`) and the fast VM (`vm_fast`).
//! - Submodules of this module define test functions generic by `TestedVm`. Specific VM versions implement `TestedVm`
//!   and can create tests based on these test functions with minimum amount of boilerplate code.
//! - Tests use [`VmTester`] built using [`VmTesterBuilder`] to create a VM instance. This allows to set up storage for the VM,
//!   custom [`SystemEnv`] / [`L1BatchEnv`], deployed contracts, pre-funded accounts etc.

use once_cell::sync::Lazy;
use zksync_contracts::{
    read_bootloader_code, read_zbin_bytecode, BaseSystemContracts, SystemContractCode,
};
use zksync_types::{
    block::L2BlockHasher, fee_model::BatchFeeInput, get_code_key, get_is_account_key,
    utils::storage_key_for_eth_balance, Address, L1BatchNumber, L2BlockNumber, L2ChainId,
    ProtocolVersionId, U256,
};
use zksync_utils::{bytecode::hash_bytecode, bytes_to_be_words, u256_to_h256};
use zksync_vm_interface::{L1BatchEnv, L2BlockEnv, SystemEnv, TxExecutionMode};

pub(super) use self::tester::{TestedVm, VmTester, VmTesterBuilder};
use crate::{
    interface::storage::InMemoryStorage, vm_latest::constants::BATCH_COMPUTATIONAL_GAS_LIMIT,
};

pub(super) mod block_tip;
pub(super) mod bootloader;
pub(super) mod bytecode_publishing;
pub(super) mod circuits;
pub(super) mod code_oracle;
pub(super) mod default_aa;
pub(super) mod gas_limit;
pub(super) mod get_used_contracts;
pub(super) mod is_write_initial;
pub(super) mod l1_tx_execution;
pub(super) mod l2_blocks;
pub(super) mod nonce_holder;
pub(super) mod precompiles;
pub(super) mod refunds;
pub(super) mod require_eip712;
pub(super) mod rollbacks;
pub(super) mod secp256r1;
mod shadow;
pub(super) mod simple_execution;
pub(super) mod storage;
mod tester;
pub(super) mod tracing_execution_error;
pub(super) mod transfer;
pub(super) mod upgrade;

static BASE_SYSTEM_CONTRACTS: Lazy<BaseSystemContracts> =
    Lazy::new(BaseSystemContracts::load_from_disk);

fn get_empty_storage() -> InMemoryStorage {
    InMemoryStorage::with_system_contracts(hash_bytecode)
}

pub(crate) fn read_max_depth_contract() -> Vec<u8> {
    read_zbin_bytecode(
        "core/tests/ts-integration/contracts/zkasm/artifacts/deep_stak.zkasm/deep_stak.zkasm.zbin",
    )
}

pub(crate) fn get_bootloader(test: &str) -> SystemContractCode {
    let bootloader_code = read_bootloader_code(test);
    let bootloader_hash = hash_bytecode(&bootloader_code);
    SystemContractCode {
        code: bytes_to_be_words(bootloader_code),
        hash: bootloader_hash,
    }
}

pub(super) fn default_system_env() -> SystemEnv {
    SystemEnv {
        zk_porter_available: false,
        version: ProtocolVersionId::latest(),
        base_system_smart_contracts: BaseSystemContracts::playground(),
        bootloader_gas_limit: BATCH_COMPUTATIONAL_GAS_LIMIT,
        execution_mode: TxExecutionMode::VerifyExecute,
        default_validation_computational_gas_limit: BATCH_COMPUTATIONAL_GAS_LIMIT,
        chain_id: L2ChainId::from(270),
    }
}

pub(super) fn default_l1_batch(number: L1BatchNumber) -> L1BatchEnv {
    // Add a bias to the timestamp to make it more realistic / "random".
    let timestamp = 1_700_000_000 + u64::from(number.0);
    L1BatchEnv {
        previous_batch_hash: None,
        number,
        timestamp,
        fee_input: BatchFeeInput::l1_pegged(
            50_000_000_000, // 50 gwei
            250_000_000,    // 0.25 gwei
        ),
        fee_account: Address::repeat_byte(1),
        enforced_base_fee: None,
        first_l2_block: L2BlockEnv {
            number: 1,
            timestamp,
            prev_block_hash: L2BlockHasher::legacy_hash(L2BlockNumber(0)),
            max_virtual_blocks_to_create: 100,
        },
    }
}

pub(super) fn make_address_rich(storage: &mut InMemoryStorage, address: Address) {
    let key = storage_key_for_eth_balance(&address);
    storage.set_value(key, u256_to_h256(U256::from(10_u64.pow(19))));
}

#[derive(Debug, Clone)]
pub(super) struct ContractToDeploy {
    bytecode: Vec<u8>,
    address: Address,
    is_account: bool,
    is_funded: bool,
}

impl ContractToDeploy {
    pub fn new(bytecode: Vec<u8>, address: Address) -> Self {
        Self {
            bytecode,
            address,
            is_account: false,
            is_funded: false,
        }
    }

    pub fn account(bytecode: Vec<u8>, address: Address) -> Self {
        Self {
            bytecode,
            address,
            is_account: true,
            is_funded: false,
        }
    }

    #[must_use]
    pub fn funded(mut self) -> Self {
        self.is_funded = true;
        self
    }

    pub fn insert(&self, storage: &mut InMemoryStorage) {
        let deployer_code_key = get_code_key(&self.address);
        storage.set_value(deployer_code_key, hash_bytecode(&self.bytecode));
        if self.is_account {
            let is_account_key = get_is_account_key(&self.address);
            storage.set_value(is_account_key, u256_to_h256(1_u32.into()));
        }
        storage.store_factory_dep(hash_bytecode(&self.bytecode), self.bytecode.clone());

        if self.is_funded {
            make_address_rich(storage, self.address);
        }
    }

    /// Inserts the contracts into the test environment, bypassing the deployer system contract.
    pub fn insert_all(contracts: &[Self], storage: &mut InMemoryStorage) {
        for contract in contracts {
            contract.insert(storage);
        }
    }
}
