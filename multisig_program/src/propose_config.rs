// ProposeConfig handler — creates a config change proposal (add/remove member, change threshold).
//
// Expected accounts:
// - accounts[0]: multisig_state PDA (read membership, increment tx_index)
// - accounts[1]: proposer (must be authorized signer, must be member)
// - accounts[2]: proposal PDA account (must be Account::default() = uninitialized)

use nssa_core::account::{Account, AccountWithMetadata};
use nssa_core::program::ChainedCall;
use multisig_core::{ConfigAction, MultisigState, Proposal};

pub fn handle(
    accounts: &[AccountWithMetadata],
    config_action: ConfigAction,
) -> (Vec<Account>, Vec<ChainedCall>) {
    assert!(accounts.len() >= 3, "ProposeConfig requires multisig_state + proposer + proposal accounts");

    let multisig_account = &accounts[0];
    let proposer_account = &accounts[1];
    let proposal_account = &accounts[2];

    assert!(proposer_account.is_authorized, "Proposer must sign the transaction");

    assert!(
        proposal_account.account == Account::default(),
        "Proposal account must be uninitialized"
    );

    let state_data: Vec<u8> = multisig_account.account.data.clone().into();
    let mut state: MultisigState = borsh::from_slice(&state_data)
        .expect("Failed to deserialize multisig state");

    let proposer_id = *proposer_account.account_id.value();
    assert!(state.is_member(&proposer_id), "Proposer is not a multisig member");

    // Basic validation at propose time
    match &config_action {
        ConfigAction::AddMember { new_member } => {
            assert!(!state.is_member(new_member), "Account is already a member");
            assert!(state.member_count < 10, "Maximum 10 members");
        }
        ConfigAction::RemoveMember { member } => {
            assert!(state.is_member(member), "Account is not a member");
        }
        ConfigAction::ChangeThreshold { new_threshold } => {
            assert!(*new_threshold >= 1, "Threshold must be at least 1");
        }
    }

    let proposal_index = state.next_proposal_index();

    let proposal = Proposal::new_config(
        proposal_index,
        proposer_id,
        state.create_key,
        config_action,
    );

    // Serialize updated multisig state
    let state_bytes = borsh::to_vec(&state).unwrap();
    let mut multisig_post = multisig_account.account.clone();
    multisig_post.data = state_bytes.try_into().unwrap();

    // Serialize proposal into new account
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
    fn test_propose_add_member() {
        let members = vec![[1u8; 32], [2u8; 32], [3u8; 32]];
        let state_data = make_state(2, members);

        let accounts = vec![
            make_account(&[10u8; 32], state_data, false),
            make_account(&[1u8; 32], vec![], true),
            make_account(&[20u8; 32], vec![], false),
        ];

        let action = ConfigAction::AddMember { new_member: [4u8; 32] };
        let (post_states, chained) = handle(&accounts, action);

        assert!(chained.is_empty());
        assert_eq!(post_states.len(), 3);

        let proposal: Proposal = borsh::from_slice(
            &Vec::from(post_states[2].data.clone())
        ).unwrap();
        assert_eq!(proposal.config_action, Some(ConfigAction::AddMember { new_member: [4u8; 32] }));
        assert_eq!(proposal.target_account_count, 0);
    }

    #[test]
    fn test_propose_remove_member() {
        let members = vec![[1u8; 32], [2u8; 32], [3u8; 32]];
        let state_data = make_state(2, members);

        let accounts = vec![
            make_account(&[10u8; 32], state_data, false),
            make_account(&[1u8; 32], vec![], true),
            make_account(&[20u8; 32], vec![], false),
        ];

        let action = ConfigAction::RemoveMember { member: [2u8; 32] };
        let (post_states, chained) = handle(&accounts, action);

        assert!(chained.is_empty());
        let proposal: Proposal = borsh::from_slice(
            &Vec::from(post_states[2].data.clone())
        ).unwrap();
        assert_eq!(proposal.config_action, Some(ConfigAction::RemoveMember { member: [2u8; 32] }));
    }

    #[test]
    fn test_propose_change_threshold() {
        let members = vec![[1u8; 32], [2u8; 32], [3u8; 32]];
        let state_data = make_state(2, members);

        let accounts = vec![
            make_account(&[10u8; 32], state_data, false),
            make_account(&[1u8; 32], vec![], true),
            make_account(&[20u8; 32], vec![], false),
        ];

        let action = ConfigAction::ChangeThreshold { new_threshold: 3 };
        let (post_states, _) = handle(&accounts, action);

        let proposal: Proposal = borsh::from_slice(
            &Vec::from(post_states[2].data.clone())
        ).unwrap();
        assert_eq!(proposal.config_action, Some(ConfigAction::ChangeThreshold { new_threshold: 3 }));
    }

    #[test]
    #[should_panic(expected = "already a member")]
    fn test_propose_add_existing_member_fails() {
        let members = vec![[1u8; 32], [2u8; 32]];
        let state_data = make_state(2, members);

        let accounts = vec![
            make_account(&[10u8; 32], state_data, false),
            make_account(&[1u8; 32], vec![], true),
            make_account(&[20u8; 32], vec![], false),
        ];

        handle(&accounts, ConfigAction::AddMember { new_member: [2u8; 32] });
    }

    #[test]
    #[should_panic(expected = "not a member")]
    fn test_propose_remove_non_member_fails() {
        let members = vec![[1u8; 32], [2u8; 32]];
        let state_data = make_state(2, members);

        let accounts = vec![
            make_account(&[10u8; 32], state_data, false),
            make_account(&[1u8; 32], vec![], true),
            make_account(&[20u8; 32], vec![], false),
        ];

        handle(&accounts, ConfigAction::RemoveMember { member: [99u8; 32] });
    }

    #[test]
    #[should_panic(expected = "at least 1")]
    fn test_propose_change_threshold_zero_fails() {
        let members = vec![[1u8; 32], [2u8; 32]];
        let state_data = make_state(2, members);

        let accounts = vec![
            make_account(&[10u8; 32], state_data, false),
            make_account(&[1u8; 32], vec![], true),
            make_account(&[20u8; 32], vec![], false),
        ];

        handle(&accounts, ConfigAction::ChangeThreshold { new_threshold: 0 });
    }

    #[test]
    #[should_panic(expected = "not a multisig member")]
    fn test_propose_config_non_member_fails() {
        let members = vec![[1u8; 32], [2u8; 32]];
        let state_data = make_state(2, members);

        let accounts = vec![
            make_account(&[10u8; 32], state_data, false),
            make_account(&[99u8; 32], vec![], true),
            make_account(&[20u8; 32], vec![], false),
        ];

        handle(&accounts, ConfigAction::AddMember { new_member: [4u8; 32] });
    }
}
