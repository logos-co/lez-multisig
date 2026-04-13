//! End-to-end test for the multisig program with token transfers via ChainedCalls.
//!
//! Flow:
//! 1.  Deploy token program + multisig program
//! 2.  Create a fungible token (definition + holding for minter)
//! 3.  Create a 2-of-3 multisig
//! 4.  Compute vault PDA (multisig's token holding account)
//! 5.  Initialize vault token holding via multisig ChainedCall to token.InitializeAccount
//!     (the vault PDA can only be authorized as a Claim::Authorized account when multisig
//!     is the ChainedCall caller — the vault_id IS the PDA of the multisig program)
//! 6.  Transfer tokens from minter to vault (now works: vault owned by token program,
//!     no claim needed for an already-initialized account)
//! 7.  Initialize recipient token holding directly (recipient signs)
//! 8.  Create a proposal (index 2) to transfer tokens from vault to recipient
//! 9.  Approve the proposal (reach threshold)
//! 10. Execute — multisig emits ChainedCall to token.Transfer(vault → recipient)
//!     (vault is authorized via PDA mechanism, recipient already exists — no new claim)
//! 11. Verify tokens arrived at recipient
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
        .map(|r| r.nonce.0)
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
    println!("  [DEBUG] Proposal account program_owner: {:?}", account.program_owner);
    println!("  [DEBUG] Proposal account balance: {}", account.balance);
    println!("  [DEBUG] Proposal account nonce: {}", account.nonce.0);
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
        match client.send_transaction(NSSATransaction::ProgramDeployment(tx)).await {
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
    // Both def and holding accounts must sign to authorize Claim::Authorized in the token program.
    // Fresh accounts have nonce=0.
    let msg = Message::try_new(
        token_program_id,
        vec![def_id, minter_holding_id],
        vec![nssa_core::account::Nonce(0), nssa_core::account::Nonce(0)],
        token_instruction,
    ).unwrap();
    let ws = WitnessSet::for_message(&msg, &[&def_key, &minter_holding_key]);
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
    let vault_seed = vault_pda_seed_bytes(&create_key);

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

    // ── Initialize vault token holding via multisig ChainedCall ─────────
    // The vault is a PDA of the multisig program. The token program requires
    // Claim::Authorized for fresh accounts. A PDA can only be authorized via the
    // ChainedCall mechanism when the PDA's owner program is the caller.
    // So we initialize the vault by having the multisig execute a proposal that
    // calls token.InitializeAccount(def_id, vault_id) — vault_id is an authorized
    // PDA of multisig in the ChainedCall, so Claim::Authorized succeeds.
    // After this, vault_id.program_owner = token_program_id.
    println!("\n═══ STEP 3: Initialize vault holding via multisig ChainedCall ═══");
    let vault_init_proposal_id = compute_proposal_pda(&multisig_program_id, &create_key, 1);
    println!("  Vault-init proposal PDA: {}", vault_init_proposal_id);

    let vault_init_instruction_data = risc0_zkvm::serde::to_vec(
        &TokenInstruction::InitializeAccount
    ).unwrap();

    // Propose #1: initialize vault via token.InitializeAccount
    // target accounts: [def_id (index 0), vault_id (index 1)]
    // vault at index 1 is the authorized PDA (authorized_indices = [1])
    let nonce_m1 = get_nonce(&client, m1).await;
    let msg = Message::try_new(
        multisig_program_id,
        vec![multisig_state_id, m1, vault_init_proposal_id],
        vec![nssa_core::account::Nonce(nonce_m1)],
        Instruction::Propose {
            target_program_id: token_program_id,
            target_instruction_data: vault_init_instruction_data,
            target_account_count: 2,   // def_id + vault_id
            pda_seeds: vec![vault_seed],
            authorized_indices: vec![1], // vault at index 1
            create_key,
            proposal_index: 1,
        },
    ).unwrap();
    let ws = WitnessSet::for_message(&msg, &[&key1]);
    submit_tx(&client, PublicTransaction::new(msg, ws)).await;

    let vault_init_proposal = get_proposal(&client, vault_init_proposal_id).await;
    assert_eq!(vault_init_proposal.approved.len(), 1);
    println!("  ✅ Vault-init proposal #1 created");

    // Approve vault-init proposal (m2)
    let nonce_m2 = get_nonce(&client, m2).await;
    let msg = Message::try_new(
        multisig_program_id,
        vec![multisig_state_id, m2, vault_init_proposal_id],
        vec![nssa_core::account::Nonce(nonce_m2)],
        Instruction::Approve { create_key, proposal_index: 1 },
    ).unwrap();
    let ws = WitnessSet::for_message(&msg, &[&key2]);
    submit_tx(&client, PublicTransaction::new(msg, ws)).await;

    // Execute vault-init: ChainedCall → token.InitializeAccount(def_id, vault_id)
    // In the ChainedCall, vault_id is in authorized_pdas = {vault_id} because
    // compute_authorized_pdas(multisig_program_id, [vault_seed]) yields vault_id.
    // So Claim::Authorized for vault_id is validated → vault.program_owner = token_program_id.
    let nonce_m1 = get_nonce(&client, m1).await;
    let msg = Message::try_new(
        multisig_program_id,
        vec![multisig_state_id, m1, vault_init_proposal_id, def_id, vault_id],
        vec![nssa_core::account::Nonce(nonce_m1)],
        Instruction::Execute { create_key, proposal_index: 1 },
    ).unwrap();
    let ws = WitnessSet::for_message(&msg, &[&key1]);
    submit_tx(&client, PublicTransaction::new(msg, ws)).await;

    let vault_init_proposal = get_proposal(&client, vault_init_proposal_id).await;
    assert_eq!(vault_init_proposal.status, ProposalStatus::Executed, "Vault-init proposal should be executed");
    println!("  ✅ Vault initialized — vault.program_owner = token_program");

    // ── Fund the vault (direct transfer) ────────────────────────────────
    // Now that vault is owned by the token program, token.Transfer can write to it
    // without claiming it (new_claimed_if_default returns no claim when
    // account.program_owner != DEFAULT_PROGRAM_ID). No vault signing needed.
    println!("\n═══ STEP 4: Transfer 500 tokens from minter to vault ═══");
    let nonce = get_nonce(&client, minter_holding_id).await;
    let msg = Message::try_new(
        token_program_id,
        vec![minter_holding_id, vault_id],
        vec![nssa_core::account::Nonce(nonce)],
        TokenInstruction::Transfer { amount_to_transfer: 500 },
    ).unwrap();
    let ws = WitnessSet::for_message(&msg, &[&minter_holding_key]);
    submit_tx(&client, PublicTransaction::new(msg, ws)).await;

    let vault_balance = get_balance(&client, vault_id).await;
    println!("  Vault balance: {:?}", vault_balance);
    assert_eq!(vault_balance, Some(500), "Vault should have 500 tokens");
    println!("  ✅ Vault funded with 500 tokens!");

    // ── Initialize recipient token holding directly ──────────────────────
    // The recipient must pre-initialize their holding before the multisig executes
    // the transfer. Once recipient.program_owner = token_program_id, the ChainedCall
    // token.Transfer will not need Claim::Authorized for the recipient (no claim when
    // account already has a non-default program_owner).
    println!("\n═══ STEP 5: Initialize recipient holding ═══");
    let recipient_key = PrivateKey::new_os_random();
    let recipient_id = account_id_from_key(&recipient_key);
    let msg = Message::try_new(
        token_program_id,
        vec![def_id, recipient_id],
        vec![nssa_core::account::Nonce(0)],  // recipient is fresh
        TokenInstruction::InitializeAccount,
    ).unwrap();
    let ws = WitnessSet::for_message(&msg, &[&recipient_key]);
    submit_tx(&client, PublicTransaction::new(msg, ws)).await;
    println!("  ✅ Recipient holding initialized, recipient.program_owner = token_program");

    // ── Propose token transfer from vault to recipient ───────────────────
    println!("\n═══ STEP 6: Propose transfer 200 tokens from vault ═══");
    let token_transfer_instruction = TokenInstruction::Transfer { amount_to_transfer: 200 };
    let target_instruction_data = risc0_zkvm::serde::to_vec(&token_transfer_instruction).unwrap();

    // This is proposal #2 (vault-init was #1)
    let proposal_id = compute_proposal_pda(&multisig_program_id, &create_key, 2);
    println!("  Transfer proposal PDA: {}", proposal_id);

    let nonce_m1 = get_nonce(&client, m1).await;
    let msg = Message::try_new(
        multisig_program_id,
        vec![multisig_state_id, m1, proposal_id],
        vec![nssa_core::account::Nonce(nonce_m1)],
        Instruction::Propose {
            target_program_id: token_program_id,
            target_instruction_data: target_instruction_data.clone(),
            target_account_count: 2,     // vault (index 0) + recipient (index 1)
            pda_seeds: vec![vault_seed], // vault is PDA of multisig
            authorized_indices: vec![0], // vault (sender) at index 0 must be authorized
            create_key,
            proposal_index: 2,
        },
    ).unwrap();
    let ws = WitnessSet::for_message(&msg, &[&key1]);
    submit_tx(&client, PublicTransaction::new(msg, ws)).await;

    let proposal = get_proposal(&client, proposal_id).await;
    assert_eq!(proposal.approved.len(), 1);
    let state = get_multisig_state(&client, multisig_state_id).await;
    assert_eq!(state.transaction_index, 2, "transaction_index should be 2");
    println!("  ✅ Proposal #2 created (1/2 approvals)");

    // ── Approve ─────────────────────────────────────────────────────────
    println!("\n═══ STEP 7: Member 2 approves ═══");
    let nonce_m2 = get_nonce(&client, m2).await;
    let msg = Message::try_new(
        multisig_program_id,
        vec![multisig_state_id, m2, proposal_id],
        vec![nssa_core::account::Nonce(nonce_m2)],
        Instruction::Approve { create_key, proposal_index: 2 },
    ).unwrap();
    let ws = WitnessSet::for_message(&msg, &[&key2]);
    submit_tx(&client, PublicTransaction::new(msg, ws)).await;

    let proposal = get_proposal(&client, proposal_id).await;
    assert_eq!(proposal.approved.len(), 2, "Should have 2 approvals");
    println!("  ✅ 2/2 approvals — ready to execute!");

    // ── Execute (ChainedCall to token program) ──────────────────────────
    // In the ChainedCall: vault is authorized_pdas (vault_id = PDA of multisig).
    // recipient is already owned by token → new_claimed_if_default returns no claim.
    // Only the executor (m1) needs to sign this TX.
    println!("\n═══ STEP 8: Execute — transfer tokens via ChainedCall ═══");
    let nonce_m1 = get_nonce(&client, m1).await;
    let msg = Message::try_new(
        multisig_program_id,
        vec![multisig_state_id, m1, proposal_id, vault_id, recipient_id],
        vec![nssa_core::account::Nonce(nonce_m1)],
        Instruction::Execute { create_key, proposal_index: 2 },
    ).unwrap();
    let ws = WitnessSet::for_message(&msg, &[&key1]);
    submit_tx(&client, PublicTransaction::new(msg, ws)).await;

    // ── Verify final state ──────────────────────────────────────────────
    println!("\n═══ STEP 9: Verify results ═══");

    let proposal = get_proposal(&client, proposal_id).await;
    assert_eq!(proposal.status, ProposalStatus::Executed, "Proposal should be executed");
    println!("  ✅ Proposal marked as executed");

    let state = get_multisig_state(&client, multisig_state_id).await;
    assert_eq!(state.transaction_index, 2, "transaction_index should be 2");

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