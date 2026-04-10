pub mod create_multisig;
pub mod propose;
pub mod propose_config;
pub mod approve;
pub mod reject;
pub mod execute;

use nssa_core::program::ProgramId;
use multisig_core::ConfigAction;
use spel_framework::prelude::*;

/// Multisig program using #[spel_program] macro.
/// Uses external multisig_core::Instruction enum for dispatch.
#[lez_program(instruction = "multisig_core::Instruction")]
mod multisig_program {
    use super::*;

    /// Create a new M-of-N multisig.
    /// multisig_state is initialized as a PDA derived from create_key.
    #[instruction]
    pub fn create_multisig(
        #[account(init, pda = arg("create_key"))]
        multisig_state: AccountWithMetadata,
        member_accounts: Vec<AccountWithMetadata>,
        create_key: [u8; 32],
        threshold: u8,
        members: Vec<[u8; 32]>,
    ) -> SpelResult {
        let accounts: Vec<AccountWithMetadata> = std::iter::once(multisig_state)
            .chain(member_accounts.into_iter())
            .collect();
        let (accounts_out, chained_calls) =
            crate::create_multisig::handle(&accounts, &create_key, threshold, &members);

        // multisig_state: init PDA from create_key; member accounts: claimed via Authorized
        let claims: Vec<AutoClaim> =
            std::iter::once(AutoClaim::pda_from_seeds(&[&create_key[..]]))
                .chain(std::iter::repeat(AutoClaim::Claimed(Claim::Authorized)).take(members.len()))
                .collect();

        Ok(SpelOutput::execute(
            accounts_out.into_iter().zip(claims).collect::<Vec<_>>(),
            chained_calls,
        ))
    }

    /// Propose a new transaction.
    /// proposer must be a member signer. proposal is initialized as a new PDA.
    /// proposal PDA seeds: ["multisig_prop___", create_key, proposal_index]
    #[instruction]
    pub fn propose(
        #[account(mut)]
        multisig_state: AccountWithMetadata,
        #[account(signer)]
        proposer: AccountWithMetadata,
        #[account(init)]
        proposal: AccountWithMetadata,
        target_program_id: ProgramId,
        target_instruction_data: Vec<u32>,
        target_account_count: u8,
        pda_seeds: Vec<[u8; 32]>,
        authorized_indices: Vec<u8>,
        create_key: [u8; 32],
        proposal_index: u64,
    ) -> SpelResult {
        let accounts = vec![multisig_state, proposer, proposal];
        let (accounts_out, chained_calls) = crate::propose::handle(
            &accounts,
            &target_program_id,
            &target_instruction_data,
            target_account_count,
            &pda_seeds,
            &authorized_indices,
        );

        let claims = vec![
            AutoClaim::None,
            AutoClaim::None,
            AutoClaim::pda_from_seeds(&[b"multisig_prop___" as &[u8], &create_key, &proposal_index.to_be_bytes()]),
        ];

        Ok(SpelOutput::execute(
            accounts_out.into_iter().zip(claims).collect::<Vec<_>>(),
            chained_calls,
        ))
    }

    /// Approve an existing proposal.
    /// approver must be a member signer.
    /// proposal PDA seeds: ["multisig_prop___", create_key, proposal_index]
    #[instruction]
    pub fn approve(
        #[account(mut)]
        multisig_state: AccountWithMetadata,
        #[account(signer)]
        approver: AccountWithMetadata,
        #[account(mut)]
        proposal: AccountWithMetadata,
        proposal_index: u64,
        create_key: [u8; 32],
    ) -> SpelResult {
        let accounts = vec![multisig_state, approver, proposal];
        let (accounts_out, chained_calls) =
            crate::approve::handle(&accounts, proposal_index);

        let claims = vec![AutoClaim::None, AutoClaim::None, AutoClaim::None];

        Ok(SpelOutput::execute(
            accounts_out.into_iter().zip(claims).collect::<Vec<_>>(),
            chained_calls,
        ))
    }

    /// Reject an existing proposal.
    /// rejector must be a member signer.
    /// proposal PDA seeds: ["multisig_prop___", create_key, proposal_index]
    #[instruction]
    pub fn reject(
        #[account(mut)]
        multisig_state: AccountWithMetadata,
        #[account(signer)]
        rejector: AccountWithMetadata,
        #[account(mut, pda = [literal("multisig_prop___"), arg("create_key"), arg("proposal_index")])]
        proposal: AccountWithMetadata,
        proposal_index: u64,
        create_key: [u8; 32],
    ) -> SpelResult {
        let accounts = vec![multisig_state, rejector, proposal];
        let (accounts_out, chained_calls) =
            crate::reject::handle(&accounts, proposal_index);

        let claims = vec![AutoClaim::None, AutoClaim::None, AutoClaim::None];

        Ok(SpelOutput::execute(
            accounts_out.into_iter().zip(claims).collect::<Vec<_>>(),
            chained_calls,
        ))
    }

    /// Execute a fully-approved proposal.
    /// executor must be a member signer. target_accounts are the rest accounts.
    /// proposal PDA seeds: ["multisig_prop___", create_key, proposal_index]
    #[instruction]
    pub fn execute(
        #[account(mut)]
        multisig_state: AccountWithMetadata,
        #[account(signer)]
        executor: AccountWithMetadata,
        #[account(mut, pda = [literal("multisig_prop___"), arg("create_key"), arg("proposal_index")])]
        proposal: AccountWithMetadata,
        target_accounts: Vec<AccountWithMetadata>,
        proposal_index: u64,
        create_key: [u8; 32],
    ) -> SpelResult {
        let target_count = target_accounts.len();
        let mut accounts = vec![multisig_state, executor, proposal];
        accounts.extend(target_accounts);
        let (accounts_out, chained_calls) =
            crate::execute::handle(&accounts, proposal_index);

        // First 3 accounts: mut/signer/mut (no claim); target accounts: no claim
        let claims: Vec<AutoClaim> =
            std::iter::repeat(AutoClaim::None).take(3 + target_count).collect();

        Ok(SpelOutput::execute(
            accounts_out.into_iter().zip(claims).collect::<Vec<_>>(),
            chained_calls,
        ))
    }

    /// Propose adding a new member.
    /// proposer must be a member signer. proposal is initialized.
    /// proposal PDA seeds: ["multisig_prop___", create_key, proposal_index]
    #[instruction]
    pub fn propose_add_member(
        #[account(mut)]
        multisig_state: AccountWithMetadata,
        #[account(signer)]
        proposer: AccountWithMetadata,
        #[account(init, pda = [literal("multisig_prop___"), arg("create_key"), arg("proposal_index")])]
        proposal: AccountWithMetadata,
        new_member: [u8; 32],
        create_key: [u8; 32],
        proposal_index: u64,
    ) -> SpelResult {
        let accounts = vec![multisig_state, proposer, proposal];
        let (accounts_out, chained_calls) = crate::propose_config::handle(
            &accounts,
            ConfigAction::AddMember { new_member },
        );

        let claims = vec![
            AutoClaim::None,
            AutoClaim::None,
            AutoClaim::pda_from_seeds(&[b"multisig_prop___" as &[u8], &create_key, &proposal_index.to_be_bytes()]),
        ];

        Ok(SpelOutput::execute(
            accounts_out.into_iter().zip(claims).collect::<Vec<_>>(),
            chained_calls,
        ))
    }

    /// Propose removing a member.
    /// proposer must be a member signer. proposal is initialized.
    /// proposal PDA seeds: ["multisig_prop___", create_key, proposal_index]
    #[instruction]
    pub fn propose_remove_member(
        #[account(mut)]
        multisig_state: AccountWithMetadata,
        #[account(signer)]
        proposer: AccountWithMetadata,
        #[account(init, pda = [literal("multisig_prop___"), arg("create_key"), arg("proposal_index")])]
        proposal: AccountWithMetadata,
        member: [u8; 32],
        create_key: [u8; 32],
        proposal_index: u64,
    ) -> SpelResult {
        let accounts = vec![multisig_state, proposer, proposal];
        let (accounts_out, chained_calls) = crate::propose_config::handle(
            &accounts,
            ConfigAction::RemoveMember { member },
        );

        let claims = vec![
            AutoClaim::None,
            AutoClaim::None,
            AutoClaim::pda_from_seeds(&[b"multisig_prop___" as &[u8], &create_key, &proposal_index.to_be_bytes()]),
        ];

        Ok(SpelOutput::execute(
            accounts_out.into_iter().zip(claims).collect::<Vec<_>>(),
            chained_calls,
        ))
    }

    /// Propose changing the threshold.
    /// proposer must be a member signer. proposal is initialized.
    /// proposal PDA seeds: ["multisig_prop___", create_key, proposal_index]
    #[instruction]
    pub fn propose_change_threshold(
        #[account(mut)]
        multisig_state: AccountWithMetadata,
        #[account(signer)]
        proposer: AccountWithMetadata,
        #[account(init, pda = [literal("multisig_prop___"), arg("create_key"), arg("proposal_index")])]
        proposal: AccountWithMetadata,
        new_threshold: u8,
        create_key: [u8; 32],
        proposal_index: u64,
    ) -> SpelResult {
        let accounts = vec![multisig_state, proposer, proposal];
        let (accounts_out, chained_calls) = crate::propose_config::handle(
            &accounts,
            ConfigAction::ChangeThreshold { new_threshold },
        );

        let claims = vec![
            AutoClaim::None,
            AutoClaim::None,
            AutoClaim::pda_from_seeds(&[b"multisig_prop___" as &[u8], &create_key, &proposal_index.to_be_bytes()]),
        ];

        Ok(SpelOutput::execute(
            accounts_out.into_iter().zip(claims).collect::<Vec<_>>(),
            chained_calls,
        ))
    }
}

// Legacy process() function for the existing guest binary.
// The #[lez_program] macro generates main() and IDL, but the guest binary
// (methods/guest/src/bin/multisig.rs) uses this for the risc0 entry point.
pub fn process(
    accounts: &[nssa_core::account::AccountWithMetadata],
    instruction: &multisig_core::Instruction,
) -> (Vec<nssa_core::program::AccountPostState>, Vec<nssa_core::program::ChainedCall>) {
    use multisig_core::Instruction;
    use nssa_core::program::{AccountPostState, Claim};
    use spel_framework::prelude::AutoClaim;

    // Helper to apply claims to account results
    fn apply_claims(
        accounts: Vec<nssa_core::account::Account>,
        claims: Vec<AutoClaim>,
        chained_calls: Vec<nssa_core::program::ChainedCall>,
    ) -> (Vec<AccountPostState>, Vec<nssa_core::program::ChainedCall>) {
        let post_states = accounts
            .into_iter()
            .zip(claims)
            .map(|(acc, claim)| claim.to_post_state(acc))
            .collect();
        (post_states, chained_calls)
    }

    match instruction {
        Instruction::CreateMultisig { create_key, threshold, members } => {
            let (accs, calls) = create_multisig::handle(accounts, create_key, *threshold, members);
            let claims: Vec<AutoClaim> =
                std::iter::once(AutoClaim::pda_from_seeds(&[&create_key[..]]))
                    .chain(std::iter::repeat(AutoClaim::Claimed(Claim::Authorized)).take(members.len()))
                    .collect();
            apply_claims(accs, claims, calls)
        }
        Instruction::Propose { target_program_id, target_instruction_data, target_account_count, pda_seeds, authorized_indices, create_key, proposal_index } => {
            let (accs, calls) = propose::handle(accounts, target_program_id, target_instruction_data, *target_account_count, pda_seeds, authorized_indices);
            let claims = vec![
                AutoClaim::None,
                AutoClaim::None,
                AutoClaim::pda_from_seeds(&[b"multisig_prop___" as &[u8], &create_key[..], &proposal_index.to_be_bytes()]),
            ];
            apply_claims(accs, claims, calls)
        }
        Instruction::Approve { proposal_index, .. } => {
            let (accs, calls) = approve::handle(accounts, *proposal_index);
            let claims = vec![AutoClaim::None; accs.len()];
            apply_claims(accs, claims, calls)
        }
        Instruction::Reject { proposal_index, .. } => {
            let (accs, calls) = reject::handle(accounts, *proposal_index);
            let claims = vec![AutoClaim::None; accs.len()];
            apply_claims(accs, claims, calls)
        }
        Instruction::Execute { proposal_index, .. } => {
            let (accs, calls) = execute::handle(accounts, *proposal_index);
            let claims = vec![AutoClaim::None; accs.len()];
            apply_claims(accs, claims, calls)
        }
        Instruction::ProposeAddMember { new_member, create_key, proposal_index } => {
            let (accs, calls) = propose_config::handle(accounts, ConfigAction::AddMember { new_member: *new_member });
            let claims = vec![
                AutoClaim::None,
                AutoClaim::None,
                AutoClaim::pda_from_seeds(&[b"multisig_prop___" as &[u8], &create_key[..], &proposal_index.to_be_bytes()]),
            ];
            apply_claims(accs, claims, calls)
        }
        Instruction::ProposeRemoveMember { member, create_key, proposal_index } => {
            let (accs, calls) = propose_config::handle(accounts, ConfigAction::RemoveMember { member: *member });
            let claims = vec![
                AutoClaim::None,
                AutoClaim::None,
                AutoClaim::pda_from_seeds(&[b"multisig_prop___" as &[u8], &create_key[..], &proposal_index.to_be_bytes()]),
            ];
            apply_claims(accs, claims, calls)
        }
        Instruction::ProposeChangeThreshold { new_threshold, create_key, proposal_index } => {
            let (accs, calls) = propose_config::handle(accounts, ConfigAction::ChangeThreshold { new_threshold: *new_threshold });
            let claims = vec![
                AutoClaim::None,
                AutoClaim::None,
                AutoClaim::pda_from_seeds(&[b"multisig_prop___" as &[u8], &create_key[..], &proposal_index.to_be_bytes()]),
            ];
            apply_claims(accs, claims, calls)
        }
    }
}
