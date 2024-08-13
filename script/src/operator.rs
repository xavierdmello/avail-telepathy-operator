/// Continuously keep light client updated with chain
use alloy_primitives::U256;
use anyhow::Result;
use helios::consensus::rpc::ConsensusRpc;
use helios::consensus::{rpc::nimbus_rpc::NimbusRpc, Inner};
use log::{error, info};
use ssz_rs::prelude::*;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use avail_telepathy_operator::*;

struct SP1LightClientOperator {
    // TODO - AVAIL: Add fields for configuring avail chain (e.g. rpc url, chain id, etc.)
}

impl SP1LightClientOperator {
    pub async fn new() -> Self {
        dotenv::dotenv().ok();

        /// TODO - AVAIL: Read avail chain config from env, add more fields
        let private_key = env::var("PRIVATE_KEY").expect("PRIVATE_KEY not set");
        // let chain_id = env::var("CHAIN_ID").expect("CHAIN_ID not set");


        Self {
            // Avail fields
        }
    }

    /// Fetch values and genera
    async fn request_update(
        &self,
        client: Inner<NimbusRpc>,
    ) -> Result<Option<Vec<u8>>> {
        // TODO - AVAIL: Fetch values from avail storage instead of contract on ethereum
        let head: u64 = contract
            .head()
            .call()
            .await
            .unwrap()
            .head
            .try_into()
            .unwrap();
        let period: u64 = contract
            .getSyncCommitteePeriod(U256::from(head))
            .call()
            .await
            .unwrap()
            ._0
            .try_into()
            .unwrap();
        let contract_current_sync_committee = contract
            .syncCommittees(U256::from(period))
            .call()
            .await
            .unwrap()
            ._0;
        let contract_next_sync_committee = contract
            .syncCommittees(U256::from(period + 1))
            .call()
            .await
            .unwrap()
            ._0;

        // Setup client.
        let updates = get_updates(&client).await;
        let (client, updates) = sync_client(
            client,
            updates,
            head,
            contract_current_sync_committee,
            contract_next_sync_committee,
        )
        .await;

        let finality_update = client.rpc.get_finality_update().await.unwrap();
        let latest_block = finality_update.finalized_header.slot;

        if latest_block.as_u64() <= head {
            info!("Contract is up to date. Nothing to update.");
            return Ok(None);
        }

        let execution_state_proof = get_execution_state_root_proof(latest_block.into())
            .await
            .unwrap();

        let expected_current_slot = client.expected_current_slot();
        let inputs = ProofInputs {
            updates,
            finality_update,
            expected_current_slot,
            store: client.store,
            genesis_root: client.config.chain.genesis_root.clone().try_into().unwrap(),
            forks: client.config.forks.clone(),
            execution_state_proof,
        };

        let encoded_proof_inputs = serde_cbor::to_vec(&inputs)?;
        
        info!("New head: {:?}", latest_block.as_u64());
        Ok(Some(encoded_proof_inputs))
    }

    /// Relay an update proof to the SP1 LightClient contract.
    async fn relay_update(&self, encoded_proof_inputs: Vec<u8>) -> Result<()> {
        // TODO - AVAIL: Call fulfill_call on Avail with encoded_proof_inputs as input

        Ok(())
    }

    /// Start the operator.
    async fn run(&mut self, loop_delay_mins: u64) -> Result<()> {
        info!("Starting SP1 Telepathy operator");

        loop {
            // TODO - AVAIL: Get the stored current slot from avail
            let slot = contract.head().call().await?.head.try_into().unwrap();

            // Fetch the checkpoint at that slot
            let checkpoint = get_checkpoint(slot).await;

            // Get the client from the checkpoint
            let client = get_client(checkpoint.as_bytes().to_vec()).await;

            // Request an update
            match self.request_update(client).await {
                Ok(Some(encoded_proof_inputs)) => {
                    self.relay_update(encoded_proof_inputs).await?;
                }
                Ok(None) => {
                    // Contract is up to date. Nothing to update.
                }
                Err(e) => {
                    error!("Header range request failed: {}", e);
                }
            };

            info!("Sleeping for {:?} minutes", loop_delay_mins);
            tokio::time::sleep(tokio::time::Duration::from_secs(60 * loop_delay_mins)).await;
        }
    }
}

#[tokio::main]
async fn main() {
    env::set_var("RUST_LOG", "info");
    dotenv::dotenv().ok();
    env_logger::init();

    let loop_delay_mins = match env::var("LOOP_DELAY_MINS") {
        Ok(value) if value.is_empty() => 5, // Use default if empty
        Ok(value) => value.parse().expect("Invalid LOOP_DELAY_MINS"),
        Err(_) => 5, // Use default if not set
    };

    let mut operator = SP1LightClientOperator::new().await;
    loop {
        if let Err(e) = operator.run(loop_delay_mins).await {
            error!("Error running operator: {}", e);
        }
    }
}
