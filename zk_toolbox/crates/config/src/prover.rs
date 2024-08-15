use std::{
    cell::OnceCell,
    path::{Path, PathBuf},
};

use common::logger;
use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;
use xshell::Shell;
use zksync_config::{
    configs::{
        fri_prover_group::FriProverGroupConfig, FriProofCompressorConfig, FriProverConfig,
        FriProverGatewayConfig, FriWitnessGeneratorConfig, FriWitnessVectorGeneratorConfig,
        GeneralConfig, Secrets,
    },
    PostgresConfig,
};
use zksync_protobuf_config::{decode_yaml_repr, encode_yaml_repr};

use crate::{
    consts::PROVER_CONFIG_NAME,
    traits::{FileConfigWithDefaultName, ReadConfig, SaveConfig, ZkToolboxConfig},
    utils::find_file,
    PROVER_FILE, SECRETS_FILE,
};

#[derive(Debug, Clone)]
pub struct GeneralProverConfig {
    pub name: String,
    pub link_to_code: PathBuf,
    pub bellman_cuda_dir: Option<PathBuf>,
    pub config: PathBuf,
    pub shell: OnceCell<Shell>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralProverConfigInternal {
    pub name: String,
    pub link_to_code: PathBuf,
    pub bellman_cuda_dir: Option<PathBuf>,
    pub config: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProverConfig {
    pub postgres_config: PostgresConfig,
    pub fri_prover_config: FriProverConfig,
    pub fri_witness_generator_config: FriWitnessGeneratorConfig,
    pub fri_witness_vector_generator_config: FriWitnessVectorGeneratorConfig,
    pub fri_prover_gateway_config: FriProverGatewayConfig,
    pub fri_proof_compressor_config: FriProofCompressorConfig,
    pub fri_prover_group_config: FriProverGroupConfig,
}

impl Serialize for GeneralProverConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.get_internal().serialize(serializer)
    }
}

impl ReadConfig for GeneralProverConfig {
    fn read(shell: &Shell, path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let config: GeneralProverConfigInternal = GeneralProverConfigInternal::read(shell, path)?;

        let bellman_cuda_dir = config
            .bellman_cuda_dir
            .map(|dir| shell.current_dir().join(dir));

        Ok(GeneralProverConfig {
            name: config.name.clone(),
            link_to_code: shell.current_dir().join(config.link_to_code),
            bellman_cuda_dir,
            config: config.config.clone(),
            shell: Default::default(),
        })
    }
}

impl ReadConfig for ProverConfig {
    fn read(shell: &Shell, path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = shell.current_dir().join(path);
        let config = decode_yaml_repr::<zksync_protobuf_config::proto::general::GeneralConfig>(
            &path, false,
        )?;
        Ok(config.into())
    }
}

impl SaveConfig for ProverConfig {
    fn save(&self, shell: &Shell, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let general_config = GeneralConfig {
            postgres_config: Some(self.postgres_config.clone()),
            api_config: None,
            contract_verifier: None,
            circuit_breaker_config: None,
            mempool_config: None,
            operations_manager_config: None,
            state_keeper_config: None,
            prover_config: Some(self.fri_prover_config.clone()),
            witness_generator: Some(self.fri_witness_generator_config.clone()),
            prometheus_config: None,
            proof_data_handler_config: None,
            db_config: None,
            eth: None,
            snapshot_creator: None,
            observability: None,
            da_dispatcher_config: None,
            protective_reads_writer_config: None,
            basic_witness_input_producer_config: None,
            commitment_generator: None,
            snapshot_recovery: None,
            pruning: None,
            core_object_store: None,
            base_token_adjuster: None,
            external_price_api_client_config: None,
            consensus_config: None,
            external_proof_integration_api_config: None,
            witness_vector_generator: Some(self.fri_witness_vector_generator_config.clone()),
            prover_gateway: Some(self.fri_prover_gateway_config.clone()),
            proof_compressor_config: Some(self.fri_proof_compressor_config.clone()),
            prover_group_config: Some(self.fri_prover_group_config.clone()),
            house_keeper_config: None,
            experimental_vm_config: None,
        };

        let bytes = encode_yaml_repr::<zksync_protobuf_config::proto::general::GeneralConfig>(
            &general_config,
        )?;
        Ok(shell.write_file(path, bytes)?)
    }
}

impl FileConfigWithDefaultName for ProverConfig {
    const FILE_NAME: &'static str = PROVER_FILE;
}

impl FileConfigWithDefaultName for GeneralProverConfig {
    const FILE_NAME: &'static str = PROVER_CONFIG_NAME;
}

impl ZkToolboxConfig for GeneralProverConfigInternal {}
impl ZkToolboxConfig for GeneralProverConfig {}

/// Result of checking if the ecosystem exists.
#[derive(Error, Debug)]
pub enum GeneralProverConfigFromFileError {
    #[error("General prover configuration not found (Could not find 'ProverSubsystem.toml' in {path:?}: Make sure you have created a subsystem & are in the new folder `cd path/to/prover/subsystem/name`)"
    )]
    NotExists { path: PathBuf },
    #[error("Invalid subsystem configuration")]
    InvalidConfig { source: anyhow::Error },
}

impl GeneralProverConfig {
    fn get_shell(&self) -> &Shell {
        self.shell.get().expect("Must be initialized")
    }

    pub fn from_file(shell: &Shell) -> Result<Self, GeneralProverConfigFromFileError> {
        let Ok(path) = find_file(shell, shell.current_dir(), PROVER_CONFIG_NAME) else {
            return Err(GeneralProverConfigFromFileError::NotExists {
                path: shell.current_dir(),
            });
        };

        shell.change_dir(&path);

        let prover = match GeneralProverConfig::read(shell, PROVER_CONFIG_NAME) {
            Ok(mut config) => {
                config.shell = shell.clone().into();
                config
            }
            Err(_) => {
                let current_dir = shell.current_dir();
                let Some(parent) = current_dir.parent() else {
                    return Err(GeneralProverConfigFromFileError::NotExists { path });
                };
                // Try to find subsystem somewhere in parent directories
                shell.change_dir(parent);
                GeneralProverConfig::from_file(shell)?
            }
        };
        Ok(prover)
    }

    pub fn path_to_prover_config(&self) -> PathBuf {
        self.config.join(PROVER_FILE)
    }

    pub fn path_to_secrets(&self) -> PathBuf {
        self.config.join(SECRETS_FILE)
    }

    pub fn load_prover_config(&self) -> anyhow::Result<ProverConfig> {
        ProverConfig::read(self.get_shell(), &self.config.join(PROVER_FILE))
    }

    pub fn load_secrets_config(&self) -> anyhow::Result<Secrets> {
        Secrets::read(self.get_shell(), &self.config.join(SECRETS_FILE))
    }

    pub fn get_internal(&self) -> GeneralProverConfigInternal {
        let bellman_cuda_dir = self
            .bellman_cuda_dir
            .clone()
            .map(|dir| self.get_shell().current_dir().join(dir));

        GeneralProverConfigInternal {
            name: self.name.clone(),
            link_to_code: self.link_to_code.clone(),
            bellman_cuda_dir,
            config: self.config.clone(),
        }
    }
}

impl From<GeneralConfig> for ProverConfig {
    fn from(config: GeneralConfig) -> Self {
        Self {
            postgres_config: config.postgres_config.expect("Postgres config not found"),
            fri_prover_config: config.prover_config.expect("FRI prover config not found"),
            fri_witness_generator_config: config
                .witness_generator
                .expect("FRI witness generator config not found"),
            fri_witness_vector_generator_config: config
                .witness_vector_generator
                .expect("FRI witness vector generator config not found"),
            fri_prover_gateway_config: config
                .prover_gateway
                .expect("FRI prover gateway config not found"),
            fri_proof_compressor_config: config
                .proof_compressor_config
                .expect("FRI proof compressor config not found"),
            fri_prover_group_config: config
                .prover_group_config
                .expect("FRI prover group config not found"),
        }
    }
}