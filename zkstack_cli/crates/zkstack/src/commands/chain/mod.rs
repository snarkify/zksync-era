use ::common::forge::ForgeScriptArgs;
use args::build_transactions::BuildTransactionsArgs;
pub(crate) use args::create::ChainCreateArgsFinal;
use clap::{command, Subcommand};
pub(crate) use create::create_chain_inner;
use xshell::Shell;

use crate::commands::chain::{
    args::create::ChainCreateArgs, genesis::GenesisCommand, init::ChainInitCommand,
};

mod accept_chain_ownership;
pub(crate) mod args;
mod build_transactions;
mod common;
mod create;
pub mod deploy_l2_contracts;
pub mod deploy_paymaster;
pub mod genesis;
pub mod init;
pub mod register_chain;
mod set_token_multiplier_setter;
mod setup_legacy_bridge;

#[derive(Subcommand, Debug)]
pub enum ChainCommands {
    /// Create a new chain, setting the necessary configurations for later initialization
    Create(ChainCreateArgs),
    /// Create unsigned transactions for chain deployment
    BuildTransactions(BuildTransactionsArgs),
    /// Initialize chain, deploying necessary contracts and performing on-chain operations
    Init(Box<ChainInitCommand>),
    /// Run server genesis
    Genesis(GenesisCommand),
    /// Register a new chain on L1 (executed by L1 governor).
    /// This command deploys and configures Governance, ChainAdmin, and DiamondProxy contracts,
    /// registers chain with BridgeHub and sets pending admin for DiamondProxy.
    /// Note: After completion, L2 governor can accept ownership by running `accept-chain-ownership`
    #[command(alias = "register")]
    RegisterChain(ForgeScriptArgs),
    /// Deploy all L2 contracts (executed by L1 governor).
    #[command(alias = "l2")]
    DeployL2Contracts(deploy_l2_contracts::Command),
    /// Accept ownership of L2 chain (executed by L2 governor).
    /// This command should be run after `register-chain` to accept ownership of newly created
    /// DiamondProxy contract.
    #[command(alias = "accept-ownership")]
    AcceptChainOwnership(ForgeScriptArgs),
    /// Initialize bridges on L2
    #[command(alias = "bridge")]
    InitializeBridges(deploy_l2_contracts::Command),
    /// Deploy L2 consensus registry
    #[command(alias = "consensus")]
    DeployConsensusRegistry(deploy_l2_contracts::Command),
    /// Deploy L2 multicall3
    #[command(alias = "multicall3")]
    DeployMulticall3(deploy_l2_contracts::Command),
    /// Deploy Default Upgrader
    #[command(alias = "upgrader")]
    DeployUpgrader(deploy_l2_contracts::Command),
    /// Deploy paymaster smart contract
    #[command(alias = "paymaster")]
    DeployPaymaster(ForgeScriptArgs),
    /// Update Token Multiplier Setter address on L1
    UpdateTokenMultiplierSetter(ForgeScriptArgs),
}

pub(crate) async fn run(shell: &Shell, args: ChainCommands) -> anyhow::Result<()> {
    match args {
        ChainCommands::Create(args) => create::run(args, shell),
        ChainCommands::Init(args) => init::run(*args, shell).await,
        ChainCommands::BuildTransactions(args) => build_transactions::run(args, shell).await,
        ChainCommands::Genesis(args) => genesis::run(args, shell).await,
        ChainCommands::RegisterChain(args) => register_chain::run(args, shell).await,
        ChainCommands::DeployL2Contracts(cmd) => {
            cmd.run(shell, deploy_l2_contracts::Contracts::all()).await
        }
        ChainCommands::AcceptChainOwnership(args) => accept_chain_ownership::run(args, shell).await,
        ChainCommands::DeployConsensusRegistry(cmd) => {
            let mut c = deploy_l2_contracts::Contracts::default();
            c.consensus_registry = true;
            cmd.run(shell, c).await
        }
        ChainCommands::DeployMulticall3(cmd) => {
            let mut c = deploy_l2_contracts::Contracts::default();
            c.multicall3 = true;
            cmd.run(shell, c).await
        }
        ChainCommands::DeployUpgrader(cmd) => {
            let mut c = deploy_l2_contracts::Contracts::default();
            c.force_deploy_upgrader = true;
            cmd.run(shell, c).await
        }
        ChainCommands::InitializeBridges(cmd) => {
            let mut c = deploy_l2_contracts::Contracts::default();
            c.shared_bridge = true;
            cmd.run(shell, c).await
        }
        ChainCommands::DeployPaymaster(args) => deploy_paymaster::run(args, shell).await,
        ChainCommands::UpdateTokenMultiplierSetter(args) => {
            set_token_multiplier_setter::run(args, shell).await
        }
    }
}