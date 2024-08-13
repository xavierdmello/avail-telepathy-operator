use alloy_primitives::B256;
use alloy_sol_types::sol;
use common::config::types::Forks;
use consensus_core::types::{Bytes32, FinalityUpdate, LightClientStore, Update};
use ssz_rs::prelude::*;
pub use ssz_rs::prelude::{Bitvector, Vector};

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct ProofInputs {
    pub updates: Vec<Update>,
    pub finality_update: FinalityUpdate,
    pub expected_current_slot: u64,
    pub store: LightClientStore,
    pub genesis_root: Bytes32,
    pub forks: Forks,
    pub execution_state_proof: ExecutionStateProof,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct ExecutionStateProof {
    #[serde(rename = "executionStateRoot")]
    pub execution_state_root: B256,
    #[serde(rename = "executionStateBranch")]
    pub execution_state_branch: Vec<B256>,
    pub gindex: String,
}
