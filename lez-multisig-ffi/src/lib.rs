//! lez-multisig-ffi — C FFI wrapper for the LEZ Multisig program.
//!
//! The bulk of the implementation lives in `multisig.rs`, which is generated
//! by lez-client-gen from `multisig_idl.json`.  This file re-exports the
//! generated extern "C" symbols under the canonical `lez_multisig_*` names
//! and adds read-only query helpers not covered by the IDL.

mod multisig;


use nssa_core::program::ProgramId;
use nssa_core::account::AccountId;
use spel_framework_core::pda::{compute_pda_multi, seed_from_str, ToSeed};

/// PDA for the multisig state account.
/// Mirrors `#[account(init, pda = arg("create_key"))]` in multisig_program/src/lib.rs.
pub fn compute_multisig_state_pda(program_id: &ProgramId, create_key: &[u8; 32]) -> AccountId {
    compute_pda_multi(program_id, &[create_key as &dyn ToSeed])
}

/// PDA for a proposal account.
/// Mirrors `#[account(init, pda = [literal("multisig_prop___"), arg("create_key"), arg("proposal_index")])]`.
pub fn compute_proposal_pda(program_id: &ProgramId, create_key: &[u8; 32], proposal_index: u64) -> AccountId {
    let tag = seed_from_str("multisig_prop___");
    compute_pda_multi(program_id, &[&tag as &dyn ToSeed, create_key, &proposal_index])
}

/// Raw seed bytes for the vault PDA, for inclusion in a `Propose` instruction's `pda_seeds`.
/// Vault seed: [literal("multisig_vault__"), arg("create_key")] — two-seed multi-hash.
pub fn vault_pda_seed_bytes(create_key: &[u8; 32]) -> [u8; 32] {
    use sha2::{Sha256, Digest};
    let tag = seed_from_str("multisig_vault__");
    let mut hasher = Sha256::new();
    hasher.update(tag);
    hasher.update(create_key);
    hasher.finalize().into()
}

/// PDA for the multisig vault account.
pub fn compute_vault_pda(program_id: &ProgramId, create_key: &[u8; 32]) -> AccountId {
    let tag = seed_from_str("multisig_vault__");
    compute_pda_multi(program_id, &[&tag as &dyn ToSeed, create_key])
}

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

fn cstr_to_str<'a>(ptr: *const c_char) -> Result<&'a str, String> {
    if ptr.is_null() { return Err("null pointer".to_string()); }
    unsafe { CStr::from_ptr(ptr) }.to_str().map_err(|e| format!("invalid UTF-8: {}", e))
}

fn to_cstring(s: String) -> *mut c_char {
    CString::new(s).unwrap_or_else(|_|
        CString::new(r#"{"success":false,"error":"null byte"}"#).unwrap()
    ).into_raw()
}

fn error_str(msg: &str) -> *mut c_char {
    to_cstring(format!(r#"{{"success":false,"error":{}}}"#, serde_json::json!(msg)))
}

// ── Generated instruction wrappers ───────────────────────────────────────────

#[no_mangle]
pub extern "C" fn lez_multisig_create(args_json: *const c_char) -> *mut c_char {
    multisig::multisig_program_create_multisig(args_json)
}

#[no_mangle]
pub extern "C" fn lez_multisig_propose(args_json: *const c_char) -> *mut c_char {
    multisig::multisig_program_propose(args_json)
}

#[no_mangle]
pub extern "C" fn lez_multisig_approve(args_json: *const c_char) -> *mut c_char {
    multisig::multisig_program_approve(args_json)
}

#[no_mangle]
pub extern "C" fn lez_multisig_reject(args_json: *const c_char) -> *mut c_char {
    multisig::multisig_program_reject(args_json)
}

#[no_mangle]
pub extern "C" fn lez_multisig_execute(args_json: *const c_char) -> *mut c_char {
    multisig::multisig_program_execute(args_json)
}

#[no_mangle]
pub extern "C" fn lez_multisig_free_string(s: *mut c_char) {
    multisig::multisig_program_free_string(s)
}

#[no_mangle]
pub extern "C" fn lez_multisig_version() -> *mut c_char {
    multisig::multisig_program_version()
}

#[no_mangle]
pub extern "C" fn lez_multisig_get_idl() -> *mut c_char {
    const IDL_JSON: &str = include_str!("multisig_idl.json");
    to_cstring(IDL_JSON.to_string())
}

// ── Read-only helpers (not in IDL) ───────────────────────────────────────────

#[no_mangle]
pub extern "C" fn lez_multisig_list_proposals(args_json: *const c_char) -> *mut c_char {
    let args = match cstr_to_str(args_json) { Ok(s) => s, Err(e) => return error_str(&e) };
    to_cstring(multisig_queries::list_proposals(args))
}

#[no_mangle]
pub extern "C" fn lez_multisig_get_state(args_json: *const c_char) -> *mut c_char {
    let args = match cstr_to_str(args_json) { Ok(s) => s, Err(e) => return error_str(&e) };
    to_cstring(multisig_queries::get_state(args))
}

mod multisig_queries {
    use wallet::WalletCore;
    use serde_json::{Value, json};
    use multisig_core::{MultisigState, Proposal};
    use crate::multisig::{compute_proposal_pda, compute_multisig_state_pda};
    use nssa_core::account::AccountId;

    fn load_wallet(v: &Value) -> Result<WalletCore, String> {
        if let Some(p) = v["wallet_path"].as_str() {
            std::env::set_var("NSSA_WALLET_HOME_DIR", p);
        }
        WalletCore::from_env().map_err(|e| format!("wallet: {}", e))
    }

    fn parse_program_id_hex(s: &str) -> Result<nssa_core::program::ProgramId, String> {
        let s = s.trim_start_matches("0x");
        if s.len() != 64 { return Err(format!("program_id must be 64 hex chars")); }
        let bytes = hex::decode(s).map_err(|e| format!("hex: {}", e))?;
        let mut pid = [0u32; 8];
        for (i, chunk) in bytes.chunks(4).enumerate() {
            pid[i] = u32::from_le_bytes(chunk.try_into().unwrap());
        }
        Ok(pid)
    }

    async fn fetch_borsh<T: borsh::BorshDeserialize>(
        wallet: &WalletCore,
        account_id: AccountId,
    ) -> Result<Option<T>, String> {
        match wallet.get_account_public(account_id).await {
            Ok(acc) => {
                let data: Vec<u8> = acc.data.into();
                if data.is_empty() { return Ok(None); }
                borsh::from_slice::<T>(&data).map(Some).map_err(|e| format!("deserialize: {}", e))
            }
            Err(e) => Err(format!("get_account: {}", e)),
        }
    }

    fn parse_account(s: &str) -> Result<AccountId, String> {
        s.parse().map_err(|e| format!("invalid account: {:?}", e))
    }

    pub fn list_proposals(args: &str) -> String {
        let v: Value = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return json!({"success": false, "error": format!("{}", e)}).to_string(),
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let wallet = load_wallet(&v)?;
            let program_id = parse_program_id_hex(v["program_id_hex"].as_str().ok_or("missing program_id_hex")?)?;
            let ms_id = parse_account(v["multisig_state"].as_str().ok_or("missing multisig_state")?)?;
            let state: MultisigState = match fetch_borsh(&wallet, ms_id).await? {
                Some(s) => s,
                None => return Err("multisig_state not found".to_string()),
            };
            let mut proposals = Vec::new();
            for i in 0..state.transaction_index {
                let prop_id = compute_proposal_pda(&program_id, &state.create_key, i);
                if let Some(prop) = fetch_borsh::<Proposal>(&wallet, prop_id).await? {
                    let proposer_b58 = bs58::encode(prop.proposer).into_string();
                    proposals.push(json!({
                        "index": prop.index,
                        "status": format!("{:?}", prop.status),
                        "proposer": proposer_b58,
                        "approvals": prop.approved.len(),
                        "rejections": prop.rejected.len(),
                        "threshold": state.threshold,
                    }));
                }
            }
            Ok::<String, String>(json!({"success": true, "proposals": proposals}).to_string())
        }).unwrap_or_else(|e| json!({"success": false, "error": e}).to_string())
    }

    pub fn get_state(args: &str) -> String {
        let v: Value = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return json!({"success": false, "error": format!("{}", e)}).to_string(),
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let wallet = load_wallet(&v)?;
            let program_id = parse_program_id_hex(v["program_id_hex"].as_str().ok_or("missing program_id_hex")?)?;
            let create_key_hex = v["create_key"].as_str().ok_or("missing create_key")?;
            let create_key_bytes = hex::decode(create_key_hex.trim_start_matches("0x"))
                .map_err(|e| format!("create_key hex: {}", e))?;
            let mut create_key = [0u8; 32];
            create_key.copy_from_slice(&create_key_bytes);
            let ms_id = compute_multisig_state_pda(&program_id, &create_key);
            match fetch_borsh::<MultisigState>(&wallet, ms_id).await? {
                Some(state) => {
                    let members: Vec<String> = state.members.iter()
                        .map(|m| bs58::encode(m).into_string())
                        .collect();
                    Ok(json!({
                        "success": true,
                        "threshold": state.threshold,
                        "member_count": state.member_count,
                        "members": members,
                        "transaction_index": state.transaction_index,
                        "multisig_state_id": ms_id.to_string(),
                    }).to_string())
                }
                None => Err("multisig_state not found".to_string()),
            }
        }).unwrap_or_else(|e| json!({"success": false, "error": e}).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nssa_core::program::ProgramId;

    #[test]
    fn test_pda_computation_matches_core() {
        let program_id: ProgramId = [1, 2, 3, 4, 5, 6, 7, 8];
        let create_key: [u8; 32] = [9; 32];
        
        // Verify multisig state PDA matches raw SPEL compute_pda_multi
        let state_pda = compute_multisig_state_pda(&program_id, &create_key);
        let expected = spel_framework_core::pda::compute_pda_multi(
            &program_id,
            &[&create_key as &dyn spel_framework_core::pda::ToSeed],
        );
        assert_eq!(state_pda, expected, "multisig state PDA should match SPEL compute_pda_multi");
    }

    #[test]
    fn test_vault_pda_consistent() {
        let program_id: ProgramId = [1, 2, 3, 4, 5, 6, 7, 8];
        let create_key: [u8; 32] = [9; 32];

        // compute_vault_pda and vault_pda_seed_bytes must agree
        let vault_via_fn = compute_vault_pda(&program_id, &create_key);
        let seed_bytes = vault_pda_seed_bytes(&create_key);
        let vault_via_seed = nssa_core::account::AccountId::from(
            (&program_id, &nssa_core::program::PdaSeed::new(seed_bytes))
        );
        assert_eq!(vault_via_fn, vault_via_seed, "compute_vault_pda and vault_pda_seed_bytes must agree");
    }
}
