#![no_main]

use multisig_program::multisig_program;

risc0_zkvm::guest::entry!(multisig_program::main);
