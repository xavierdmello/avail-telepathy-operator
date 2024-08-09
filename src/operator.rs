/// Continuously keep light client updated with chain
use alloy_primitives::U256;
use anyhow::Result;
use helios::consensus::rpc::ConsensusRpc;
use helios::consensus::{rpc::nimbus_rpc::NimbusRpc, Inner};
use helios_2_script::*;
use log::{error, info};
use sp1_helios_primitives::types::ProofInputs;
use sp1_sdk::{ProverClient, SP1ProofWithPublicValues, SP1ProvingKey, SP1Stdin};
use ssz_rs::prelude::*;
use std::env;
use std::sync::Arc;
use std::time::Duration;

const ELF: &[u8] = include_bytes!("../../program/elf/riscv32im-succinct-zkvm-elf");

/// Alias the fill provider for the Ethereum network. Retrieved from the instantiation of the
/// ProviderBuilder. Recommended method for passing around a ProviderBuilder.
type EthereumFillProvider = FillProvider<
    JoinFill<
        JoinFill<JoinFill<JoinFill<Identity, GasFiller>, NonceFiller>, ChainIdFiller>,
        WalletFiller<EthereumWallet>,
    >,
    RootProvider<Http<Client>>,
    Http<Client>,
    Ethereum,
>;

struct SP1LightClientOperator {
    client: ProverClient,
    pk: SP1ProvingKey,
    wallet_filler: Arc<EthereumFillProvider>,
    contract_address: Address,
    relayer_address: Address,
    chain_id: u64,
}

impl SP1LightClientOperator {
    pub async fn new() -> Self {
        dotenv::dotenv().ok();

        let client = ProverClient::new();
        let (pk, _) = client.setup(ELF);
        let chain_id: u64 = env::var("DEST_CHAIN_ID")
            .expect("DEST_CHAIN_ID not set")
            .parse()
            .unwrap();
        let rpc_url = env::var("DEST_RPC_URL")
            .expect("DEST_RPC_URL not set")
            .parse()
            .unwrap();

        let private_key = env::var("PRIVATE_KEY").expect("PRIVATE_KEY not set");
        let contract_address: Address = env::var("CONTRACT_ADDRESS")
            .expect("CONTRACT_ADDRESS not set")
            .parse()
            .unwrap();
        let signer: PrivateKeySigner = private_key.parse().expect("Failed to parse private key");
        let relayer_address = signer.address();
        let wallet = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(wallet)
            .on_http(rpc_url);

        Self {
            client,
            pk,
            wallet_filler: Arc::new(provider),
            chain_id,
            contract_address,
            relayer_address,
        }
    }

    /// Fetch values and generate an 'update' proof for the SP1 LightClient contract.
    async fn request_update(
        &self,
        client: Inner<NimbusRpc>,
    ) -> Result<Option<SP1ProofWithPublicValues>> {
        // Fetch required values.
        let contract = SP1LightClient::new(self.contract_address, self.wallet_filler.clone());
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

        let mut stdin = SP1Stdin::new();

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
        stdin.write_slice(&encoded_proof_inputs);

        // Generate proof.
        let proof = self.client.prove(&self.pk, stdin).plonk().run().unwrap();

        info!("New head: {:?}", latest_block.as_u64());
        Ok(Some(proof))
    }

    /// Relay an update proof to the SP1 LightClient contract.
    async fn relay_update(&self, proof: SP1ProofWithPublicValues) -> Result<()> {
        let proof_as_bytes = if env::var("SP1_PROVER").unwrap().to_lowercase() == "mock" {
            vec![]
        } else {
            proof.bytes()
        };
        let public_values_bytes = proof.public_values.to_vec();

        let contract = SP1LightClient::new(self.contract_address, self.wallet_filler.clone());

        let gas_limit = relay::get_gas_limit(self.chain_id);
        let max_fee_per_gas = relay::get_fee_cap(self.chain_id, self.wallet_filler.root()).await;

        let nonce = self
            .wallet_filler
            .get_transaction_count(self.relayer_address)
            .await?;

        // Wait for 3 required confirmations with a timeout of 60 seconds.
        const NUM_CONFIRMATIONS: u64 = 3;
        const TIMEOUT_SECONDS: u64 = 60;
        let receipt = contract
            .update(proof_as_bytes.into(), public_values_bytes.into())
            .gas_price(max_fee_per_gas)
            .gas(gas_limit)
            .nonce(nonce)
            .send()
            .await?
            .with_required_confirmations(NUM_CONFIRMATIONS)
            .with_timeout(Some(Duration::from_secs(TIMEOUT_SECONDS)))
            .get_receipt()
            .await?;

        // If status is false, it reverted.
        if !receipt.status() {
            error!("Transaction reverted!");
        }

        info!("Transaction hash: {:?}", receipt.transaction_hash);

        Ok(())
    }

    /// Start the operator.
    async fn run(&mut self, loop_delay_mins: u64) -> Result<()> {
        info!("Starting SP1 Tel epathy operator");

        loop {
            let contract = SP1LightClient::new(self.contract_address, self.wallet_filler.clone());

            // TODO: Get the current slot from avail
            // let slot = contract.head().call().await?.head.try_into().unwrap();

            // Fetch the checkpoint at that slot
            let checkpoint = get_checkpoint(slot).await;

            // Get the client from the checkpoint
            let client = get_client(checkpoint.as_bytes().to_vec()).await;

            // Request an update
            match self.request_update(client).await {
                Ok(Some(proof)) => {
                    self.relay_update(proof).await?;
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
