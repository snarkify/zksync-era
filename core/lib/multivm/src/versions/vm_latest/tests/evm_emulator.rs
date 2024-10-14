use std::collections::HashMap;

use ethabi::Token;
use test_casing::{test_casing, Product};
use zksync_contracts::SystemContractCode;
use zksync_system_constants::{
    CONTRACT_DEPLOYER_ADDRESS, KNOWN_CODES_STORAGE_ADDRESS, L2_BASE_TOKEN_ADDRESS,
};
use zksync_test_contracts::{TestContract, TxType};
use zksync_types::{
    get_code_key, get_known_code_key,
    utils::{key_for_eth_balance, storage_key_for_eth_balance},
    AccountTreeId, Address, Execute, StorageKey, H256, U256,
};
use zksync_utils::{
    be_words_to_bytes,
    bytecode::{hash_bytecode, hash_evm_bytecode},
    bytes_to_be_words, h256_to_u256,
};

use crate::{
    interface::{
        storage::InMemoryStorage, TxExecutionMode, VmExecutionResultAndLogs, VmInterfaceExt,
    },
    versions::testonly::default_system_env,
    vm_latest::{
        tests::tester::{VmTester, VmTesterBuilder},
        HistoryEnabled,
    },
};

fn override_system_contracts(storage: &mut InMemoryStorage) {
    let mock_deployer = TestContract::mock_deployer().bytecode.to_vec();
    let mock_deployer_hash = hash_bytecode(&mock_deployer);
    let mock_known_code_storage = TestContract::mock_known_code_storage().bytecode.to_vec();
    let mock_known_code_storage_hash = hash_bytecode(&mock_known_code_storage);

    storage.set_value(get_code_key(&CONTRACT_DEPLOYER_ADDRESS), mock_deployer_hash);
    storage.set_value(
        get_known_code_key(&mock_deployer_hash),
        H256::from_low_u64_be(1),
    );
    storage.set_value(
        get_code_key(&KNOWN_CODES_STORAGE_ADDRESS),
        mock_known_code_storage_hash,
    );
    storage.set_value(
        get_known_code_key(&mock_known_code_storage_hash),
        H256::from_low_u64_be(1),
    );
    storage.store_factory_dep(mock_deployer_hash, mock_deployer);
    storage.store_factory_dep(mock_known_code_storage_hash, mock_known_code_storage);
}

#[derive(Debug)]
struct EvmTestBuilder {
    deploy_emulator: bool,
    storage: InMemoryStorage,
    evm_contract_addresses: Vec<Address>,
}

impl EvmTestBuilder {
    fn new(deploy_emulator: bool, evm_contract_address: Address) -> Self {
        Self {
            deploy_emulator,
            storage: InMemoryStorage::with_system_contracts(hash_bytecode),
            evm_contract_addresses: vec![evm_contract_address],
        }
    }

    fn with_mock_deployer(mut self) -> Self {
        override_system_contracts(&mut self.storage);
        self
    }

    fn with_evm_address(mut self, address: Address) -> Self {
        self.evm_contract_addresses.push(address);
        self
    }

    fn build(self) -> VmTester<HistoryEnabled> {
        let mock_emulator = TestContract::mock_evm_emulator().bytecode.to_vec();
        let mut storage = self.storage;
        let mut system_env = default_system_env();
        if self.deploy_emulator {
            let evm_bytecode: Vec<_> = (0..32).collect();
            let evm_bytecode_hash = hash_evm_bytecode(&evm_bytecode);
            storage.set_value(
                get_known_code_key(&evm_bytecode_hash),
                H256::from_low_u64_be(1),
            );
            for evm_address in self.evm_contract_addresses {
                storage.set_value(get_code_key(&evm_address), evm_bytecode_hash);
            }

            system_env.base_system_smart_contracts.evm_emulator = Some(SystemContractCode {
                hash: hash_bytecode(&mock_emulator),
                code: bytes_to_be_words(mock_emulator),
            });
        } else {
            let emulator_hash = hash_bytecode(&mock_emulator);
            storage.set_value(get_known_code_key(&emulator_hash), H256::from_low_u64_be(1));
            storage.store_factory_dep(emulator_hash, mock_emulator);

            for evm_address in self.evm_contract_addresses {
                storage.set_value(get_code_key(&evm_address), emulator_hash);
                // Set `isUserSpace` in the emulator storage to `true`, so that it skips emulator-specific checks
                storage.set_value(
                    StorageKey::new(AccountTreeId::new(evm_address), H256::zero()),
                    H256::from_low_u64_be(1),
                );
            }
        }

        VmTesterBuilder::new(HistoryEnabled)
            .with_system_env(system_env)
            .with_storage(storage)
            .with_execution_mode(TxExecutionMode::VerifyExecute)
            .with_random_rich_accounts(1)
            .build()
    }
}

#[test]
fn tracing_evm_contract_deployment() {
    let mut storage = InMemoryStorage::with_system_contracts(hash_bytecode);
    override_system_contracts(&mut storage);

    let mut system_env = default_system_env();
    // The EVM emulator will not be accessed, so we set it to a dummy value.
    system_env.base_system_smart_contracts.evm_emulator =
        Some(system_env.base_system_smart_contracts.default_aa.clone());
    let mut vm = VmTesterBuilder::new(HistoryEnabled)
        .with_system_env(system_env)
        .with_storage(storage)
        .with_execution_mode(TxExecutionMode::VerifyExecute)
        .with_random_rich_accounts(1)
        .build();
    let account = &mut vm.rich_accounts[0];

    let args = [Token::Bytes((0..32).collect())];
    let evm_bytecode = ethabi::encode(&args);
    let expected_bytecode_hash = hash_evm_bytecode(&evm_bytecode);
    let execute = Execute::for_deploy(expected_bytecode_hash, vec![0; 32], &args);
    let deploy_tx = account.get_l2_tx_for_execute(execute, None);
    let (_, vm_result) = vm
        .vm
        .execute_transaction_with_bytecode_compression(deploy_tx, true);
    assert!(!vm_result.result.is_failed(), "{:?}", vm_result.result);

    // Check that the surrogate EVM bytecode was added to the decommitter.
    let known_bytecodes = vm.vm.state.decommittment_processor.known_bytecodes.inner();
    let known_evm_bytecode =
        be_words_to_bytes(&known_bytecodes[&h256_to_u256(expected_bytecode_hash)]);
    assert_eq!(known_evm_bytecode, evm_bytecode);

    let new_known_factory_deps = vm_result.new_known_factory_deps.unwrap();
    assert_eq!(new_known_factory_deps.len(), 2); // the deployed EraVM contract + EVM contract
    assert_eq!(
        new_known_factory_deps[&expected_bytecode_hash],
        evm_bytecode
    );
}

#[test]
fn mock_emulator_basics() {
    let called_address = Address::repeat_byte(0x23);
    let mut vm = EvmTestBuilder::new(true, called_address).build();
    let account = &mut vm.rich_accounts[0];
    let tx = account.get_l2_tx_for_execute(
        Execute {
            contract_address: Some(called_address),
            calldata: vec![],
            value: 0.into(),
            factory_deps: vec![],
        },
        None,
    );

    let (_, vm_result) = vm
        .vm
        .execute_transaction_with_bytecode_compression(tx, true);
    assert!(!vm_result.result.is_failed(), "{:?}", vm_result.result);
}

const RECIPIENT_ADDRESS: Address = Address::repeat_byte(0x12);

/// `deploy_emulator = false` here and below tests the mock emulator as an ordinary contract (i.e., sanity-checks its logic).
#[test_casing(2, [false, true])]
#[test]
fn mock_emulator_with_payment(deploy_emulator: bool) {
    let mut vm = EvmTestBuilder::new(deploy_emulator, RECIPIENT_ADDRESS).build();

    let mut current_balance = U256::zero();
    for i in 1_u64..=5 {
        let transferred_value = (1_000_000_000 * i).into();
        let vm_result = test_payment(
            &mut vm,
            &TestContract::mock_evm_emulator().abi,
            &mut current_balance,
            transferred_value,
        );

        let balance_storage_logs = vm_result.logs.storage_logs.iter().filter_map(|log| {
            (*log.log.key.address() == L2_BASE_TOKEN_ADDRESS)
                .then_some((*log.log.key.key(), h256_to_u256(log.log.value)))
        });
        let balances: HashMap<_, _> = balance_storage_logs.collect();
        assert_eq!(
            balances[&key_for_eth_balance(&RECIPIENT_ADDRESS)],
            current_balance
        );
    }
}

fn test_payment(
    vm: &mut VmTester<HistoryEnabled>,
    mock_emulator_abi: &ethabi::Contract,
    balance: &mut U256,
    transferred_value: U256,
) -> VmExecutionResultAndLogs {
    *balance += transferred_value;
    let test_payment_fn = mock_emulator_abi.function("testPayment").unwrap();
    let account = &mut vm.rich_accounts[0];
    let tx = account.get_l2_tx_for_execute(
        Execute {
            contract_address: Some(RECIPIENT_ADDRESS),
            calldata: test_payment_fn
                .encode_input(&[Token::Uint(transferred_value), Token::Uint(*balance)])
                .unwrap(),
            value: transferred_value,
            factory_deps: vec![],
        },
        None,
    );

    let (_, vm_result) = vm
        .vm
        .execute_transaction_with_bytecode_compression(tx, true);
    assert!(!vm_result.result.is_failed(), "{vm_result:?}");
    vm_result
}

#[test_casing(4, Product(([false, true], [false, true])))]
#[test]
fn mock_emulator_with_recursion(deploy_emulator: bool, is_external: bool) {
    let mock_emulator_abi = &TestContract::mock_evm_emulator().abi;
    let recipient_address = Address::repeat_byte(0x12);
    let mut vm = EvmTestBuilder::new(deploy_emulator, recipient_address).build();
    let account = &mut vm.rich_accounts[0];

    let test_recursion_fn = mock_emulator_abi
        .function(if is_external {
            "testExternalRecursion"
        } else {
            "testRecursion"
        })
        .unwrap();
    let mut expected_value = U256::one();
    let depth = 50_u32;
    for i in 2..=depth {
        expected_value *= i;
    }

    let factory_deps = if is_external {
        vec![TestContract::recursive_test().bytecode.to_vec()]
    } else {
        vec![]
    };
    let tx = account.get_l2_tx_for_execute(
        Execute {
            contract_address: Some(recipient_address),
            calldata: test_recursion_fn
                .encode_input(&[Token::Uint(depth.into()), Token::Uint(expected_value)])
                .unwrap(),
            value: 0.into(),
            factory_deps,
        },
        None,
    );
    let (_, vm_result) = vm
        .vm
        .execute_transaction_with_bytecode_compression(tx, true);
    assert!(!vm_result.result.is_failed(), "{vm_result:?}");
}

#[test]
fn calling_to_mock_emulator_from_native_contract() {
    let recipient_address = Address::repeat_byte(0x12);
    let mut vm = EvmTestBuilder::new(true, recipient_address).build();
    let account = &mut vm.rich_accounts[0];

    // Deploy a native contract.
    let deploy_tx = account.get_deploy_tx(
        TestContract::recursive_test().bytecode,
        Some(&[Token::Address(recipient_address)]),
        TxType::L2,
    );
    let (_, vm_result) = vm
        .vm
        .execute_transaction_with_bytecode_compression(deploy_tx.tx, true);
    assert!(!vm_result.result.is_failed(), "{:?}", vm_result.result);

    // Call from the native contract to the EVM emulator.
    let test_fn = TestContract::recursive_test()
        .abi
        .function("recurse")
        .unwrap();
    let test_tx = account.get_l2_tx_for_execute(
        Execute {
            contract_address: Some(deploy_tx.address),
            calldata: test_fn.encode_input(&[Token::Uint(50.into())]).unwrap(),
            value: Default::default(),
            factory_deps: vec![],
        },
        None,
    );
    let (_, vm_result) = vm
        .vm
        .execute_transaction_with_bytecode_compression(test_tx, true);
    assert!(!vm_result.result.is_failed(), "{:?}", vm_result.result);
}

#[test]
fn mock_emulator_with_deployment() {
    let contract_address = Address::repeat_byte(0xaa);
    let mut vm = EvmTestBuilder::new(true, contract_address)
        .with_mock_deployer()
        .build();
    let account = &mut vm.rich_accounts[0];

    let mock_emulator_abi = &TestContract::mock_evm_emulator().abi;
    let new_evm_bytecode = vec![0xfe; 96];
    let new_evm_bytecode_hash = hash_evm_bytecode(&new_evm_bytecode);

    let test_fn = mock_emulator_abi.function("testDeploymentAndCall").unwrap();
    let test_tx = account.get_l2_tx_for_execute(
        Execute {
            contract_address: Some(contract_address),
            calldata: test_fn
                .encode_input(&[
                    Token::FixedBytes(new_evm_bytecode_hash.0.into()),
                    Token::Bytes(new_evm_bytecode.clone()),
                ])
                .unwrap(),
            value: 0.into(),
            factory_deps: vec![],
        },
        None,
    );
    let (_, vm_result) = vm
        .vm
        .execute_transaction_with_bytecode_compression(test_tx, true);
    assert!(!vm_result.result.is_failed(), "{vm_result:?}");

    let factory_deps = vm_result.new_known_factory_deps.unwrap();
    assert_eq!(
        factory_deps,
        HashMap::from([(new_evm_bytecode_hash, new_evm_bytecode)])
    );
}

#[test]
fn mock_emulator_with_delegate_call() {
    let evm_contract_address = Address::repeat_byte(0xaa);
    let other_evm_contract_address = Address::repeat_byte(0xbb);
    let mut builder = EvmTestBuilder::new(true, evm_contract_address);
    builder.storage.set_value(
        storage_key_for_eth_balance(&evm_contract_address),
        H256::from_low_u64_be(1_000_000),
    );
    builder.storage.set_value(
        storage_key_for_eth_balance(&other_evm_contract_address),
        H256::from_low_u64_be(2_000_000),
    );
    let mut vm = builder.with_evm_address(other_evm_contract_address).build();
    let account = &mut vm.rich_accounts[0];

    // Deploy a native contract.
    let deploy_tx =
        account.get_deploy_tx(TestContract::increment_test().bytecode, None, TxType::L2);
    let (_, vm_result) = vm
        .vm
        .execute_transaction_with_bytecode_compression(deploy_tx.tx, true);
    assert!(!vm_result.result.is_failed(), "{:?}", vm_result.result);

    let test_fn = TestContract::increment_test()
        .abi
        .function("testDelegateCall")
        .unwrap();
    // Delegate to the native contract from EVM.
    test_delegate_call(&mut vm, test_fn, evm_contract_address, deploy_tx.address);
    // Delegate to EVM from the native contract.
    test_delegate_call(&mut vm, test_fn, deploy_tx.address, evm_contract_address);
    // Delegate to EVM from EVM.
    test_delegate_call(
        &mut vm,
        test_fn,
        evm_contract_address,
        other_evm_contract_address,
    );
}

fn test_delegate_call(
    vm: &mut VmTester<HistoryEnabled>,
    test_fn: &ethabi::Function,
    from: Address,
    to: Address,
) {
    let account = &mut vm.rich_accounts[0];
    let test_tx = account.get_l2_tx_for_execute(
        Execute {
            contract_address: Some(from),
            calldata: test_fn.encode_input(&[Token::Address(to)]).unwrap(),
            value: 0.into(),
            factory_deps: vec![],
        },
        None,
    );
    let (_, vm_result) = vm
        .vm
        .execute_transaction_with_bytecode_compression(test_tx, true);
    assert!(!vm_result.result.is_failed(), "{vm_result:?}");
}

#[test]
fn mock_emulator_with_static_call() {
    let evm_contract_address = Address::repeat_byte(0xaa);
    let other_evm_contract_address = Address::repeat_byte(0xbb);
    let mut builder = EvmTestBuilder::new(true, evm_contract_address);
    builder.storage.set_value(
        storage_key_for_eth_balance(&evm_contract_address),
        H256::from_low_u64_be(1_000_000),
    );
    builder.storage.set_value(
        storage_key_for_eth_balance(&other_evm_contract_address),
        H256::from_low_u64_be(2_000_000),
    );
    // Set differing read values for tested contracts. The slot index is defined in the contract.
    let value_slot = H256::from_low_u64_be(0x123);
    builder.storage.set_value(
        StorageKey::new(AccountTreeId::new(evm_contract_address), value_slot),
        H256::from_low_u64_be(100),
    );
    builder.storage.set_value(
        StorageKey::new(AccountTreeId::new(other_evm_contract_address), value_slot),
        H256::from_low_u64_be(200),
    );
    let mut vm = builder.with_evm_address(other_evm_contract_address).build();
    let account = &mut vm.rich_accounts[0];

    // Deploy a native contract.
    let deploy_tx =
        account.get_deploy_tx(TestContract::increment_test().bytecode, None, TxType::L2);
    let (_, vm_result) = vm
        .vm
        .execute_transaction_with_bytecode_compression(deploy_tx.tx, true);
    assert!(!vm_result.result.is_failed(), "{:?}", vm_result.result);

    let test_fn = TestContract::increment_test()
        .abi
        .function("testStaticCall")
        .unwrap();
    // Call to the native contract from EVM.
    test_static_call(&mut vm, test_fn, evm_contract_address, deploy_tx.address, 0);
    // Call to EVM from the native contract.
    test_static_call(
        &mut vm,
        test_fn,
        deploy_tx.address,
        evm_contract_address,
        100,
    );
    // Call to EVM from EVM.
    test_static_call(
        &mut vm,
        test_fn,
        evm_contract_address,
        other_evm_contract_address,
        200,
    );
}

fn test_static_call(
    vm: &mut VmTester<HistoryEnabled>,
    test_fn: &ethabi::Function,
    from: Address,
    to: Address,
    expected_value: u64,
) {
    let account = &mut vm.rich_accounts[0];
    let test_tx = account.get_l2_tx_for_execute(
        Execute {
            contract_address: Some(from),
            calldata: test_fn
                .encode_input(&[Token::Address(to), Token::Uint(expected_value.into())])
                .unwrap(),
            value: 0.into(),
            factory_deps: vec![],
        },
        None,
    );
    let (_, vm_result) = vm
        .vm
        .execute_transaction_with_bytecode_compression(test_tx, true);
    assert!(!vm_result.result.is_failed(), "{vm_result:?}");
}
