use nssa::program::Program;
use nssa_core::account::AccountId;
use nssa_core::program::PdaSeed;

fn pid_hex(pid: &nssa_core::program::ProgramId) -> String {
    pid.iter().flat_map(|w| w.to_le_bytes()).map(|b| format!("{:02x}", b)).collect()
}

#[test]
fn debug_all_paths() {
    let bin = std::env::var("MULTISIG_PROGRAM").unwrap();
    let bc = std::fs::read(&bin).unwrap();
    let prog = Program::new(bc).unwrap();
    let pid = prog.id();
    
    // Use a FIXED create_key so we can compare
    let ck: [u8; 32] = [1u8; 32];
    
    eprintln!("Binary: {}", bin);
    eprintln!("Image ID: {}", pid_hex(&pid));
    eprintln!("create_key: {}", hex::encode(ck));
    
    // Path 1: AccountId::from (used by test via compute_multisig_state_pda)
    let pda1 = AccountId::from((&pid, &PdaSeed::new(ck)));
    eprintln!("Path 1 (AccountId::from): {}", pda1);
    
    // Path 2: compute_pda (used by guest in SPEL macro)
    let pda2 = spel_framework::pda::compute_pda(&pid, &[&ck]);
    eprintln!("Path 2 (compute_pda):     {}", pda2);
    
    // Path 3: multisig_core
    let pda3 = multisig_core::compute_multisig_state_pda(&pid, &ck);
    eprintln!("Path 3 (multisig_core):   {}", pda3);
    
    assert_eq!(pda1, pda2, "Path 1 != Path 2!");
    assert_eq!(pda1, pda3, "Path 1 != Path 3!");
    eprintln!("ALL AGREE: {}", pda1);
}
