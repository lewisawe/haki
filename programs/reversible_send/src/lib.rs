//! # Reversible Send (native Solana program)
//!
//! Self-custodial conditional payments: a send the recipient must **accept**,
//! that the sender can **cancel** until it is accepted, and that **auto-refunds**
//! to the sender if the claim window expires unclaimed. Funds are held by a
//! per-transfer PDA (the vault) and the reversibility rules are enforced here on
//! chain, not in any frontend.
//!
//! This is the native (`solana-program`) implementation of the Phase 1 program —
//! see `.kiro/specs/reversible-send/` and `NOTES.md`. Same account model, same
//! four instructions, and same FR5 invariants as the original Anchor design.
//!
//! ## Modules
//! - [`state`] — the `Transfer` account + fixed manual serialization.
//! - [`instruction`] — the 1-byte-tag wire format + argument decoding.
//! - [`errors`] — custom errors mapped to `ProgramError::Custom`.
//! - [`processor`] — the instruction handlers and invariant enforcement.

pub mod errors;
pub mod instruction;
pub mod processor;
pub mod state;

use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult, pubkey::Pubkey,
};

// Program ID. The matching keypair lives at
// `target/deploy/reversible_send-keypair.json` (generated with solana-keygen).
// Native programs do not hard-require this constant, but it documents the
// program's on-chain identity and is used by clients for PDA derivation.
solana_program::declare_id!("EpwZhUpnRGfDf7mmST8DYXHyEVAJnT6rEgpiKdaQvNRh");

entrypoint!(process_instruction);

/// Program entrypoint — delegates to [`processor::process_instruction`].
pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    processor::process_instruction(program_id, accounts, instruction_data)
}
