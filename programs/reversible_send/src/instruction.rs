//! Instruction set + wire format for `reversible_send`.
//!
//! Wire format: `instruction_data[0]` is a 1-byte tag; any following bytes are the
//! little-endian-packed arguments for that instruction. Only `CreateTransfer`
//! carries arguments.
//!
//! Tag byte mapping:
//! | tag | instruction      |
//! |-----|------------------|
//! | 0   | CreateTransfer   |
//! | 1   | CancelTransfer   |
//! | 2   | AcceptTransfer   |
//! | 3   | RefundExpired    |

use solana_program::program_error::ProgramError;

use crate::errors::ReversibleSendError;

/// Parsed instruction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReversibleSendInstruction {
    /// Create + fund a new reversible transfer.
    ///
    /// Accounts:
    /// 0. `[signer, writable]` sender (funds the PDA, pays rent)
    /// 1. `[writable]`         transfer PDA (seeds: ["transfer", sender, recipient, seed_le])
    /// 2. `[]`                 recipient (not a signer; recorded on the account)
    /// 3. `[]`                 system program
    ///
    /// Args (after the tag byte): `amount: u64 | claim_window: i64 | seed: u64`
    /// packed little-endian (24 bytes).
    CreateTransfer {
        amount: u64,
        claim_window: i64,
        seed: u64,
    },

    /// Cancel a Pending transfer; refund the sender and close the account.
    ///
    /// Accounts:
    /// 0. `[signer, writable]` sender (must equal transfer.sender; receives refund + rent)
    /// 1. `[writable]`         transfer PDA
    CancelTransfer,

    /// Accept a Pending transfer before expiry; pay the recipient and close.
    ///
    /// The PDA holds exactly `amount` lamports (it was created with `amount`), so
    /// the full locked amount — including what was the rent reserve — is swept to
    /// the recipient. The recipient receives the entire balance; nothing is
    /// returned to the sender on accept (the sender funded it as the payment).
    ///
    /// Accounts:
    /// 0. `[signer, writable]` recipient (must equal transfer.recipient; receives funds)
    /// 1. `[writable]`         transfer PDA
    AcceptTransfer,

    /// Permissionless refund of an expired Pending transfer; pay the sender, close.
    ///
    /// Accounts:
    /// 0. `[signer]`           cranker (anyone; pays tx fee only)
    /// 1. `[writable]`         transfer PDA
    /// 2. `[writable]`         sender (must equal transfer.sender; receives funds + rent)
    RefundExpired,
}

impl ReversibleSendInstruction {
    /// Parse the 1-byte tag + packed args from raw instruction data.
    pub fn unpack(input: &[u8]) -> Result<Self, ProgramError> {
        let (&tag, rest) = input
            .split_first()
            .ok_or(ReversibleSendError::InvalidInstruction)?;

        match tag {
            0 => {
                // amount(8) + claim_window(8) + seed(8) = 24 bytes
                if rest.len() != 24 {
                    return Err(ReversibleSendError::InvalidInstruction.into());
                }
                let amount = u64::from_le_bytes(rest[0..8].try_into().unwrap());
                let claim_window = i64::from_le_bytes(rest[8..16].try_into().unwrap());
                let seed = u64::from_le_bytes(rest[16..24].try_into().unwrap());
                Ok(ReversibleSendInstruction::CreateTransfer {
                    amount,
                    claim_window,
                    seed,
                })
            }
            1 => Ok(ReversibleSendInstruction::CancelTransfer),
            2 => Ok(ReversibleSendInstruction::AcceptTransfer),
            3 => Ok(ReversibleSendInstruction::RefundExpired),
            _ => Err(ReversibleSendError::InvalidInstruction.into()),
        }
    }
}
