use nssa::program::Program;
use std::fs;

#[test]
fn show_image_id() {
    let bin = std::env::var("MULTISIG_PROGRAM").unwrap();
    let bc = fs::read(&bin).unwrap();
    let prog = Program::new(bc).unwrap();
    let pid = prog.id();
    let hex: String = pid.iter().flat_map(|w| w.to_le_bytes()).map(|b| format!("{:02x}", b)).collect();
    eprintln!("Image ID: {}", hex);
}
