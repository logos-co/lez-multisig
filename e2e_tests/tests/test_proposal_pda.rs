use nssa_core::program::ProgramId;

#[test]
fn test_proposal_pda_ffi_matches_core() {
    let program_id: ProgramId = [1, 2, 3, 4, 5, 6, 7, 8];
    let create_key: [u8; 32] = [9; 32];
    let proposal_index: u64 = 1;
    
    // Compute using FFI
    let ffi_pda = lez_multisig_ffi::compute_proposal_pda(&program_id, &create_key, proposal_index);
    
    // Compute using multisig_core
    let core_pda = multisig_core::compute_proposal_pda(&program_id, &create_key, proposal_index);
    
    assert_eq!(ffi_pda, core_pda, "FFI proposal PDA should match multisig_core PDA");
}
