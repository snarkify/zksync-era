use std::{fmt, path::PathBuf};

use anyhow::Context as _;
use tokio::fs;
use zksync_queued_job_processor::async_trait;
use zksync_types::contract_verification_api::{CompilationArtifacts, CompilerVersions};
use zksync_utils::env::Workspace;

use crate::{
    compilers::{Solc, SolcInput, ZkSolc, ZkSolcInput, ZkVyper, ZkVyperInput},
    error::ContractVerifierError,
};

/// Compiler versions supported by a [`CompilerResolver`].
#[derive(Debug)]
pub(crate) struct SupportedCompilerVersions {
    pub solc: Vec<String>,
    pub zksolc: Vec<String>,
    pub vyper: Vec<String>,
    pub zkvyper: Vec<String>,
}

impl SupportedCompilerVersions {
    pub fn lacks_any_compiler(&self) -> bool {
        self.solc.is_empty()
            || self.zksolc.is_empty()
            || self.vyper.is_empty()
            || self.zkvyper.is_empty()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CompilerPaths {
    /// Path to the base (non-zk) compiler.
    pub base: PathBuf,
    /// Path to the zk compiler.
    pub zk: PathBuf,
}

/// Encapsulates compiler paths resolution.
#[async_trait]
pub(crate) trait CompilerResolver: fmt::Debug + Send + Sync {
    /// Returns compiler versions supported by this resolver.
    ///
    /// # Errors
    ///
    /// Returned errors are assumed to be fatal.
    async fn supported_versions(&self) -> anyhow::Result<SupportedCompilerVersions>;

    /// Resolves a `solc` compiler.
    async fn resolve_solc(
        &self,
        version: &str,
    ) -> Result<Box<dyn Compiler<SolcInput>>, ContractVerifierError>;

    /// Resolves a `zksolc` compiler.
    async fn resolve_zksolc(
        &self,
        versions: &CompilerVersions,
    ) -> Result<Box<dyn Compiler<ZkSolcInput>>, ContractVerifierError>;

    /// Resolves a `zkvyper` compiler.
    async fn resolve_zkvyper(
        &self,
        versions: &CompilerVersions,
    ) -> Result<Box<dyn Compiler<ZkVyperInput>>, ContractVerifierError>;
}

/// Encapsulates a one-off compilation process.
#[async_trait]
pub(crate) trait Compiler<In>: Send + fmt::Debug {
    /// Performs compilation.
    async fn compile(
        self: Box<Self>,
        input: In,
    ) -> Result<CompilationArtifacts, ContractVerifierError>;
}

/// Default [`CompilerResolver`] using pre-downloaded compilers in the `/etc` subdirectories (relative to the workspace).
#[derive(Debug)]
pub(crate) struct EnvCompilerResolver {
    home_dir: PathBuf,
}

impl Default for EnvCompilerResolver {
    fn default() -> Self {
        Self {
            home_dir: Workspace::locate().core(),
        }
    }
}

impl EnvCompilerResolver {
    async fn read_dir(&self, dir: &str) -> anyhow::Result<Vec<String>> {
        let mut dir_entries = fs::read_dir(self.home_dir.join(dir))
            .await
            .context("failed reading dir")?;
        let mut versions = vec![];
        while let Some(entry) = dir_entries.next_entry().await? {
            let Ok(file_type) = entry.file_type().await else {
                continue;
            };
            if file_type.is_dir() {
                if let Ok(name) = entry.file_name().into_string() {
                    versions.push(name);
                }
            }
        }
        Ok(versions)
    }

    async fn resolve_solc_path(
        &self,
        solc_version: &str,
    ) -> Result<PathBuf, ContractVerifierError> {
        let solc_path = self
            .home_dir
            .join("etc")
            .join("solc-bin")
            .join(solc_version)
            .join("solc");
        if !fs::try_exists(&solc_path)
            .await
            .context("failed accessing solc")?
        {
            return Err(ContractVerifierError::UnknownCompilerVersion(
                "solc",
                solc_version.to_owned(),
            ));
        }
        Ok(solc_path)
    }
}

#[async_trait]
impl CompilerResolver for EnvCompilerResolver {
    async fn supported_versions(&self) -> anyhow::Result<SupportedCompilerVersions> {
        Ok(SupportedCompilerVersions {
            solc: self
                .read_dir("etc/solc-bin")
                .await
                .context("failed reading solc dir")?,
            zksolc: self
                .read_dir("etc/zksolc-bin")
                .await
                .context("failed reading zksolc dir")?,
            vyper: self
                .read_dir("etc/vyper-bin")
                .await
                .context("failed reading vyper dir")?,
            zkvyper: self
                .read_dir("etc/zkvyper-bin")
                .await
                .context("failed reading zkvyper dir")?,
        })
    }

    async fn resolve_solc(
        &self,
        version: &str,
    ) -> Result<Box<dyn Compiler<SolcInput>>, ContractVerifierError> {
        let solc_path = self.resolve_solc_path(version).await?;
        Ok(Box::new(Solc::new(solc_path)))
    }

    async fn resolve_zksolc(
        &self,
        versions: &CompilerVersions,
    ) -> Result<Box<dyn Compiler<ZkSolcInput>>, ContractVerifierError> {
        let zksolc_version = versions.zk_compiler_version().to_owned();
        let zksolc_path = self
            .home_dir
            .join("etc")
            .join("zksolc-bin")
            .join(&zksolc_version)
            .join("zksolc");
        if !fs::try_exists(&zksolc_path)
            .await
            .context("failed accessing zksolc")?
        {
            return Err(ContractVerifierError::UnknownCompilerVersion(
                "zksolc",
                zksolc_version.to_owned(),
            ));
        }

        let solc_path = self.resolve_solc_path(versions.compiler_version()).await?;
        let compiler_paths = CompilerPaths {
            base: solc_path,
            zk: zksolc_path,
        };
        Ok(Box::new(ZkSolc::new(compiler_paths, zksolc_version)))
    }

    async fn resolve_zkvyper(
        &self,
        versions: &CompilerVersions,
    ) -> Result<Box<dyn Compiler<ZkVyperInput>>, ContractVerifierError> {
        let zkvyper_version = versions.zk_compiler_version();
        let zkvyper_path = self
            .home_dir
            .join("etc")
            .join("zkvyper-bin")
            .join(zkvyper_version)
            .join("zkvyper");
        if !fs::try_exists(&zkvyper_path)
            .await
            .context("failed accessing zkvyper")?
        {
            return Err(ContractVerifierError::UnknownCompilerVersion(
                "zkvyper",
                zkvyper_version.to_owned(),
            ));
        }

        let vyper_version = versions.compiler_version();
        let vyper_path = self
            .home_dir
            .join("etc")
            .join("vyper-bin")
            .join(vyper_version)
            .join("vyper");
        if !fs::try_exists(&vyper_path)
            .await
            .context("failed accessing vyper")?
        {
            return Err(ContractVerifierError::UnknownCompilerVersion(
                "vyper",
                vyper_version.to_owned(),
            ));
        }

        let compiler_paths = CompilerPaths {
            base: vyper_path,
            zk: zkvyper_path,
        };
        Ok(Box::new(ZkVyper::new(compiler_paths)))
    }
}
