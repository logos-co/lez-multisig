#![no_main]

use nssa_core::program::{ProgramInput, ProgramOutput, read_nssa_inputs};
use multisig_core::Instruction;

risc0_zkvm::guest::entry!(main);

fn main() {
    let (ProgramInput { pre_states, instruction }, instruction_words) =
        read_nssa_inputs::<Instruction>();

    let pre_states_clone = pre_states.clone();

    let (post_states, chained_calls) = multisig_program::process(&pre_states, &instruction);

    ProgramOutput::new(
        instruction_words,
        pre_states_clone,
        post_states,
    )
    .with_chained_calls(chained_calls)
    .write();
}
