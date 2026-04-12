use nssa::program::Program;
use std::fs;

#[test]
fn check_imgid() {
    let bin = std::env::var("MULTISIG_PROGRAM").unwrap();
    let bc = fs::read(&bin).unwrap();
    let prog = Program::new(bc.clone()).unwrap();
    let pid = prog.id();
    eprintln!("Binary: {}", bin);
    eprintln!("Size: {} bytes", bc.len());
    let hex: String = pid.iter().flat_map(|w| w.to_le_bytes()).map(|b| format!("{:02x}", b)).collect();
    eprintln!("Image ID: {}", hex);
}
