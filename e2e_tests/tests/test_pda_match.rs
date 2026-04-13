use nssa_core::program::ProgramId;

#[test]
fn test_multisig_state_pda_matches_spel() {
    use spel_framework::pda::{compute_pda_multi, ToSeed};

    let program_id: ProgramId = [1, 2, 3, 4, 5, 6, 7, 8];
    let create_key: [u8; 32] = [9; 32];

    let ffi_pda = lez_multisig_ffi::compute_multisig_state_pda(&program_id, &create_key);

    // Single-seed: SPEL macro `pda = arg("create_key")` passes create_key directly
    let expected = compute_pda_multi(&program_id, &[&create_key as &dyn ToSeed]);

    assert_eq!(ffi_pda, expected, "multisig state PDA should match SPEL compute_pda_multi");
}
