// CreateMultisig handler — initializes a new M-of-N multisig

use nssa_core::account::{Account, AccountWithMetadata};
use nssa_core::program::ChainedCall;
use multisig_core::MultisigState;

/// Handle CreateMultisig instruction
/// 
/// Expected accounts:
/// - accounts[0]: multisig_state (PDA, uninitialized) — derived from (program_id, create_key)
/// - accounts[1..N+1]: member accounts (must be Account::default() = uninitialized/fresh)
///
/// All member accounts are claimed by the multisig program during creation.
/// This means members must use fresh keypairs dedicated to this multisig.
/// After claiming, member accounts have program_owner = multisig_program_id,
/// which allows them to be included in subsequent instructions without
/// triggering LEZ validation rule 7.
///
/// Authorization: anyone can create a new multisig (create_key makes PDA unique)
pub fn handle(
    accounts: &[AccountWithMetadata],
    create_key: &[u8; 32],
    threshold: u8,
    members: &[[u8; 32]],
) -> (Vec<Account>, Vec<ChainedCall>) {
    // Validate inputs
    assert!(!members.is_empty(), "Multisig must have at least one member");
    assert!(threshold >= 1, "Threshold must be at least 1");
    assert!((threshold as usize) <= members.len(), "Threshold cannot exceed member count");
    assert!(members.len() <= 10, "Maximum 10 members for PoC");

    // We need multisig_state + all member accounts
    assert!(
        accounts.len() >= 1 + members.len(),
        "CreateMultisig requires multisig_state + {} member accounts, got {}",
        members.len(),
        accounts.len()
    );

    // Verify multisig state account is uninitialized
    assert!(
        accounts[0].account == Account::default(),
        "Multisig state account must be uninitialized"
    );

    // Verify each member account is uninitialized and matches the member list
    for (i, member_id) in members.iter().enumerate() {
        let member_account = &accounts[1 + i];
        assert!(
            member_account.account == Account::default(),
            "Member account {} must be uninitialized (fresh keypair required)",
            i
        );
        assert_eq!(
            member_account.account_id.value(),
            member_id,
            "Member account {} ID does not match member list",
            i
        );
    }

    // Create multisig state
    let state = MultisigState::new(*create_key, threshold, members.to_vec());
    
    let mut multisig_account = Account::default();
    let state_bytes = borsh::to_vec(&state).unwrap();
    multisig_account.data = state_bytes.try_into().unwrap();
    
    // Build accounts: multisig_state + all member accounts
    // Claiming member accounts satisfies LEZ Rule 7: the executor (a member) must be
    // owned by the multisig program for Execute to work.
    // Claim metadata is applied in lib.rs via AutoClaim.
    let mut result = vec![multisig_account];

    for i in 0..members.len() {
        result.push(accounts[1 + i].account.clone());
    }

    (result, vec![])
}

#[cfg(test)]
mod tests {
    use super::*;
    use nssa_core::account::{Account, AccountId};

    fn make_account(id: &[u8; 32], authorized: bool) -> AccountWithMetadata {
        AccountWithMetadata {
            account_id: AccountId::new(*id),
            account: Account::default(),
            is_authorized: authorized,
        }
    }

    #[test]
    fn test_create_multisig_2_of_3() {
        let create_key = [1u8; 32];
        let members: Vec<[u8; 32]> = vec![[10u8; 32], [11u8; 32], [12u8; 32]];

        let mut accounts = vec![make_account(&[99u8; 32], false)]; // state PDA
        for m in &members {
            accounts.push(make_account(m, false));
        }

        let (accounts_out, chained) = handle(&accounts, &create_key, 2, &members);

        assert!(chained.is_empty());
        // state + 3 member accounts
        assert_eq!(accounts_out.len(), 4);

        // Verify multisig state was written correctly
        let state: MultisigState = borsh::from_slice(
            &Vec::from(accounts_out[0].data.clone())
        ).unwrap();
        assert_eq!(state.threshold, 2);
        assert_eq!(state.member_count, 3);
        assert_eq!(state.members, members);
        assert_eq!(state.create_key, create_key);
        assert_eq!(state.transaction_index, 0);
    }

    #[test]
    #[should_panic(expected = "Threshold must be at least 1")]
    fn test_create_multisig_zero_threshold_fails() {
        let create_key = [1u8; 32];
        let members: Vec<[u8; 32]> = vec![[10u8; 32]];
        let mut accounts = vec![make_account(&[99u8; 32], false)];
        accounts.push(make_account(&[10u8; 32], false));
        handle(&accounts, &create_key, 0, &members);
    }

    #[test]
    #[should_panic(expected = "Threshold cannot exceed member count")]
    fn test_create_multisig_threshold_exceeds_members_fails() {
        let create_key = [1u8; 32];
        let members: Vec<[u8; 32]> = vec![[10u8; 32], [11u8; 32]];
        let mut accounts = vec![make_account(&[99u8; 32], false)];
        for m in &members { accounts.push(make_account(m, false)); }
        handle(&accounts, &create_key, 3, &members);
    }

    #[test]
    #[should_panic(expected = "Maximum 10 members")]
    fn test_create_multisig_too_many_members_fails() {
        let create_key = [1u8; 32];
        let members: Vec<[u8; 32]> = (0u8..11).map(|i| [i; 32]).collect();
        let mut accounts = vec![make_account(&[99u8; 32], false)];
        for m in &members { accounts.push(make_account(m, false)); }
        handle(&accounts, &create_key, 1, &members);
    }

    #[test]
    #[should_panic(expected = "must be uninitialized")]
    fn test_create_multisig_already_initialized_fails() {
        let create_key = [1u8; 32];
        let members: Vec<[u8; 32]> = vec![[10u8; 32]];

        // State account already has data
        let mut state_account = Account::default();
        state_account.data = vec![1u8; 10].try_into().unwrap();
        let accounts = vec![
            AccountWithMetadata {
                account_id: AccountId::new([99u8; 32]),
                account: state_account,
                is_authorized: false,
            },
            make_account(&[10u8; 32], false),
        ];
        handle(&accounts, &create_key, 1, &members);
    }
}
