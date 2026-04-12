use nssa_core::program::{ProgramId, PdaSeed};
use nssa_core::account::AccountId;

#[test]
fn test_pda_ffi_matches_sPEL() {
    // Test that our FFI PDA computation matches what SPEL would compute
    let program_id: ProgramId = [1, 2, 3, 4, 5, 6, 7, 8];
    let create_key: [u8; 32] = [9; 32];
    
    // Compute using multisig_core (should match SPEL)
    let core_pda = multisig_core::compute_multisig_state_pda(&program_id, &create_key);
    
    // Manual computation matching SPEL's compute_pda for single seed
    let pda_seed = PdaSeed::new(create_key);
    let expected_pda = AccountId::from((&program_id, &pda_seed));
    
    assert_eq!(core_pda, expected_pda, "multisig_core PDA should match manual SPEL computation");
}
