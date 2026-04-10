// Approve handler — any member approves an existing proposal
//
// Expected accounts:
// - accounts[0]: multisig_state PDA (read membership)
// - accounts[1]: approver account (must be authorized = is a signer)
// - accounts[2]: proposal PDA account (owned by multisig program)

use nssa_core::account::{Account, AccountWithMetadata};
use nssa_core::program::ChainedCall;
use multisig_core::{MultisigState, Proposal, ProposalStatus};

pub fn handle(
    accounts: &[AccountWithMetadata],
    _proposal_index: u64,
) -> (Vec<Account>, Vec<ChainedCall>) {
    assert!(accounts.len() >= 3, "Approve requires multisig_state + approver + proposal accounts");

    let multisig_account = &accounts[0];
    let approver_account = &accounts[1];
    let proposal_account = &accounts[2];

    assert!(approver_account.is_authorized, "Approver must sign the transaction");

    // Read multisig state for membership check
    let state_data: Vec<u8> = multisig_account.account.data.clone().into();
    let state: MultisigState = borsh::from_slice(&state_data)
        .expect("Failed to deserialize multisig state");

    let approver_id = *approver_account.account_id.value();
    assert!(state.is_member(&approver_id), "Approver is not a multisig member");

    // Read and update proposal
    let proposal_data: Vec<u8> = proposal_account.account.data.clone().into();
    let mut proposal: Proposal = borsh::from_slice(&proposal_data)
        .expect("Failed to deserialize proposal");

    assert_eq!(proposal.multisig_create_key, state.create_key, "Proposal does not belong to this multisig");
    assert_eq!(proposal.status, ProposalStatus::Active, "Proposal is not active");

    let is_new = proposal.approve(approver_id);
    assert!(is_new, "Member has already approved this proposal");

    // Write back proposal
    let proposal_bytes = borsh::to_vec(&proposal).unwrap();
    let mut proposal_post = proposal_account.account.clone();
    proposal_post.data = proposal_bytes.try_into().unwrap();

    // Return account for every pre_state
    let multisig_post = multisig_account.account.clone();
    let approver_post = approver_account.account.clone();

    (
        vec![multisig_post, approver_post, proposal_post],
        vec![],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use nssa_core::account::{Account, AccountId};
    use nssa_core::program::ProgramId;
    use multisig_core::MultisigState;

    fn make_account(id: &[u8; 32], data: Vec<u8>, authorized: bool) -> AccountWithMetadata {
        let mut account = Account::default();
        account.data = data.try_into().unwrap();
        AccountWithMetadata {
            account_id: AccountId::new(*id),
            account,
            is_authorized: authorized,
        }
    }

    fn make_multisig_state(threshold: u8, members: Vec<[u8; 32]>) -> Vec<u8> {
        let mut state = MultisigState::new([0u8; 32], threshold, members);
        state.transaction_index = 1; // proposal exists
        borsh::to_vec(&state).unwrap()
    }

    fn make_proposal(proposer: [u8; 32]) -> Vec<u8> {
        let fake_program_id: ProgramId = [42u32; 8];
        let proposal = Proposal::new(
            1,
            proposer,
            [0u8; 32], // create_key matches multisig
            fake_program_id,
            vec![0u32],
            1,
            vec![],
            vec![],
        );
        borsh::to_vec(&proposal).unwrap()
    }

    #[test]
    fn test_approve_adds_approval() {
        let members = vec![[1u8; 32], [2u8; 32], [3u8; 32]];
        let state_data = make_multisig_state(2, members);
        let proposal_data = make_proposal([1u8; 32]);

        let accounts = vec![
            make_account(&[10u8; 32], state_data, false),
            make_account(&[2u8; 32], vec![], true),
            make_account(&[20u8; 32], proposal_data, false),
        ];

        let (post_states, _) = handle(&accounts, 1);

        let proposal: Proposal = borsh::from_slice(&Vec::from(post_states[2].data.clone())).unwrap();
        assert_eq!(proposal.approved.len(), 2);
        assert!(proposal.approved.contains(&[1u8; 32]));
        assert!(proposal.approved.contains(&[2u8; 32]));
    }

    #[test]
    #[should_panic(expected = "already approved")]
    fn test_approve_duplicate_fails() {
        let members = vec![[1u8; 32], [2u8; 32]];
        let state_data = make_multisig_state(2, members);
        let proposal_data = make_proposal([1u8; 32]);

        let accounts = vec![
            make_account(&[10u8; 32], state_data, false),
            make_account(&[1u8; 32], vec![], true),
            make_account(&[20u8; 32], proposal_data, false),
        ];

        handle(&accounts, 1);
    }
}
