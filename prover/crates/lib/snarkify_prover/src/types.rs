use std::sync::Arc;

use chrono::{DateTime, NaiveDateTime, Utc};
use circuit_definitions::{
    boojum::{cs::implementations::{proof::Proof, witness::WitnessVec}, field::goldilocks::GoldilocksField},
    circuit_definitions::recursion_layer::{
        ZkSyncRecursionLayerVerificationKey, ZkSyncRecursionProof,
    },
};
use serde::{Deserialize, Deserializer, Serialize};
use zksync_prover_fri_types::CircuitWrapper;
use zksync_prover_keystore::GoldilocksGpuProverSetupData;

#[derive(Deserialize, Debug, PartialEq)]
#[serde(rename_all = "UPPERCASE")]
pub enum TaskState {
    Pending,
    Success,
    Failure,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "UPPERCASE")]
pub enum ProofType {
    Chunk,
    Batch,
    Bundle,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateTaskRequest<Input: Serialize> {
    pub service_id: String,
    pub input: Input,
    pub proof_type: ProofType,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CompressionInput {
    pub proof: ZkSyncRecursionProof,
    pub scheduler_vk: ZkSyncRecursionLayerVerificationKey,
}

#[derive(Serialize, Deserialize)]
pub struct ProveInput {
    pub circuit: CircuitWrapper,
    pub witness_vector: WitnessVec<GoldilocksField>,
    pub setup_data: Arc<GoldilocksGpuProverSetupData>,
}

/// Response for Get/Create tasks requests
#[derive(Deserialize, Debug)]
pub struct TaskResponse {
    /// Task ID in Snarkify platform. It can be UUID or an empty string.
    pub task_id: String,
    #[serde(deserialize_with = "deserialize_datetime")]
    pub created: Option<DateTime<Utc>>,
    #[serde(deserialize_with = "deserialize_datetime")]
    pub started: Option<DateTime<Utc>>,
    #[serde(deserialize_with = "deserialize_datetime")]
    pub finished: Option<DateTime<Utc>>,
    pub state: TaskState,
    /// Task input data necessary for the proof generation.
    pub input: String,
    /// Serialized JSON string including the base64 encoded proof and its metadata.
    pub proof: Option<String>,
    pub error: Option<String>,
    pub proof_type: Option<ProofType>,
}

pub fn deserialize_datetime<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
where
    D: Deserializer<'de>,
{
    // The datetimes from the Snarkify API does not provide timezone information,
    // so we assume it is UTC.
    Option::<String>::deserialize(deserializer)?
        .map(|s| {
            NaiveDateTime::parse_from_str(&s, "%Y-%m-%dT%H:%M:%S")
                .map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc))
                .map_err(serde::de::Error::custom)
        })
        .transpose()
}
