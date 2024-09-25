use std::sync::Arc;

use once_cell::sync::OnceCell;
use zksync_test_account::Account;
use zksync_types::{Address, Execute, K256PrivateKey, H256};

use crate::{
    interface::{TxExecutionMode, VmExecutionMode, VmInterface},
    tracers::CallTracer,
    utils::testonly::check_call_tracer_test_result,
    vm_latest::{
        constants::BATCH_COMPUTATIONAL_GAS_LIMIT,
        tests::{
            tester::VmTesterBuilder,
            utils::{read_max_depth_contract, read_test_contract},
        },
        HistoryEnabled, ToTracerPointer,
    },
};

// This test is ultra slow, so it's ignored by default.
#[test]
#[ignore]
fn test_max_depth() {
    let contarct = read_max_depth_contract();
    let address = Address::random();
    let mut vm = VmTesterBuilder::new(HistoryEnabled)
        .with_empty_in_memory_storage()
        .with_random_rich_accounts(1)
        .with_deployer()
        .with_bootloader_gas_limit(BATCH_COMPUTATIONAL_GAS_LIMIT)
        .with_execution_mode(TxExecutionMode::VerifyExecute)
        .with_custom_contracts(vec![(contarct, address, true)])
        .build();

    let account = &mut vm.rich_accounts[0];
    let tx = account.get_l2_tx_for_execute(
        Execute {
            contract_address: Some(address),
            calldata: vec![],
            value: Default::default(),
            factory_deps: vec![],
        },
        None,
    );

    let result = Arc::new(OnceCell::new());
    let call_tracer = CallTracer::new(result.clone()).into_tracer_pointer();
    vm.vm.push_transaction(tx);
    let res = vm
        .vm
        .inspect(&mut call_tracer.into(), VmExecutionMode::OneTx);
    assert!(result.get().is_some());
    assert!(res.result.is_failed());
}

#[test]
fn test_basic_behavior() {
    let contarct = read_test_contract();
    let address = Address::from_low_u64_le(0x1c7264e2bd8d2d84);
    let mut vm = VmTesterBuilder::new(HistoryEnabled)
        .with_empty_in_memory_storage()
        .with_rich_accounts(vec![Account::new(
            K256PrivateKey::from_bytes(H256::from([1; 32])).unwrap(),
        )])
        .with_deployer()
        .with_bootloader_gas_limit(BATCH_COMPUTATIONAL_GAS_LIMIT)
        .with_execution_mode(TxExecutionMode::VerifyExecute)
        .with_custom_contracts(vec![(contarct, address, true)])
        .build();

    let increment_by_6_calldata =
        "7cf5dab00000000000000000000000000000000000000000000000000000000000000006";

    let account = &mut vm.rich_accounts[0];
    let tx = account.get_l2_tx_for_execute(
        Execute {
            contract_address: Some(address),
            calldata: hex::decode(increment_by_6_calldata).unwrap(),
            value: Default::default(),
            factory_deps: vec![],
        },
        None,
    );

    let result = Arc::new(OnceCell::new());
    let call_tracer = CallTracer::new(result.clone()).into_tracer_pointer();
    vm.vm.push_transaction(tx);
    let res = vm
        .vm
        .inspect(&mut call_tracer.into(), VmExecutionMode::OneTx);

    let call_tracer_result = result.get().unwrap();

    check_call_tracer_test_result(call_tracer_result);
    assert!(!res.result.is_failed());
}
