//! End-to-end test for the multisig program with token transfers via ChainedCalls.
//!
//! Flow:
//! 1. Deploy token program + multisig program
//! 2. Create a fungible token (definition + holding for minter)
//! 3. Create a 2-of-3 multisig
//! 4. Compute vault PDA (multisig's token holding account)
//! 5. Transfer tokens from minter to vault PDA
//! 6. Create a proposal to transfer tokens from vault to recipient (ChainedCall to token program)
//! 7. Approve the proposal (reach threshold)
//! 8. Execute — multisig emits ChainedCall to token program
//! 9. Verify tokens arrived at recipient
//!
//! Prerequisites:
//! - Running sequencer at SEQUENCER_URL (default http://127.0.0.1:3040)
//! - MULTISIG_PROGRAM env var pointing to compiled multisig guest binary (default: target/riscv32im-risc0-zkvm-elf/docker/multisig.bin)
//! - TOKEN_PROGRAM env var pointing to token guest binary (default: $HOME/lssa/artifacts/program_methods/token.bin)

use std::time::Duration;

use nssa::{
    AccountId, PrivateKey, ProgramDeploymentTransaction, PublicKey, PublicTransaction,
    program::Program,
    public_transaction::{Message, WitnessSet},
};
use nssa_core::program::PdaSeed;
use multisig_core::{Instruction, MultisigState, Proposal, ProposalStatus};
use lez_multisig_ffi::{
    compute_multisig_state_pda, compute_proposal_pda, compute_vault_pda, vault_pda_seed_bytes,
};
use sequencer_service_rpc::{SequencerClient, SequencerClientBuilder, RpcClient as _};
use common::transaction::NSSATransaction;
use token_core::{Instruction as TokenInstruction, TokenHolding};

const BLOCK_WAIT_SECS: u64 = 15;

fn account_id_from_key(key: &PrivateKey) -> AccountId {
    let pk = PublicKey::new_from_private_key(key);
    AccountId::from(&pk)
}

fn sequencer_client() -> SequencerClient {
    let url = std::env::var("SEQUENCER_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:3040".to_string());
    SequencerClientBuilder::default().build(&url).expect("Failed to create sequencer client")
}

async fn submit_tx(client: &SequencerClient, tx: PublicTransaction) {
    let response = client.send_transaction(NSSATransaction::Public(tx)).await.expect("Failed to submit tx");
    let tx_hash = response;
    println!("  tx_hash: {}", hex::encode(tx_hash.0));

    // Wait for inclusion: poll for up to 2 block periods
    let max_wait = Duration::from_secs(BLOCK_WAIT_SECS * 3);
    let poll_interval = Duration::from_secs(3);
    let start = std::time::Instant::now();

    loop {
        tokio::time::sleep(poll_interval).await;

        match client.get_transaction(tx_hash.clone()).await {
            Ok(resp) if resp.is_some() => {
                println!("  ✅ tx included in block");
                return;
            }
            _ => {
                if start.elapsed() > max_wait {
                    panic!(
                        "❌ Transaction {} was NOT included after {:?}. Check sequencer logs for rejection reason.",
                        tx_hash, max_wait
                    );
                }
            }
        }
    }
}

async fn get_nonce(client: &SequencerClient, account_id: AccountId) -> u128 {
    client.get_account(account_id).await
        .map(|r| r.nonce)
        .unwrap_or(0)
}

async fn get_balance(client: &SequencerClient, account_id: AccountId) -> Option<u128> {
    let resp = client.get_account(account_id).await.ok()?;
    let data: Vec<u8> = resp.data.into();
    let holding: TokenHolding = borsh::from_slice(&data).ok()?;
    match holding {
        TokenHolding::Fungible { balance, .. } => Some(balance),
        _ => None,
    }
}

async fn get_multisig_state(client: &SequencerClient, state_id: AccountId) -> MultisigState {
    let account = client.get_account(state_id).await.expect("Failed to get multisig state");
    let data: Vec<u8> = account.data.into();
    borsh::from_slice(&data).expect("Failed to deserialize multisig state")
}

async fn get_proposal(client: &SequencerClient, proposal_id: AccountId) -> Proposal {
    let account = client.get_account(proposal_id).await.expect("Failed to get proposal");
    println!("  [DEBUG] Proposal account program_owner: {:?}", account.account.program_owner);
    println!("  [DEBUG] Proposal account balance: {}", account.account.balance);
    println!("  [DEBUG] Proposal account nonce: {}", account.nonce);
    let data: Vec<u8> = account.data.into();
    println!("  [DEBUG] Proposal raw data length: {} bytes", data.len());
    if data.len() >= 128 {
        println!("  [DEBUG] Proposal raw data (first 128 bytes): {:02x?}", &data[..128]);
    } else {
        println!("  [DEBUG] Proposal raw data (all {} bytes): {:02x?}", data.len(), &data);
    }
    // Also try to manually read the index field (first u64)
    if data.len() >= 8 {
        let index = u64::from_le_bytes(data[0..8].try_into().unwrap());
        println!("  [DEBUG] Manual index read: {}", index);
    }
    match borsh::from_slice::<Proposal>(&data) {
        Ok(p) => {
            println!("  [DEBUG] Proposal deserialized OK! index={}, status={:?}, approved={}", p.index, p.status, p.approved.len());
            p
        }
        Err(e) => {
            // Try to deserialize a MultisigState instead to see if wrong account
            if let Ok(ms) = borsh::from_slice::<MultisigState>(&data) {
                panic!("Account contains MultisigState (not Proposal)! members={}, threshold={}", ms.members.len(), ms.threshold);
            }
            panic!("Failed to deserialize proposal ({} bytes): {}", data.len(), e);
        }
    }
}

fn deploy_program(bytecode: Vec<u8>) -> (ProgramDeploymentTransaction, nssa::ProgramId) {
    let program = Program::new(bytecode.clone()).expect("Invalid program");
    let program_id = program.id();
    let msg = nssa::program_deployment_transaction::Message::new(bytecode);
    (ProgramDeploymentTransaction::new(msg), program_id)
}

#[tokio::test]
async fn test_multisig_token_transfer() {
    let client = sequencer_client();

    // ── Deploy programs ─────────────────────────────────────────────────
    println!("📦 Deploying programs...");

    let token_path = std::env::var("TOKEN_PROGRAM")
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").expect("HOME env var not set");
            format!("{}/lssa/artifacts/program_methods/token.bin", home)
        });
    let token_bytecode = std::fs::read(&token_path)
        .unwrap_or_else(|_| panic!("Cannot read token binary at '{}'", token_path));
    let (token_deploy_tx, token_program_id) = deploy_program(token_bytecode);

    let multisig_path = std::env::var("MULTISIG_PROGRAM")
        .unwrap_or_else(|_| {
            let manifest_dir = env!("CARGO_MANIFEST_DIR");
            format!("{}/../target/riscv32im-risc0-zkvm-elf/docker/multisig.bin", manifest_dir)
        });
    let multisig_bytecode = std::fs::read(&multisig_path)
        .unwrap_or_else(|_| panic!("Cannot read multisig binary at '{}'", multisig_path));
    let (multisig_deploy_tx, multisig_program_id) = deploy_program(multisig_bytecode);

    // Deploy both (skip if already deployed)
    for (name, tx) in [("token", token_deploy_tx), ("multisig", multisig_deploy_tx)] {
        match client.send_tx_program(tx).await {
            Ok(r) => {
                println!("  {} deployed: {}", name, hex::encode(r.0));
                tokio::time::sleep(Duration::from_secs(BLOCK_WAIT_SECS)).await;
            }
            Err(e) => println!("  {} deploy skipped: {}", name, e),
        }
    }

    // ── Create token ────────────────────────────────────────────────────
    println!("\n═══ STEP 1: Create fungible token ═══");
    let minter_key = PrivateKey::new_os_random();
    let minter_id = account_id_from_key(&minter_key);
    // Token definition and holding are random AccountIds
    let def_key = PrivateKey::new_os_random();
    let def_id = account_id_from_key(&def_key);
    let minter_holding_key = PrivateKey::new_os_random();
    let minter_holding_id = account_id_from_key(&minter_holding_key);

    let token_instruction = TokenInstruction::NewFungibleDefinition {
        name: "TestToken".to_string(),
        total_supply: 1_000_000,
    };
    let msg = Message::try_new(
        token_program_id,
        vec![def_id, minter_holding_id],
        vec![],
        token_instruction,
    ).unwrap();
    let ws = WitnessSet::for_message(&msg, &[] as &[&PrivateKey]);
    submit_tx(&client, PublicTransaction::new(msg, ws)).await;

    let balance = get_balance(&client, minter_holding_id).await;
    println!("  Minter balance: {:?}", balance);
    assert_eq!(balance, Some(1_000_000), "Minter should have full supply");
    println!("  ✅ Token created, minter has 1,000,000 tokens");

    // ── Create multisig ─────────────────────────────────────────────────
    println!("\n═══ STEP 2: Create 2-of-3 multisig ═══");
    let key1 = PrivateKey::new_os_random();
    let key2 = PrivateKey::new_os_random();
    let key3 = PrivateKey::new_os_random();
    let m1 = account_id_from_key(&key1);
    let m2 = account_id_from_key(&key2);
    let m3 = account_id_from_key(&key3);

    let create_key: [u8; 32] = *AccountId::from(
        &PublicKey::new_from_private_key(&PrivateKey::new_os_random())
    ).value();

    let multisig_state_id = compute_multisig_state_pda(&multisig_program_id, &create_key);
    let vault_id = compute_vault_pda(&multisig_program_id, &create_key);

    println!("  Multisig state PDA: {}", multisig_state_id);
    println!("  Vault PDA: {}", vault_id);

    let instruction = Instruction::CreateMultisig {
        create_key,
        threshold: 2,
        members: vec![*m1.value(), *m2.value(), *m3.value()],
    };
    let msg = Message::try_new(
        multisig_program_id,
        vec![multisig_state_id, m1, m2, m3],
        vec![],
        instruction,
    ).unwrap();
    let ws = WitnessSet::for_message(&msg, &[] as &[&PrivateKey]);
    submit_tx(&client, PublicTransaction::new(msg, ws)).await;

    let state = get_multisig_state(&client, multisig_state_id).await;
    assert_eq!(state.threshold, 2);
    assert_eq!(state.members.len(), 3);
    println!("  ✅ 2-of-3 multisig created!");

    // ── Fund the vault ──────────────────────────────────────────────────
    println!("\n═══ STEP 3: Transfer tokens to multisig vault ═══");
    let nonce = get_nonce(&client, minter_holding_id).await;
    let transfer_to_vault = TokenInstruction::Transfer {
        amount_to_transfer: 500,
    };
    let msg = Message::try_new(
        token_program_id,
        vec![minter_holding_id, vault_id],
        vec![nonce],
        transfer_to_vault,
    ).unwrap();
    // Sign with minter_holding_key (the key that derives to the sender account)
    let ws = WitnessSet::for_message(&msg, &[&minter_holding_key]);
    submit_tx(&client, PublicTransaction::new(msg, ws)).await;

    let vault_balance = get_balance(&client, vault_id).await;
    println!("  Vault balance: {:?}", vault_balance);
    assert_eq!(vault_balance, Some(500), "Vault should have 500 tokens");
    println!("  ✅ Vault funded with 500 tokens!");

    // ── Propose token transfer from vault ───────────────────────────────
    println!("\n═══ STEP 4: Propose transfer 200 tokens from vault ═══");
    let recipient_key = PrivateKey::new_os_random();
    let recipient_id = account_id_from_key(&recipient_key);

    // Build the token transfer instruction that the ChainedCall will execute
    let token_transfer_instruction = TokenInstruction::Transfer {
        amount_to_transfer: 200,
    };
    let target_instruction_data = risc0_zkvm::serde::to_vec(&token_transfer_instruction).unwrap();

    let vault_seed = vault_pda_seed_bytes(&create_key);

    // Compute proposal PDA
    let proposal_id = compute_proposal_pda(&multisig_program_id, &create_key, 1);
    println!("  Proposal PDA: {}", proposal_id);

    let nonce_state = get_nonce(&client, multisig_state_id).await;
    let nonce_m1 = get_nonce(&client, m1).await;
    let propose_instruction = Instruction::Propose {
        target_program_id: token_program_id,
        target_instruction_data: target_instruction_data.clone(),
        target_account_count: 2,  // vault_holding + recipient_holding
        pda_seeds: vec![vault_seed],
        authorized_indices: vec![0], // vault (index 0) gets is_authorized=true
        create_key,
        proposal_index: 1,
    };
    let msg = Message::try_new(
        multisig_program_id,
        vec![multisig_state_id, m1, proposal_id], // Propose expects 3 accounts now
        vec![nonce_m1], // Only signer nonces
        propose_instruction,
    ).unwrap();
    let ws = WitnessSet::for_message(&msg, &[&key1]);
    submit_tx(&client, PublicTransaction::new(msg, ws)).await;

    // Verify proposal was created
    let proposal = get_proposal(&client, proposal_id).await;
    assert_eq!(proposal.approved.len(), 1);
    
    let state = get_multisig_state(&client, multisig_state_id).await;
    assert_eq!(state.transaction_index, 1, "transaction_index should be incremented");
    println!("  ✅ Proposal #1 created (1/2 approvals)");

    // ── Approve ─────────────────────────────────────────────────────────
    println!("\n═══ STEP 5: Member 2 approves ═══");
    let nonce_state = get_nonce(&client, multisig_state_id).await;
    let nonce_m2 = get_nonce(&client, m2).await;
    let nonce_proposal = get_nonce(&client, proposal_id).await;
    
    let msg = Message::try_new(
        multisig_program_id,
        vec![multisig_state_id, m2, proposal_id], // Approve expects 3 accounts now
        vec![nonce_m2], // Only signer nonces
        Instruction::Approve { create_key, proposal_index: 1 },
    ).unwrap();
    let ws = WitnessSet::for_message(&msg, &[&key2]);
    submit_tx(&client, PublicTransaction::new(msg, ws)).await;

    let proposal = get_proposal(&client, proposal_id).await;
    assert_eq!(proposal.approved.len(), 2, "Should have 2 approvals");
    println!("  ✅ 2/2 approvals — ready to execute!");

    // ── Execute (ChainedCall to token program) ──────────────────────────
    println!("\n═══ STEP 6: Execute — transfer tokens via ChainedCall ═══");
    let nonce_state = get_nonce(&client, multisig_state_id).await;
    let nonce_m1 = get_nonce(&client, m1).await;
    let nonce_proposal = get_nonce(&client, proposal_id).await;
    let nonce_vault = get_nonce(&client, vault_id).await;
    let nonce_recipient = get_nonce(&client, recipient_id).await;

    // Execute tx includes: [multisig_state, executor, proposal_pda, vault_holding, recipient_holding]
    let msg = Message::try_new(
        multisig_program_id,
        vec![multisig_state_id, m1, proposal_id, vault_id, recipient_id],
        vec![nonce_m1], // Only signer nonces
        Instruction::Execute { create_key, proposal_index: 1 },
    ).unwrap();
    let ws = WitnessSet::for_message(&msg, &[&key1]);
    submit_tx(&client, PublicTransaction::new(msg, ws)).await;

    // ── Verify final state ──────────────────────────────────────────────
    println!("\n═══ STEP 7: Verify results ═══");
    
    let proposal = get_proposal(&client, proposal_id).await;
    assert_eq!(proposal.status, ProposalStatus::Executed, "Proposal should be executed");
    println!("  ✅ Proposal marked as executed");

    let state = get_multisig_state(&client, multisig_state_id).await;
    assert_eq!(state.transaction_index, 1, "transaction_index should remain 1");

    let vault_balance = get_balance(&client, vault_id).await;
    println!("  Vault balance: {:?}", vault_balance);
    assert_eq!(vault_balance, Some(300), "Vault should have 300 tokens (500 - 200)");

    let recipient_balance = get_balance(&client, recipient_id).await;
    println!("  Recipient balance: {:?}", recipient_balance);
    assert_eq!(recipient_balance, Some(200), "Recipient should have 200 tokens");

    println!("\n🎉 Full multisig + token transfer e2e test PASSED!");
    println!("   - Deploy programs ✅");
    println!("   - Create token ✅");
    println!("   - Create multisig ✅");
    println!("   - Fund vault PDA ✅");
    println!("   - Propose transfer via ChainedCall ✅");
    println!("   - Approve + Execute ✅");
    println!("   - Token balances verified ✅");
}