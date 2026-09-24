/**
 * lez_multisig.h — C FFI interface for the LEZ Multisig program
 *
 * Enables Logos Core Qt plugins to interact with the LEZ multisig
 * program without depending on Rust directly.
 *
 * All functions take/return JSON strings (UTF-8, null-terminated).
 * Caller must free returned strings with lez_multisig_free_string().
 *
 * JSON error response format:
 *   { "success": false, "error": "<message>" }
 *
 * JSON success response format varies by function (documented inline).
 */

#ifndef LEZ_MULTISIG_H
#define LEZ_MULTISIG_H

#ifdef __cplusplus
extern "C" {
#endif

#include <stdint.h>

/* ── Multisig Operations ─────────────────────────────────────────────────── */

/**
 * Create a new M-of-N multisig.
 *
 * args_json: {
 *   "sequencer_url":       "http://...",
 *   "wallet_path":         "...",
 *   "multisig_program_id": "hex64",
 *   "account":             "<signer AccountId>",
 *   "create_key":          "hex64  (unique key for PDA derivation)",
 *   "threshold":           2,
 *   "members":             ["hex64", "hex64", ...]
 * }
 *
 * Returns: {
 *   "success": true,
 *   "tx_hash": "0x...",
 *   "multisig_state_pda": "...",
 *   "create_key": "hex64"
 * }
 */
char* lez_multisig_create(const char* args_json);

/**
 * Create a new proposal in a multisig.
 *
 * args_json: {
 *   "sequencer_url":           "http://...",
 *   "wallet_path":             "...",
 *   "multisig_program_id":     "hex64",
 *   "account":                 "<proposer AccountId>",
 *   "create_key":              "hex64",
 *   "target_program_id":       "hex64",
 *   "target_instruction_data": "hex (encoded bytes)",
 *   "target_account_count":    3,
 *   "target_account_ids":      ["hex64", ...],  (the approved transfer targets;
 *                                                length MUST equal target_account_count.
 *                                                Bound at propose time so the executor
 *                                                cannot substitute the destination — issue #40)
 *   "pda_seeds":               ["hex64", ...],
 *   "authorized_indices":      [0, 1]
 * }
 *
 * Returns: {
 *   "success": true,
 *   "tx_hash": "0x...",
 *   "proposal_index": 1,
 *   "proposal_pda": "..."
 * }
 */
char* lez_multisig_propose(const char* args_json);

/**
 * Approve an existing proposal.
 *
 * args_json: {
 *   "sequencer_url":       "http://...",
 *   "wallet_path":         "...",
 *   "multisig_program_id": "hex64",
 *   "account":             "<approver AccountId>",
 *   "create_key":          "hex64",
 *   "proposal_index":      1
 * }
 *
 * Returns: { "success": true, "tx_hash": "0x...", "proposal_index": 1, "action": "approved" }
 */
char* lez_multisig_approve(const char* args_json);

/**
 * Reject an existing proposal.
 *
 * args_json: (same as approve)
 *
 * Returns: { "success": true, "tx_hash": "0x...", "proposal_index": 1, "action": "rejected" }
 */
char* lez_multisig_reject(const char* args_json);

/**
 * Execute a fully-approved proposal.
 *
 * args_json: {
 *   "sequencer_url":       "http://...",
 *   "wallet_path":         "...",
 *   "multisig_program_id": "hex64",
 *   "account":             "<executor AccountId>",
 *   "create_key":          "hex64",
 *   "proposal_index":      1
 * }
 *
 * Returns: { "success": true, "tx_hash": "0x...", "proposal_index": 1 }
 */
char* lez_multisig_execute(const char* args_json);

/**
 * List proposals for a multisig.
 *
 * args_json: {
 *   "sequencer_url":       "http://...",
 *   "wallet_path":         "...",
 *   "multisig_program_id": "hex64",
 *   "create_key":          "hex64"
 * }
 *
 * Returns: {
 *   "success": true,
 *   "proposals": [
 *     {
 *       "index": 1,
 *       "proposer": "hex64",
 *       "target_program_id": "hex64",
 *       "target_account_count": 3,
 *       "target_account_ids": ["hex64", ...],  (approved destinations — issue #40)
 *       "approved_count": 2,
 *       "rejected_count": 0,
 *       "status": "Active|Approved|Rejected|Executed",
 *       "proposal_pda": "..."
 *     },
 *     ...
 *   ],
 *   "transaction_index": 3
 * }
 */
char* lez_multisig_list_proposals(const char* args_json);

/**
 * Get the state of a multisig.
 *
 * args_json: {
 *   "sequencer_url":       "http://...",
 *   "wallet_path":         "...",
 *   "multisig_program_id": "hex64",
 *   "create_key":          "hex64"
 * }
 *
 * Returns: {
 *   "success": true,
 *   "state": {
 *     "create_key": "hex64",
 *     "threshold": 2,
 *     "member_count": 3,
 *     "members": ["hex64", ...],
 *     "transaction_index": 5
 *   },
 *   "multisig_state_pda": "..."
 * }
 */
char* lez_multisig_get_state(const char* args_json);

/* ── Memory Management ───────────────────────────────────────────────────── */

/**
 * Free a string returned by any lez_multisig_* function.
 * Must be called for every non-NULL return value to avoid memory leaks.
 */
void lez_multisig_free_string(char* s);


/* ── IDL ─────────────────────────────────────────────────────────────────── */

/**
 * Returns the program IDL as a JSON string (embedded at compile time).
 * Caller must free with lez_multisig_free_string().
 */
const char* lez_multisig_get_idl(void);

/* ── Version Info ────────────────────────────────────────────────────────── */

/**
 * Returns the version string of this FFI library.
 * Caller must free with lez_multisig_free_string().
 */
char* lez_multisig_version(void);

#ifdef __cplusplus
}
#endif

#endif /* LEZ_MULTISIG_H */
