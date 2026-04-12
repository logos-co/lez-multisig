use nssa_core::account::AccountId;
use nssa_core::program::{PdaSeed, ProgramId};

fn pid_from_hex(hex_str: &str) -> ProgramId {
    let bytes = hex::decode(hex_str).unwrap();
    let mut pid = [0u32; 8];
    for (i, chunk) in bytes.chunks(4).enumerate() {
        pid[i] = u32::from_le_bytes(chunk.try_into().unwrap());
    }
    pid
}

#[test]
fn debug_final() {
    let pid = pid_from_hex("2c0a201450bd058a3cc3afd7a2ef9d2a9dad9cb63ae151b08fce52ebf5d5ecf5");
    let seed: [u8; 32] = hex::decode("5c03a3c7e2a1f7cde90f893ebde7b3c76b6a164fc2a4bfaa79be6f5c186eb5f5").unwrap().try_into().unwrap();
    
    let pda_seed = PdaSeed::new(seed);
    let pda = AccountId::from((&pid, &pda_seed));
    eprintln!("Host PDA (AccountId::from): {}", pda);
    
    let pda2 = spel_framework::pda::compute_pda(&pid, &[&seed]);
    eprintln!("Host PDA (compute_pda):     {}", pda2);
    
    eprintln!("Guest PDA (from seq log): 3AmAHhNKGs1ukSj1AjWBrvAyu4uVbWnoawR8cLu5mzaT");
    eprintln!("Test PDA (from e2e):      XALoeChWNwFYP8uDRCFygnntEYhHooQbEfW9RESi5vF");
    
    // Are host PDAs the same?
    assert_eq!(pda, pda2);
}
