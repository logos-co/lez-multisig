// Execute handler — executes a fully-approved proposal by emitting a ChainedCall.
//
// The multisig doesn't execute actions directly. It builds a ChainedCall
// to the target program specified in the proposal, delegating actual execution.
//
// Expected accounts:
// - accounts[0]: multisig_state PDA (read threshold/membership)
// - accounts[1]: executor (must be authorized signer, must be member)
// - accounts[2]: proposal PDA account (owned by multisig program)
// - accounts[3..]: target accounts to pass to the ChainedCall

use nssa_core::account::{Account, AccountWithMetadata};
use nssa_core::program::{ChainedCall, PdaSeed};
use multisig_core::{ConfigAction, MultisigState, Proposal, ProposalStatus};

pub fn handle(
    accounts: &[AccountWithMetadata],
    _proposal_index: u64,
) -> (Vec<AccountWithMetadata>, Vec<ChainedCall>) {
    assert!(accounts.len() >= 3, "Execute requires at least multisig_state + executor + proposal");

    let multisig_account = &accounts[0];
    let executor_account = &accounts[1];
    let proposal_account = &accounts[2];
    let target_accounts = &accounts[3..];

    assert!(executor_account.is_authorized, "Executor must sign the transaction");

    // Read multisig state
    let state_data: Vec<u8> = multisig_account.account.data.clone().into();
    let mut state: MultisigState = borsh::from_slice(&state_data)
        .expect("Failed to deserialize multisig state");

    let executor_id = *executor_account.account_id.value();
    assert!(state.is_member(&executor_id), "Executor is not a multisig member");

    // Read proposal
    let proposal_data: Vec<u8> = proposal_account.account.data.clone().into();
    let mut proposal: Proposal = borsh::from_slice(&proposal_data)
        .expect("Failed to deserialize proposal");

    assert_eq!(proposal.multisig_create_key, state.create_key, "Proposal does not belong to this multisig");
    assert_eq!(proposal.status, ProposalStatus::Active, "Proposal is not active");
    assert!(
        proposal.has_threshold(state.threshold),
        "Proposal does not have enough approvals: need {}, have {}",
        state.threshold,
        proposal.approved.len()
    );

    // Mark as executed
    proposal.status = ProposalStatus::Executed;

    // Handle config change vs transfer proposal
    if let Some(config_action) = &proposal.config_action {
        // Config change: modify MultisigState directly, no ChainedCall
        assert_eq!(
            target_accounts.len(), 0,
            "Config change proposals should not have target accounts"
        );

        match config_action {
            ConfigAction::AddMember { new_member } => {
                assert!(!state.is_member(new_member), "Account is already a member");
                assert!(state.member_count < 10, "Maximum 10 members");
                state.members.push(*new_member);
                state.member_count += 1;
            }
            ConfigAction::RemoveMember { member } => {
                assert!(state.is_member(member), "Account is not a member");
                assert!(
                    state.member_count - 1 >= state.threshold,
                    "Cannot remove member: would make member count ({}) less than threshold ({})",
                    state.member_count - 1,
                    state.threshold
                );
                state.members.retain(|m| m != member);
                state.member_count -= 1;
            }
            ConfigAction::ChangeThreshold { new_threshold } => {
                assert!(*new_threshold >= 1, "Threshold must be at least 1");
                assert!(
                    *new_threshold <= state.member_count,
                    "Threshold ({}) cannot exceed member count ({})",
                    new_threshold,
                    state.member_count
                );
                state.threshold = *new_threshold;
            }
        }

        // Write back updated state
        let state_bytes = borsh::to_vec(&state).unwrap();
        let mut multisig_post = multisig_account.account.clone();
        multisig_post.data = state_bytes.try_into().unwrap();

        let proposal_bytes = borsh::to_vec(&proposal).unwrap();
        let mut proposal_post = proposal_account.account.clone();
        proposal_post.data = proposal_bytes.try_into().unwrap();

        let executor_post = executor_account.account.clone();

        let wrap = |acc: Account, orig: &AccountWithMetadata| AccountWithMetadata {
            account: acc, account_id: orig.account_id, is_authorized: false,
        };
        (
            vec![
                wrap(multisig_post, multisig_account),
                wrap(executor_post, executor_account),
                wrap(proposal_post, proposal_account),
            ],
            vec![],
        )
    } else {
        // Transfer proposal: emit ChainedCall
        assert_eq!(
            target_accounts.len(),
            proposal.target_account_count as usize,
            "Expected {} target accounts, got {}",
            proposal.target_account_count,
            target_accounts.len()
        );

        let target_program_id = proposal.target_program_id.clone();
        let target_instruction_data = proposal.target_instruction_data.clone();
        let pda_seeds: Vec<PdaSeed> = proposal.pda_seeds.iter().map(|s| PdaSeed::new(*s)).collect();
        let authorized_indices = proposal.authorized_indices.clone();

        let proposal_bytes = borsh::to_vec(&proposal).unwrap();
        let mut proposal_post = proposal_account.account.clone();
        proposal_post.data = proposal_bytes.try_into().unwrap();

        let chained_pre_states: Vec<AccountWithMetadata> = target_accounts
            .iter()
            .enumerate()
            .map(|(i, acc)| {
                let mut acc = acc.clone();
                if authorized_indices.contains(&(i as u8)) {
                    acc.is_authorized = true;
                }
                acc
            })
            .collect();

        let chained_call = ChainedCall {
            program_id: target_program_id,
            instruction_data: target_instruction_data,
            pre_states: chained_pre_states,
            pda_seeds,
        };

        let wrap = |acc: Account, orig: &AccountWithMetadata| AccountWithMetadata {
            account: acc, account_id: orig.account_id, is_authorized: false,
        };

        let multisig_post = multisig_account.account.clone();
        let executor_post = executor_account.account.clone();

        let mut accounts_out = vec![
            wrap(multisig_post, multisig_account),
            wrap(executor_post, executor_account),
            wrap(proposal_post, proposal_account),
        ];

        for target in target_accounts {
            accounts_out.push(target.clone());
        }

        (accounts_out, vec![chained_call])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nssa_core::account::{Account, AccountId};
    use nssa_core::program::ProgramId;
    use multisig_core::{MultisigState, Proposal, ProposalStatus};

    fn make_account(id: &[u8; 32], data: Vec<u8>, authorized: bool) -> AccountWithMetadata {
        let mut account = Account::default();
        account.data = data.try_into().unwrap();
        AccountWithMetadata {
            account_id: AccountId::new(*id),
            account,
            is_authorized: authorized,
        }
    }

    fn make_state(threshold: u8, members: Vec<[u8; 32]>) -> Vec<u8> {
        borsh::to_vec(&MultisigState::new([0u8; 32], threshold, members)).unwrap()
    }

    fn make_proposal_with_approvals(approvals: Vec<[u8; 32]>, target_account_count: u8) -> Vec<u8> {
        let fake_program_id: ProgramId = [42u32; 8];
        let mut proposal = Proposal::new(
            1,
            approvals[0],
            [0u8; 32],
            fake_program_id,
            vec![0u32],
            target_account_count,
            vec![],
            vec![0u8], // first target account is authorized
        );
        for approver in &approvals[1..] {
            proposal.approve(*approver);
        }
        borsh::to_vec(&proposal).unwrap()
    }

    #[test]
    fn test_execute_emits_chained_call() {
        let members = vec![[1u8; 32], [2u8; 32], [3u8; 32]];
        let state_data = make_state(2, members);
        // 2 approvals (member 1 auto, member 2 added)
        let proposal_data = make_proposal_with_approvals(vec![[1u8; 32], [2u8; 32]], 1);

        let accounts = vec![
            make_account(&[10u8; 32], state_data, false),   // multisig state
            make_account(&[1u8; 32], vec![], true),           // executor (member)
            make_account(&[20u8; 32], proposal_data, false),  // proposal PDA
            make_account(&[30u8; 32], vec![], false),          // target account
        ];

        let (accounts_out, chained) = handle(&accounts, 1);

        assert_eq!(chained.len(), 1);
        assert_eq!(accounts_out.len(), 4);

        // Proposal should be marked executed
        let proposal: Proposal = borsh::from_slice(
            &Vec::from(accounts_out[2].account.data.clone())
        ).unwrap();
        assert_eq!(proposal.status, ProposalStatus::Executed);

        // ChainedCall should have 1 pre_state with is_authorized=true
        assert_eq!(chained[0].pre_states.len(), 1);
        assert!(chained[0].pre_states[0].is_authorized);
    }

    #[test]
    #[should_panic(expected = "enough approvals")]
    fn test_execute_below_threshold_fails() {
        let members = vec![[1u8; 32], [2u8; 32], [3u8; 32]];
        let state_data = make_state(2, members);
        // Only 1 approval (proposer only)
        let proposal_data = make_proposal_with_approvals(vec![[1u8; 32]], 1);

        let accounts = vec![
            make_account(&[10u8; 32], state_data, false),
            make_account(&[1u8; 32], vec![], true),
            make_account(&[20u8; 32], proposal_data, false),
            make_account(&[30u8; 32], vec![], false),
        ];

        handle(&accounts, 1);
    }

    #[test]
    #[should_panic(expected = "Expected 1 target accounts, got 0")]
    fn test_execute_wrong_account_count_fails() {
        let members = vec![[1u8; 32], [2u8; 32]];
        let state_data = make_state(2, members);
        let proposal_data = make_proposal_with_approvals(vec![[1u8; 32], [2u8; 32]], 1);

        // Missing the target account
        let accounts = vec![
            make_account(&[10u8; 32], state_data, false),
            make_account(&[1u8; 32], vec![], true),
            make_account(&[20u8; 32], proposal_data, false),
            // no target account!
        ];

        handle(&accounts, 1);
    }

    #[test]
    #[should_panic(expected = "not a multisig member")]
    fn test_execute_non_member_fails() {
        let members = vec![[1u8; 32], [2u8; 32]];
        let state_data = make_state(2, members);
        let proposal_data = make_proposal_with_approvals(vec![[1u8; 32], [2u8; 32]], 1);

        let accounts = vec![
            make_account(&[10u8; 32], state_data, false),
            make_account(&[99u8; 32], vec![], true), // NOT a member
            make_account(&[20u8; 32], proposal_data, false),
            make_account(&[30u8; 32], vec![], false),
        ];

        handle(&accounts, 1);
    }

    // -- Config action tests --

    fn make_config_proposal(approvals: Vec<[u8; 32]>, action: ConfigAction) -> Vec<u8> {
        let mut proposal = Proposal::new_config(
            1,
            approvals[0],
            [0u8; 32],
            action,
        );
        for approver in &approvals[1..] {
            proposal.approve(*approver);
        }
        borsh::to_vec(&proposal).unwrap()
    }

    #[test]
    fn test_execute_add_member() {
        let members = vec![[1u8; 32], [2u8; 32], [3u8; 32]];
        let state_data = make_state(2, members);
        let proposal_data = make_config_proposal(
            vec![[1u8; 32], [2u8; 32]],
            ConfigAction::AddMember { new_member: [4u8; 32] },
        );

        let accounts = vec![
            make_account(&[10u8; 32], state_data, false),
            make_account(&[1u8; 32], vec![], true),
            make_account(&[20u8; 32], proposal_data, false),
        ];

        let (accounts_out, chained) = handle(&accounts, 1);

        assert!(chained.is_empty());
        let state: MultisigState = borsh::from_slice(
            &Vec::from(accounts_out[0].account.data.clone())
        ).unwrap();
        assert_eq!(state.member_count, 4);
        assert!(state.members.contains(&[4u8; 32]));
    }

    #[test]
    fn test_execute_remove_member() {
        let members = vec![[1u8; 32], [2u8; 32], [3u8; 32]];
        let state_data = make_state(2, members);
        let proposal_data = make_config_proposal(
            vec![[1u8; 32], [2u8; 32]],
            ConfigAction::RemoveMember { member: [3u8; 32] },
        );

        let accounts = vec![
            make_account(&[10u8; 32], state_data, false),
            make_account(&[1u8; 32], vec![], true),
            make_account(&[20u8; 32], proposal_data, false),
        ];

        let (accounts_out, chained) = handle(&accounts, 1);

        assert!(chained.is_empty());
        let state: MultisigState = borsh::from_slice(
            &Vec::from(accounts_out[0].account.data.clone())
        ).unwrap();
        assert_eq!(state.member_count, 2);
        assert!(!state.members.contains(&[3u8; 32]));
    }

    #[test]
    #[should_panic(expected = "Cannot remove member")]
    fn test_execute_remove_member_would_break_threshold() {
        let members = vec![[1u8; 32], [2u8; 32]];
        let state_data = make_state(2, members);
        let proposal_data = make_config_proposal(
            vec![[1u8; 32], [2u8; 32]],
            ConfigAction::RemoveMember { member: [2u8; 32] },
        );

        let accounts = vec![
            make_account(&[10u8; 32], state_data, false),
            make_account(&[1u8; 32], vec![], true),
            make_account(&[20u8; 32], proposal_data, false),
        ];

        handle(&accounts, 1);
    }

    #[test]
    fn test_execute_change_threshold() {
        let members = vec![[1u8; 32], [2u8; 32], [3u8; 32]];
        let state_data = make_state(2, members);
        let proposal_data = make_config_proposal(
            vec![[1u8; 32], [2u8; 32]],
            ConfigAction::ChangeThreshold { new_threshold: 3 },
        );

        let accounts = vec![
            make_account(&[10u8; 32], state_data, false),
            make_account(&[1u8; 32], vec![], true),
            make_account(&[20u8; 32], proposal_data, false),
        ];

        let (accounts_out, chained) = handle(&accounts, 1);

        assert!(chained.is_empty());
        let state: MultisigState = borsh::from_slice(
            &Vec::from(accounts_out[0].account.data.clone())
        ).unwrap();
        assert_eq!(state.threshold, 3);
    }

    #[test]
    #[should_panic(expected = "cannot exceed member count")]
    fn test_execute_change_threshold_too_high() {
        let members = vec![[1u8; 32], [2u8; 32], [3u8; 32]];
        let state_data = make_state(2, members);
        let proposal_data = make_config_proposal(
            vec![[1u8; 32], [2u8; 32]],
            ConfigAction::ChangeThreshold { new_threshold: 5 },
        );

        let accounts = vec![
            make_account(&[10u8; 32], state_data, false),
            make_account(&[1u8; 32], vec![], true),
            make_account(&[20u8; 32], proposal_data, false),
        ];

        handle(&accounts, 1);
    }
}
