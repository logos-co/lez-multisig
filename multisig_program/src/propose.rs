// Propose handler — creates a new proposal as a separate PDA account.
//
// Expected accounts:
// - accounts[0]: multisig_state PDA (read membership, increment tx_index)
// - accounts[1]: proposer (must be authorized signer, must be member)
// - accounts[2]: proposal PDA account (must be Account::default() = uninitialized)

use nssa_core::account::{Account, AccountWithMetadata};
use nssa_core::program::{ChainedCall, InstructionData, ProgramId};
use multisig_core::{MultisigState, Proposal};

pub fn handle(
    accounts: &[AccountWithMetadata],
    target_program_id: &ProgramId,
    target_instruction_data: &InstructionData,
    target_account_count: u8,
    pda_seeds: &[[u8; 32]],
    authorized_indices: &[u8],
) -> (Vec<Account>, Vec<ChainedCall>) {
    assert!(accounts.len() >= 3, "Propose requires multisig_state + proposer + proposal accounts");

    let multisig_account = &accounts[0];
    let proposer_account = &accounts[1];
    let proposal_account = &accounts[2];

    assert!(proposer_account.is_authorized, "Proposer must sign the transaction");

    // Proposal account must be uninitialized
    assert!(
        proposal_account.account == Account::default(),
        "Proposal account must be uninitialized"
    );

    // Read and update multisig state (increment transaction_index)
    let state_data: Vec<u8> = multisig_account.account.data.clone().into();
    let mut state: MultisigState = borsh::from_slice(&state_data)
        .expect("Failed to deserialize multisig state");

    let proposer_id = *proposer_account.account_id.value();
    assert!(state.is_member(&proposer_id), "Proposer is not a multisig member");

    let proposal_index = state.next_proposal_index();

    // Create the proposal
    let proposal = Proposal::new(
        proposal_index,
        proposer_id,
        state.create_key,
        target_program_id.clone(),
        target_instruction_data.clone(),
        target_account_count,
        pda_seeds.to_vec(),
        authorized_indices.to_vec(),
    );

    // Serialize updated multisig state (with incremented tx_index)
    let state_bytes = borsh::to_vec(&state).unwrap();
    let mut multisig_post = multisig_account.account.clone();
    multisig_post.data = state_bytes.try_into().unwrap();

    // Serialize proposal into new account (claim applied in lib.rs via AutoClaim)
    let proposal_bytes = borsh::to_vec(&proposal).unwrap();
    let mut proposal_post = Account::default();
    proposal_post.data = proposal_bytes.try_into().unwrap();

    let proposer_post = proposer_account.account.clone();

    (
        vec![multisig_post, proposer_post, proposal_post],
        vec![],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use nssa_core::account::{Account, AccountId};
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

    fn make_state(threshold: u8, members: Vec<[u8; 32]>) -> Vec<u8> {
        borsh::to_vec(&MultisigState::new([0u8; 32], threshold, members)).unwrap()
    }

    #[test]
    fn test_propose_creates_proposal_and_increments_index() {
        let members = vec![[1u8; 32], [2u8; 32], [3u8; 32]];
        let state_data = make_state(2, members.clone());

        let accounts = vec![
            make_account(&[10u8; 32], state_data, false), // multisig state
            make_account(&[1u8; 32], vec![], true),         // proposer (member)
            make_account(&[20u8; 32], vec![], false),        // proposal PDA (uninitialized)
        ];

        let program_id: ProgramId = [42u32; 8];
        let (accounts_out, chained) = handle(
            &accounts,
            &program_id,
            &vec![0u32],
            1,
            &[],
            &[],
        );

        assert!(chained.is_empty());
        assert_eq!(accounts_out.len(), 3);

        // Multisig state should have incremented tx index
        let state: MultisigState = borsh::from_slice(
            &Vec::from(accounts_out[0].data.clone())
        ).unwrap();
        assert_eq!(state.transaction_index, 1);

        // Proposal should exist with proposer auto-approved
        let proposal: Proposal = borsh::from_slice(
            &Vec::from(accounts_out[2].data.clone())
        ).unwrap();
        assert_eq!(proposal.index, 1);
        assert_eq!(proposal.proposer, [1u8; 32]);
        assert_eq!(proposal.approved, vec![[1u8; 32]]);
        assert_eq!(proposal.status, multisig_core::ProposalStatus::Active);
    }

    #[test]
    #[should_panic(expected = "not a multisig member")]
    fn test_propose_non_member_fails() {
        let members = vec![[1u8; 32], [2u8; 32]];
        let state_data = make_state(2, members);

        let accounts = vec![
            make_account(&[10u8; 32], state_data, false),
            make_account(&[99u8; 32], vec![], true), // NOT a member
            make_account(&[20u8; 32], vec![], false),
        ];

        let program_id: ProgramId = [42u32; 8];
        handle(&accounts, &program_id, &vec![0u32], 1, &[], &[]);
    }

    #[test]
    #[should_panic(expected = "must sign")]
    fn test_propose_unsigned_fails() {
        let members = vec![[1u8; 32], [2u8; 32]];
        let state_data = make_state(2, members);

        let accounts = vec![
            make_account(&[10u8; 32], state_data, false),
            make_account(&[1u8; 32], vec![], false), // not authorized
            make_account(&[20u8; 32], vec![], false),
        ];

        let program_id: ProgramId = [42u32; 8];
        handle(&accounts, &program_id, &vec![0u32], 1, &[], &[]);
    }
}
