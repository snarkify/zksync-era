use std::sync::Arc;

use zksync_config::{ContractsConfig, GenesisConfig};
use zksync_node_storage_init::{
    main_node::{L1Recovery, MainNodeGenesis},
    InitializeStorage, NodeInitializationStrategy,
};

use super::NodeInitializationStrategyResource;
use crate::{
    implementations::resources::{
        eth_interface::EthInterfaceResource,
        pools::{MasterPool, PoolResource},
    },
    wiring_layer::{WiringError, WiringLayer},
    FromContext, IntoContext,
};

/// Wiring layer for main node initialization strategy.
#[derive(Debug)]
pub struct MainNodeInitStrategyLayer {
    pub genesis: GenesisConfig,
    pub l1_recovery_enabled: bool,
    pub contracts: ContractsConfig,
}

#[derive(Debug, FromContext)]
#[context(crate = crate)]
pub struct Input {
    pub master_pool: PoolResource<MasterPool>,
    pub eth_interface: EthInterfaceResource,
}

#[derive(Debug, IntoContext)]
#[context(crate = crate)]
pub struct Output {
    pub strategy: NodeInitializationStrategyResource,
}

#[async_trait::async_trait]
impl WiringLayer for MainNodeInitStrategyLayer {
    type Input = Input;
    type Output = Output;

    fn layer_name(&self) -> &'static str {
        "main_node_role_layer"
    }

    async fn wire(self, input: Self::Input) -> Result<Self::Output, WiringError> {
        let pool = input.master_pool.get().await?;
        let EthInterfaceResource(l1_client) = input.eth_interface;
        let genesis = Arc::new(MainNodeGenesis {
            contracts: self.contracts,
            genesis: self.genesis,
            l1_client: l1_client.clone(),
            pool: pool.clone(),
        });
        let l1_recovery: Option<Arc<dyn InitializeStorage>> = if self.l1_recovery_enabled {
            Some(Arc::new(L1Recovery { l1_client, pool }))
        } else {
            None
        };
        let strategy = NodeInitializationStrategy {
            genesis,
            snapshot_recovery: None,
            block_reverter: None,
            l1_recovery,
        };

        Ok(Output {
            strategy: strategy.into(),
        })
    }
}
