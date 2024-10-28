use ethers::types::{Address, H256};
use serde::{Deserialize, Serialize};

use crate::{
    consts::CONTRACTS_FILE,
    forge_interface::{
        deploy_ecosystem::output::DeployL1Output, deploy_l2_contracts,
        register_chain::output::RegisterChainOutput,
    },
    traits::{FileConfigWithDefaultName, ZkStackConfig},
};

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ContractsConfig {
    pub create2_factory_addr: Address,
    pub create2_factory_salt: H256,
    pub ecosystem_contracts: EcosystemContracts,
    pub bridges: BridgesContracts,
    pub l1: L1Contracts,
    pub l2: L2Contracts,
    #[serde(flatten)]
    pub other: serde_json::Value,
}

impl ContractsConfig {
    pub fn update_from_l1_output(&mut self, deploy_l1_output: &DeployL1Output) {
        self.create2_factory_addr = deploy_l1_output.create2_factory_addr;
        self.create2_factory_salt = deploy_l1_output.create2_factory_salt;
        self.bridges.erc20.l1_address = deploy_l1_output
            .deployed_addresses
            .bridges
            .erc20_bridge_proxy_addr;
        self.bridges.shared.l1_address = deploy_l1_output
            .deployed_addresses
            .bridges
            .shared_bridge_proxy_addr;
        self.ecosystem_contracts.bridgehub_proxy_addr = deploy_l1_output
            .deployed_addresses
            .bridgehub
            .bridgehub_proxy_addr;
        self.ecosystem_contracts.state_transition_proxy_addr = deploy_l1_output
            .deployed_addresses
            .state_transition
            .state_transition_proxy_addr;
        self.ecosystem_contracts.transparent_proxy_admin_addr = deploy_l1_output
            .deployed_addresses
            .transparent_proxy_admin_addr;
        self.l1.default_upgrade_addr = deploy_l1_output
            .deployed_addresses
            .state_transition
            .default_upgrade_addr;
        self.l1.diamond_proxy_addr = deploy_l1_output
            .deployed_addresses
            .state_transition
            .diamond_proxy_addr;
        self.l1.governance_addr = deploy_l1_output.deployed_addresses.governance_addr;
        self.l1.multicall3_addr = deploy_l1_output.multicall3_addr;
        self.ecosystem_contracts.validator_timelock_addr =
            deploy_l1_output.deployed_addresses.validator_timelock_addr;
        self.l1.verifier_addr = deploy_l1_output
            .deployed_addresses
            .state_transition
            .verifier_addr;
        self.l1.validator_timelock_addr =
            deploy_l1_output.deployed_addresses.validator_timelock_addr;
        self.ecosystem_contracts
            .diamond_cut_data
            .clone_from(&deploy_l1_output.contracts_config.diamond_cut_data);
        self.l1.chain_admin_addr = deploy_l1_output.deployed_addresses.chain_admin;
    }

    pub fn set_chain_contracts(&mut self, register_chain_output: &RegisterChainOutput) {
        self.l1.diamond_proxy_addr = register_chain_output.diamond_proxy_addr;
        self.l1.governance_addr = register_chain_output.governance_addr;
        self.l1.chain_admin_addr = register_chain_output.chain_admin_addr;
    }

    pub fn set_l2_contracts(&mut self, output: &deploy_l2_contracts::output::Output) {
        if let Some(spec) = &output.l2_shared_bridge_proxy {
            self.bridges.shared.l2_address = Some(spec.address);
            self.bridges.erc20.l2_address = Some(spec.address);
            self.l2.legacy_shared_bridge_addr = Some(spec.address);
        }
        if let Some(spec) = &output.l2_consensus_registry_proxy {
            self.l2.consensus_registry = Some(spec.address);
        }
        if let Some(spec) = &output.l2_force_deploy_upgrader {
            self.l2.default_l2_upgrader = spec.address;
        }
        if let Some(spec) = &output.l2_multicall3 {
            self.l2.multicall3 = Some(spec.address);
        }
    }
}

impl FileConfigWithDefaultName for ContractsConfig {
    const FILE_NAME: &'static str = CONTRACTS_FILE;
}

impl ZkStackConfig for ContractsConfig {}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct EcosystemContracts {
    pub bridgehub_proxy_addr: Address,
    pub state_transition_proxy_addr: Address,
    pub transparent_proxy_admin_addr: Address,
    pub validator_timelock_addr: Address,
    pub diamond_cut_data: String,
}

impl ZkStackConfig for EcosystemContracts {}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct BridgesContracts {
    pub erc20: BridgeContractsDefinition,
    pub shared: BridgeContractsDefinition,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct BridgeContractsDefinition {
    pub l1_address: Address,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub l2_address: Option<Address>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct L1Contracts {
    pub default_upgrade_addr: Address,
    pub diamond_proxy_addr: Address,
    pub governance_addr: Address,
    #[serde(default)]
    pub chain_admin_addr: Address,
    pub multicall3_addr: Address,
    pub verifier_addr: Address,
    pub validator_timelock_addr: Address,
    pub base_token_addr: Address,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct L2Contracts {
    pub testnet_paymaster_addr: Address,
    pub default_l2_upgrader: Address,
    pub consensus_registry: Option<Address>,
    pub multicall3: Option<Address>,
    pub legacy_shared_bridge_addr: Option<Address>,
}
