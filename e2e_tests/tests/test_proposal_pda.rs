use nssa_core::program::ProgramId;

#[test]
fn test_proposal_pda_matches_spel() {
    use spel_framework::pda::{compute_pda_multi, seed_from_str, ToSeed};

    let program_id: ProgramId = [1, 2, 3, 4, 5, 6, 7, 8];
    let create_key: [u8; 32] = [9; 32];
    let proposal_index: u64 = 1;

    let ffi_pda = lez_multisig_ffi::compute_proposal_pda(&program_id, &create_key, proposal_index);

    // Multi-seed: SPEL macro `pda = [literal("multisig_prop___"), arg("create_key"), arg("proposal_index")]`
    let tag = seed_from_str("multisig_prop___");
    let expected = compute_pda_multi(
        &program_id,
        &[&tag as &dyn ToSeed, &create_key, &proposal_index],
    );

    assert_eq!(ffi_pda, expected, "proposal PDA should match SPEL compute_pda_multi");
}
