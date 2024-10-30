use zksync_config::{configs::EcosystemContracts, ContractsConfig};

use crate::{envy_load, FromEnv, FromEnvVariant};

impl FromEnv for EcosystemContracts {
    fn from_env() -> anyhow::Result<Self> {
        Self::from_env_variant("".to_string())
    }
}
impl FromEnvVariant for EcosystemContracts {
    fn from_env_variant(variant: String) -> anyhow::Result<Self> {
        Ok(Self {
            bridgehub_proxy_addr: std::env::var(format!(
                "{variant}CONTRACTS_BRIDGEHUB_PROXY_ADDR"
            ))?
            .parse()?,
            state_transition_proxy_addr: std::env::var(format!(
                "{variant}CONTRACTS_STATE_TRANSITION_PROXY_ADDR"
            ))?
            .parse()?,
            transparent_proxy_admin_addr: std::env::var(format!(
                "{variant}CONTRACTS_TRANSPARENT_PROXY_ADMIN_ADDR"
            ))?
            .parse()?,
            l1_bytecodes_supplier_addr: Some(
                std::env::var(format!("{variant}CONTRACTS_L1_BYTECODE_SUPPLIER_ADDR"))?.parse()?,
            ),
        })
    }
}

impl FromEnv for ContractsConfig {
    fn from_env() -> anyhow::Result<Self> {
        Self::from_env_variant("".to_string())
    }
}
impl FromEnvVariant for ContractsConfig {
    fn from_env_variant(variant: String) -> anyhow::Result<Self> {
        let mut contracts: ContractsConfig =
            envy_load("contracts", &format!("{variant}CONTRACTS_"))?;
        // Note: we are renaming the bridge, the address remains the same
        // These two config variables should always have the same value.
        // TODO(EVM-578): double check and potentially forbid both of them being `None`.
        contracts.l2_erc20_bridge_addr = contracts
            .l2_erc20_bridge_addr
            .or(contracts.l2_shared_bridge_addr);
        contracts.l2_shared_bridge_addr = contracts
            .l2_shared_bridge_addr
            .or(contracts.l2_erc20_bridge_addr);

        if let (Some(legacy_addr), Some(shared_addr)) = (
            contracts.l2_erc20_bridge_addr,
            contracts.l2_shared_bridge_addr,
        ) {
            if legacy_addr != shared_addr {
                panic!("L2 erc20 bridge address and L2 shared bridge address are different.");
            }
        }
        contracts.ecosystem_contracts = EcosystemContracts::from_env_variant(variant).ok();
        Ok(contracts)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use zksync_basic_types::H256;
    use zksync_config::configs::EcosystemContracts;
    use zksync_system_constants::SHARED_BRIDGE_ETHER_TOKEN_ADDRESS;

    use super::*;
    use crate::test_utils::{addr, EnvMutex};

    static MUTEX: EnvMutex = EnvMutex::new();

    fn expected_config() -> ContractsConfig {
        ContractsConfig {
            governance_addr: addr("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045"),
            verifier_addr: addr("34782eE00206EAB6478F2692caa800e4A581687b"),
            default_upgrade_addr: addr("0x5e6d086f5ec079adff4fb3774cdf3e8d6a34f7e9"),
            diamond_proxy_addr: addr("F00B988a98Ca742e7958DeF9F7823b5908715f4a"),
            validator_timelock_addr: addr("F00B988a98Ca742e7958DeF9F7823b5908715f4a"),
            l1_erc20_bridge_proxy_addr: Some(addr("8656770FA78c830456B00B4fFCeE6b1De0e1b888")),
            l2_erc20_bridge_addr: Some(addr("8656770FA78c830456B00B4fFCeE6b1De0e1b888")),
            l1_weth_bridge_proxy_addr: Some(addr("8656770FA78c830456B00B4fFCeE6b1De0e1b888")),
            l2_weth_bridge_addr: Some(addr("8656770FA78c830456B00B4fFCeE6b1De0e1b888")),
            l1_shared_bridge_proxy_addr: Some(addr("8656770FA78c830456B00B4fFCeE6b1De0e1b888")),
            l2_shared_bridge_addr: Some(addr("8656770FA78c830456B00B4fFCeE6b1De0e1b888")),
            l2_legacy_shared_bridge_addr: Some(addr("8656770FA78c830456B00B4fFCeE6b1De0e1b888")),
            l2_testnet_paymaster_addr: Some(addr("FC073319977e314F251EAE6ae6bE76B0B3BAeeCF")),
            l1_multicall3_addr: addr("0xcA11bde05977b3631167028862bE2a173976CA11"),
            ecosystem_contracts: Some(EcosystemContracts {
                bridgehub_proxy_addr: addr("0x35ea7f92f4c5f433efe15284e99c040110cf6297"),
                state_transition_proxy_addr: addr("0xd90f1c081c6117241624e97cb6147257c3cb2097"),
                transparent_proxy_admin_addr: addr("0xdd6fa5c14e7550b4caf2aa2818d24c69cbc347e5"),
                l1_bytecodes_supplier_addr: Some(addr(
                    "0x36ea7f92f4c5f433efe15284e99c040110cf6297",
                )),
            }),
            base_token_addr: Some(SHARED_BRIDGE_ETHER_TOKEN_ADDRESS),
            base_token_asset_id: Some(
                H256::from_str(
                    "0x0000000000000000000000000000000000000001000000000000000000000000",
                )
                .unwrap(),
            ),
            user_facing_bridgehub_proxy_addr: Some(addr(
                "0x35ea7f92f4c5f433efe15284e99c040110cf6297",
            )),
            user_facing_diamond_proxy_addr: Some(addr(
                "0xF00B988a98Ca742e7958DeF9F7823b5908715f4a",
            )),
            chain_admin_addr: Some(addr("0xdd6fa5c14e7550b4caf2aa2818d24c69cbc347ff")),
            l2_da_validator_addr: Some(addr("0xed6fa5c14e7550b4caf2aa2818d24c69cbc347ff")),
            settlement_layer: Some(0),
        }
    }

    #[test]
    fn from_env() {
        let mut lock = MUTEX.lock();
        let config = r#"
CONTRACTS_GOVERNANCE_ADDR="0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045"
CONTRACTS_VERIFIER_ADDR="0x34782eE00206EAB6478F2692caa800e4A581687b"
CONTRACTS_DEFAULT_UPGRADE_ADDR="0x5e6d086f5ec079adff4fb3774cdf3e8d6a34f7e9"
CONTRACTS_DIAMOND_PROXY_ADDR="0xF00B988a98Ca742e7958DeF9F7823b5908715f4a"
CONTRACTS_VALIDATOR_TIMELOCK_ADDR="0xF00B988a98Ca742e7958DeF9F7823b5908715f4a"
CONTRACTS_L1_ERC20_BRIDGE_PROXY_ADDR="0x8656770FA78c830456B00B4fFCeE6b1De0e1b888"
CONTRACTS_L2_ERC20_BRIDGE_ADDR="0x8656770FA78c830456B00B4fFCeE6b1De0e1b888"
CONTRACTS_L1_WETH_BRIDGE_PROXY_ADDR="0x8656770FA78c830456B00B4fFCeE6b1De0e1b888"
CONTRACTS_L2_WETH_BRIDGE_ADDR="0x8656770FA78c830456B00B4fFCeE6b1De0e1b888"
CONTRACTS_L2_TESTNET_PAYMASTER_ADDR="FC073319977e314F251EAE6ae6bE76B0B3BAeeCF"
CONTRACTS_L2_CONSENSUS_REGISTRY_ADDR="D64e136566a9E04eb05B30184fF577F52682D182"
CONTRACTS_L1_MULTICALL3_ADDR="0xcA11bde05977b3631167028862bE2a173976CA11"
CONTRACTS_L1_SHARED_BRIDGE_PROXY_ADDR="0x8656770FA78c830456B00B4fFCeE6b1De0e1b888"
CONTRACTS_L2_SHARED_BRIDGE_ADDR="0x8656770FA78c830456B00B4fFCeE6b1De0e1b888"
CONTRACTS_L1_BYTECODE_SUPPLIER_ADDR="0x36ea7f92f4c5f433efe15284e99c040110cf6297"
CONTRACTS_L2_LEGACY_SHARED_BRIDGE_ADDR="0x8656770FA78c830456B00B4fFCeE6b1De0e1b888"
CONTRACTS_BRIDGEHUB_PROXY_ADDR="0x35ea7f92f4c5f433efe15284e99c040110cf6297"
CONTRACTS_STATE_TRANSITION_PROXY_ADDR="0xd90f1c081c6117241624e97cb6147257c3cb2097"
CONTRACTS_TRANSPARENT_PROXY_ADMIN_ADDR="0xdd6fa5c14e7550b4caf2aa2818d24c69cbc347e5"
CONTRACTS_BASE_TOKEN_ADDR="0x0000000000000000000000000000000000000001"
CONTRACTS_BASE_TOKEN_ADDR="0x0000000000000000000000000000000000000001000000000000000000000000"
CONTRACTS_USER_FACING_BRIDGEHUB_PROXY_ADDR="0x35ea7f92f4c5f433efe15284e99c040110cf6297"
CONTRACTS_USER_FACING_DIAMOND_PROXY_ADDR="0xF00B988a98Ca742e7958DeF9F7823b5908715f4a
CONTRACTS_L2_NATIVE_TOKEN_VAULT_PROXY_ADDR="0xfc073319977e314f251eae6ae6be76b0b3baeecf"
CONTRACTS_CHAIN_ADMIN_ADDR="0xdd6fa5c14e7550b4caf2aa2818d24c69cbc347ff"
CONTRACTS_SETTLEMENT_LAYER="0"
CONTRACTS_L2_DA_VALIDATOR_ADDR="0xed6fa5c14e7550b4caf2aa2818d24c69cbc347ff"
        "#;
        lock.set_env(config);

        let actual = ContractsConfig::from_env().unwrap();
        assert_eq!(actual, expected_config());
    }
}
