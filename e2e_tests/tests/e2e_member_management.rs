//! End-to-end test for member management (add/remove member, change threshold).
//!
//! Flow:
//! 1. Deploy multisig program
//! 2. Create a 2-of-3 multisig
//! 3. Propose add member (4th) → approve → execute → verify N=4
//! 4. Propose change threshold to 3 → approve → execute → verify M=3
//! 5. Propose remove member → approve (need 3 now) → execute → verify N=3
//! 6. Test guard: try to remove when N would drop below M → should fail

use std::time::Duration;

use nssa::{
    AccountId, PrivateKey, ProgramDeploymentTransaction, PublicKey, PublicTransaction,
    program::Program,
    public_transaction::{Message, WitnessSet},
};
use multisig_core::{Instruction, MultisigState, Proposal, ProposalStatus};
use lez_multisig_ffi::{compute_multisig_state_pda, compute_proposal_pda};
use sequencer_service_rpc::{SequencerClient, SequencerClientBuilder, RpcClient as _};
use common::transaction::NSSATransaction;

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
                    panic!("❌ Transaction {} not included after {:?}", hex::encode(tx_hash.0), max_wait);
                }
            }
        }
    }
}

/// Submit a tx that we expect to fail (not get included).
/// Returns true if it was correctly rejected/not included.
async fn submit_tx_expect_failure(client: &SequencerClient, tx: PublicTransaction) -> bool {
    match client.send_transaction(NSSATransaction::Public(tx)).await {
        Err(_) => {
            println!("  ✅ Transaction rejected at submission (expected)");
            return true;
        }
        Ok(response) => {
            let tx_hash = response;
            println!("  tx_hash: {} (expecting non-inclusion)", hex::encode(tx_hash.0));
            // Wait a bit and check it wasn't included
            tokio::time::sleep(Duration::from_secs(BLOCK_WAIT_SECS * 2)).await;
            match client.get_transaction(tx_hash.clone()).await {
                Ok(resp) if resp.is_some() => {
                    println!("  ❌ Transaction was unexpectedly included!");
                    false
                }
                _ => {
                    println!("  ✅ Transaction not included (expected failure)");
                    true
                }
            }
        }
    }
}

async fn get_nonce(client: &SequencerClient, account_id: AccountId) -> Nonce {
    client.get_account(account_id).await
        .map(|r| r.nonce.0)
        .unwrap_or(0)
}

async fn get_multisig_state(client: &SequencerClient, state_id: AccountId) -> MultisigState {
    let account = client.get_account(state_id).await.expect("Failed to get multisig state");
    let data: Vec<u8> = account.data.into();
    borsh::from_slice(&data).expect("Failed to deserialize multisig state")
}

async fn get_proposal(client: &SequencerClient, proposal_id: AccountId) -> Proposal {
    let account = client.get_account(proposal_id).await.expect("Failed to get proposal");
    let data: Vec<u8> = account.data.into();
    borsh::from_slice(&data).expect("Failed to deserialize proposal")
}

fn deploy_program(bytecode: Vec<u8>) -> (ProgramDeploymentTransaction, nssa::ProgramId) {
    let program = Program::new(bytecode.clone()).expect("Invalid program");
    let program_id = program.id();
    let msg = nssa::program_deployment_transaction::Message::new(bytecode);
    (ProgramDeploymentTransaction::new(msg), program_id)
}

/// Helper: propose a config change, approve with enough members, execute, return updated state.
async fn propose_approve_execute_config(
    client: &SequencerClient,
    program_id: nssa::ProgramId,
    create_key: &[u8; 32],
    multisig_state_id: AccountId,
    instruction: Instruction,
    proposer_key: &PrivateKey,
    approver_keys: &[&PrivateKey], // additional approvers beyond proposer
    proposal_index: u64,
) -> MultisigState {
    let proposer_id = account_id_from_key(proposer_key);
    let proposal_pda = compute_proposal_pda(&program_id, create_key, proposal_index);

    // Propose
    println!("  📝 Proposing (index {})...", proposal_index);
    let nonce = get_nonce(client, proposer_id).await;
    let msg = Message::try_new(
        program_id,
        vec![multisig_state_id, proposer_id, proposal_pda],
        vec![nonce],
        instruction,
    ).unwrap();
    let ws = WitnessSet::for_message(&msg, &[proposer_key]);
    submit_tx(client, PublicTransaction::new(msg, ws)).await;

    // Approve with each additional approver
    for approver_key in approver_keys {
        let approver_id = account_id_from_key(approver_key);
        println!("  👍 Approving with {}...", approver_id);
        let nonce = get_nonce(client, approver_id).await;
        let msg = Message::try_new(
            program_id,
            vec![multisig_state_id, approver_id, proposal_pda],
            vec![nonce],
            Instruction::Approve { create_key: *create_key, proposal_index },
        ).unwrap();
        let ws = WitnessSet::for_message(&msg, &[approver_key]);
        submit_tx(client, PublicTransaction::new(msg, ws)).await;
    }

    // Execute
    println!("  ⚡ Executing...");
    let executor_id = proposer_id;
    let nonce = get_nonce(client, executor_id).await;
    let msg = Message::try_new(
        program_id,
        vec![multisig_state_id, executor_id, proposal_pda],
        vec![nonce],
        Instruction::Execute { create_key: *create_key, proposal_index },
    ).unwrap();
    let ws = WitnessSet::for_message(&msg, &[proposer_key]);
    submit_tx(client, PublicTransaction::new(msg, ws)).await;

    // Verify proposal executed
    let proposal = get_proposal(client, proposal_pda).await;
    assert_eq!(proposal.status, ProposalStatus::Executed);

    get_multisig_state(client, multisig_state_id).await
}

#[tokio::test]
async fn test_member_management() {
    let client = sequencer_client();

    // ── Deploy multisig program ─────────────────────────────────────────
    println!("📦 Deploying multisig program...");
    let multisig_path = std::env::var("MULTISIG_PROGRAM")
        .unwrap_or_else(|_| panic!("MULTISIG_PROGRAM env var not set"));
    let multisig_bytecode = std::fs::read(&multisig_path)
        .unwrap_or_else(|_| panic!("Cannot read multisig binary at '{}'", multisig_path));
    let (deploy_tx, program_id) = deploy_program(multisig_bytecode);

    match client.send_transaction(NSSATransaction::ProgramDeployment(deploy_tx)).await {
        Ok(r) => {
            println!("  Deployed: {}", hex::encode(r.0));
            tokio::time::sleep(Duration::from_secs(BLOCK_WAIT_SECS)).await;
        }
        Err(e) => println!("  Deploy skipped (already deployed): {}", e),
    }

    // ── Create 2-of-3 multisig ─────────────────────────────────────────
    println!("\n═══ STEP 1: Create 2-of-3 multisig ═══");
    let key1 = PrivateKey::new_os_random();
    let key2 = PrivateKey::new_os_random();
    let key3 = PrivateKey::new_os_random();
    let m1 = account_id_from_key(&key1);
    let m2 = account_id_from_key(&key2);
    let m3 = account_id_from_key(&key3);

    let create_key: [u8; 32] = *AccountId::from(
        &PublicKey::new_from_private_key(&PrivateKey::new_os_random())
    ).value();

    let multisig_state_id = compute_multisig_state_pda(&program_id, &create_key);
    println!("  State PDA: {}", multisig_state_id);

    let msg = Message::try_new(
        program_id,
        vec![multisig_state_id, m1, m2, m3],
        vec![],
        Instruction::CreateMultisig {
            create_key,
            threshold: 2,
            members: vec![*m1.value(), *m2.value(), *m3.value()],
        },
    ).unwrap();
    let ws = WitnessSet::for_message(&msg, &[] as &[&PrivateKey]);
    submit_tx(&client, PublicTransaction::new(msg, ws)).await;

    let state = get_multisig_state(&client, multisig_state_id).await;
    assert_eq!(state.threshold, 2);
    assert_eq!(state.member_count, 3);
    println!("  ✅ 2-of-3 multisig created");

    // ── STEP 2: Add a 4th member ────────────────────────────────────────
    println!("\n═══ STEP 2: Add member (4th) ═══");
    let key4 = PrivateKey::new_os_random();
    let m4 = account_id_from_key(&key4);

    let state = propose_approve_execute_config(
        &client, program_id, &create_key, multisig_state_id,
        Instruction::ProposeAddMember { new_member: *m4.value(), create_key, proposal_index: 1 },
        &key1, &[&key2], // proposer=m1, approver=m2
        1,
    ).await;

    assert_eq!(state.member_count, 4, "Should have 4 members");
    assert!(state.members.contains(m4.value()), "New member should be in list");
    println!("  ✅ Member added, N=4");

    // ── STEP 3: Change threshold to 3 ──────────────────────────────────
    println!("\n═══ STEP 3: Change threshold to 3 ═══");
    let state = propose_approve_execute_config(
        &client, program_id, &create_key, multisig_state_id,
        Instruction::ProposeChangeThreshold { new_threshold: 3, create_key, proposal_index: 2 },
        &key1, &[&key2], // still 2-of-4 required for this proposal
        2,
    ).await;

    assert_eq!(state.threshold, 3, "Threshold should be 3");
    println!("  ✅ Threshold changed to M=3");

    // ── STEP 4: Remove member 4 (need 3 approvals now) ─────────────────
    println!("\n═══ STEP 4: Remove member 4 ═══");
    let state = propose_approve_execute_config(
        &client, program_id, &create_key, multisig_state_id,
        Instruction::ProposeRemoveMember { member: *m4.value(), create_key, proposal_index: 3 },
        &key1, &[&key2, &key3], // need 3 approvals: m1 + m2 + m3
        3,
    ).await;

    assert_eq!(state.member_count, 3, "Should have 3 members");
    assert!(!state.members.contains(m4.value()), "Removed member should be gone");
    println!("  ✅ Member removed, N=3");

    // ── STEP 5: Test guard — remove when N would drop below M ──────────
    println!("\n═══ STEP 5: Test threshold guard (N < M should fail) ═══");
    // Currently 3-of-3. Removing anyone would make N=2 < M=3.
    // The proposal creation should succeed, but execute should fail.
    let proposal_pda = compute_proposal_pda(&program_id, &create_key, 4);

    // Propose removal of m3
    let nonce = get_nonce(&client, m1).await;
    let msg = Message::try_new(
        program_id,
        vec![multisig_state_id, m1, proposal_pda],
        vec![nonce],
        Instruction::ProposeRemoveMember { member: *m3.value(), create_key, proposal_index: 4 },
    ).unwrap();
    let ws = WitnessSet::for_message(&msg, &[&key1]);
    submit_tx(&client, PublicTransaction::new(msg, ws)).await;
    println!("  📝 Proposal to remove member created");

    // Approve with m2 and m3
    for (key, id) in [(&key2, m2), (&key3, m3)] {
        let nonce = get_nonce(&client, id).await;
        let msg = Message::try_new(
            program_id,
            vec![multisig_state_id, id, proposal_pda],
            vec![nonce],
            Instruction::Approve { create_key, proposal_index: 4 },
        ).unwrap();
        let ws = WitnessSet::for_message(&msg, &[key]);
        submit_tx(&client, PublicTransaction::new(msg, ws)).await;
    }
    println!("  👍 3/3 approvals collected");

    // Execute should fail (N-1=2 < M=3)
    let nonce = get_nonce(&client, m1).await;
    let msg = Message::try_new(
        program_id,
        vec![multisig_state_id, m1, proposal_pda],
        vec![nonce],
        Instruction::Execute { create_key, proposal_index: 4 },
    ).unwrap();
    let ws = WitnessSet::for_message(&msg, &[&key1]);
    let failed = submit_tx_expect_failure(&client, PublicTransaction::new(msg, ws)).await;
    assert!(failed, "Execute should have failed — removing would make N < M");
    println!("  ✅ Guard works: cannot remove member when N would drop below M");

    // Verify state unchanged
    let state = get_multisig_state(&client, multisig_state_id).await;
    assert_eq!(state.member_count, 3);
    assert_eq!(state.threshold, 3);

    println!("\n🎉 Member management e2e test PASSED!");
    println!("   - Create 2-of-3 multisig ✅");
    println!("   - Add member (N=4) ✅");
    println!("   - Change threshold (M=3) ✅");
    println!("   - Remove member (N=3) ✅");
    println!("   - Threshold guard (N < M blocked) ✅");
}
