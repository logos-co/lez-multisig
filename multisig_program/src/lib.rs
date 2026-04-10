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
        #[account(init)]
        member_accounts: Vec<AccountWithMetadata>,
        create_key: [u8; 32],
        threshold: u8,
        members: Vec<[u8; 32]>,
    ) -> SpelResult {
        let all: Vec<AccountWithMetadata> = std::iter::once(multisig_state)
            .chain(member_accounts.into_iter())
            .collect();
        let (modified, chained_calls) =
            crate::create_multisig::handle(&all, &create_key, threshold, &members);
        // Auto-claim: multisig_state is init/pda, member accounts are init
        let mut pairs: Vec<(nssa_core::account::Account, AutoClaim)> = Vec::new();
        pairs.push((modified[0].clone(), AutoClaim::pda_from_seeds(&[&create_key])));
        for i in 1..modified.len() {
            pairs.push((modified[i].clone(), AutoClaim::Claimed(nssa_core::program::Claim::Authorized)));
        }
        Ok(SpelOutput::execute(pairs, chained_calls))
    }

    /// Propose a new transaction.
    /// proposer must be a member signer. proposal is initialized as a new PDA.
    #[instruction]
    pub fn propose(
        #[account(mut)]
        multisig_state: AccountWithMetadata,
        #[account(signer)]
        proposer: AccountWithMetadata,
        #[account(init, pda = arg("create_key"))]
        proposal: AccountWithMetadata,
        target_program_id: ProgramId,
        target_instruction_data: Vec<u32>,
        target_account_count: u8,
        pda_seeds: Vec<[u8; 32]>,
        authorized_indices: Vec<u8>,
        create_key: [u8; 32],
        proposal_index: u64,
    ) -> SpelResult {
        let input = [multisig_state, proposer, proposal];
        let (modified, chained_calls) = crate::propose::handle(
            &input,
            &target_program_id,
            &target_instruction_data,
            target_account_count,
            &pda_seeds,
            &authorized_indices,
        );
        Ok(SpelOutput::execute(
            vec![
                (modified[0].clone(), AutoClaim::None),
                (modified[1].clone(), AutoClaim::None),
                (modified[2].clone(), AutoClaim::pda_from_seeds(&[&create_key])),
            ],
            chained_calls,
        ))
    }

    /// Approve an existing proposal.
    /// approver must be a member signer.
    #[instruction]
    pub fn approve(
        #[account(mut)]
        multisig_state: AccountWithMetadata,
        #[account(signer)]
        approver: AccountWithMetadata,
        #[account(mut, pda = arg("create_key"))]
        proposal: AccountWithMetadata,
        create_key: [u8; 32],
        proposal_index: u64,
    ) -> SpelResult {
        let input = [multisig_state, approver, proposal];
        let (modified, chained_calls) =
            crate::approve::handle(&input, proposal_index);
        Ok(SpelOutput::execute(
            vec![
                (modified[0].clone(), AutoClaim::None),
                (modified[1].clone(), AutoClaim::None),
                (modified[2].clone(), AutoClaim::None),
            ],
            chained_calls,
        ))
    }

    /// Reject an existing proposal.
    /// rejector must be a member signer.
    #[instruction]
    pub fn reject(
        #[account(mut)]
        multisig_state: AccountWithMetadata,
        #[account(signer)]
        rejector: AccountWithMetadata,
        #[account(mut, pda = arg("create_key"))]
        proposal: AccountWithMetadata,
        create_key: [u8; 32],
        proposal_index: u64,
    ) -> SpelResult {
        let input = [multisig_state, rejector, proposal];
        let (modified, chained_calls) =
            crate::reject::handle(&input, proposal_index);
        Ok(SpelOutput::execute(
            vec![
                (modified[0].clone(), AutoClaim::None),
                (modified[1].clone(), AutoClaim::None),
                (modified[2].clone(), AutoClaim::None),
            ],
            chained_calls,
        ))
    }

    /// Execute a fully-approved proposal.
    /// executor must be a member signer. target_accounts are the rest accounts.
    #[instruction]
    pub fn execute(
        #[account(mut)]
        multisig_state: AccountWithMetadata,
        #[account(signer)]
        executor: AccountWithMetadata,
        #[account(mut, pda = arg("create_key"))]
        proposal: AccountWithMetadata,
        #[account(mut)]
        target_accounts: Vec<AccountWithMetadata>,
        create_key: [u8; 32],
        proposal_index: u64,
    ) -> SpelResult {
        let mut all: Vec<AccountWithMetadata> = vec![multisig_state, executor, proposal];
        all.extend(target_accounts);
        let (modified, chained_calls) =
            crate::execute::handle(&all, proposal_index);
        let pairs: Vec<(nssa_core::account::Account, AutoClaim)> = modified
            .into_iter()
            .map(|acc| (acc, AutoClaim::None))
            .collect();
        Ok(SpelOutput::execute(pairs, chained_calls))
    }

    /// Propose adding a new member.
    /// proposer must be a member signer. proposal is initialized.
    #[instruction]
    pub fn propose_add_member(
        #[account(mut)]
        multisig_state: AccountWithMetadata,
        #[account(signer)]
        proposer: AccountWithMetadata,
        #[account(init, pda = arg("create_key"))]
        proposal: AccountWithMetadata,
        create_key: [u8; 32],
        new_member: [u8; 32],
        proposal_index: u64,
    ) -> SpelResult {
        let input = [multisig_state, proposer, proposal];
        let (modified, chained_calls) = crate::propose_config::handle(
            &input,
            ConfigAction::AddMember { new_member },
        );
        Ok(SpelOutput::execute(
            vec![
                (modified[0].clone(), AutoClaim::None),
                (modified[1].clone(), AutoClaim::None),
                (modified[2].clone(), AutoClaim::pda_from_seeds(&[&create_key])),
            ],
            chained_calls,
        ))
    }

    /// Propose removing a member.
    /// proposer must be a member signer. proposal is initialized.
    #[instruction]
    pub fn propose_remove_member(
        #[account(mut)]
        multisig_state: AccountWithMetadata,
        #[account(signer)]
        proposer: AccountWithMetadata,
        #[account(init, pda = arg("create_key"))]
        proposal: AccountWithMetadata,
        create_key: [u8; 32],
        member: [u8; 32],
        proposal_index: u64,
    ) -> SpelResult {
        let input = [multisig_state, proposer, proposal];
        let (modified, chained_calls) = crate::propose_config::handle(
            &input,
            ConfigAction::RemoveMember { member },
        );
        Ok(SpelOutput::execute(
            vec![
                (modified[0].clone(), AutoClaim::None),
                (modified[1].clone(), AutoClaim::None),
                (modified[2].clone(), AutoClaim::pda_from_seeds(&[&create_key])),
            ],
            chained_calls,
        ))
    }

    /// Propose changing the threshold.
    /// proposer must be a member signer. proposal is initialized.
    #[instruction]
    pub fn propose_change_threshold(
        #[account(mut)]
        multisig_state: AccountWithMetadata,
        #[account(signer)]
        proposer: AccountWithMetadata,
        #[account(init, pda = arg("create_key"))]
        proposal: AccountWithMetadata,
        create_key: [u8; 32],
        new_threshold: u8,
        proposal_index: u64,
    ) -> SpelResult {
        let input = [multisig_state, proposer, proposal];
        let (modified, chained_calls) = crate::propose_config::handle(
            &input,
            ConfigAction::ChangeThreshold { new_threshold },
        );
        Ok(SpelOutput::execute(
            vec![
                (modified[0].clone(), AutoClaim::None),
                (modified[1].clone(), AutoClaim::None),
                (modified[2].clone(), AutoClaim::pda_from_seeds(&[&create_key])),
            ],
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
) -> (Vec<nssa_core::account::Account>, Vec<nssa_core::program::ChainedCall>) {
    use multisig_core::Instruction;
    match instruction {
        Instruction::CreateMultisig { create_key, threshold, members } =>
            create_multisig::handle(accounts, create_key, *threshold, members),
        Instruction::Propose { target_program_id, target_instruction_data, target_account_count, pda_seeds, authorized_indices, .. } =>
            propose::handle(accounts, target_program_id, target_instruction_data, *target_account_count, pda_seeds, authorized_indices),
        Instruction::Approve { proposal_index, .. } => approve::handle(accounts, *proposal_index),
        Instruction::Reject { proposal_index, .. } => reject::handle(accounts, *proposal_index),
        Instruction::Execute { proposal_index, .. } => execute::handle(accounts, *proposal_index),
        Instruction::ProposeAddMember { new_member, .. } =>
            propose_config::handle(accounts, ConfigAction::AddMember { new_member: *new_member }),
        Instruction::ProposeRemoveMember { member, .. } =>
            propose_config::handle(accounts, ConfigAction::RemoveMember { member: *member }),
        Instruction::ProposeChangeThreshold { new_threshold, .. } =>
            propose_config::handle(accounts, ConfigAction::ChangeThreshold { new_threshold: *new_threshold }),
    }
}
