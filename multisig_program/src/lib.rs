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

        Ok(SpelOutput::execute(accounts_out, chained_calls))
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
        #[account(init, pda = [literal("multisig_prop___"), arg("create_key"), arg("proposal_index")])]
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
        Ok(SpelOutput::execute(accounts_out, chained_calls))
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
        #[account(mut, pda = [literal("multisig_prop___"), arg("create_key"), arg("proposal_index")])]
        proposal: AccountWithMetadata,
        proposal_index: u64,
        create_key: [u8; 32],
    ) -> SpelResult {
        let accounts = vec![multisig_state, approver, proposal];
        let (accounts_out, chained_calls) =
            crate::approve::handle(&accounts, proposal_index);
        Ok(SpelOutput::execute(accounts_out, chained_calls))
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
        Ok(SpelOutput::execute(accounts_out, chained_calls))
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
        let mut accounts = vec![multisig_state, executor, proposal];
        accounts.extend(target_accounts);
        let (accounts_out, chained_calls) =
            crate::execute::handle(&accounts, proposal_index);

        Ok(SpelOutput::execute(accounts_out, chained_calls))
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
        Ok(SpelOutput::execute(accounts_out, chained_calls))
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
        Ok(SpelOutput::execute(accounts_out, chained_calls))
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
        Ok(SpelOutput::execute(accounts_out, chained_calls))
    }
}
