//! Custom program errors for `reversible_send`.
//!
//! Each variant maps to a `ProgramError::Custom(u32)` so clients can decode the
//! specific invariant that was violated. The numeric codes are stable and must
//! not be reordered.

use solana_program::program_error::ProgramError;

/// Errors returned by the reversible_send program.
///
/// Mapped to `ProgramError::Custom(n)` where `n` is the `u32` discriminant below.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReversibleSendError {
    /// Instruction data was empty or the tag byte was unrecognized. (0)
    InvalidInstruction = 0,
    /// The signer attempting to cancel is not the recorded sender. (1)
    NotSender = 1,
    /// The signer attempting to accept is not the recorded recipient. (2)
    NotRecipient = 2,
    /// The transfer is not in the `Pending` state (already terminal / closed). (3)
    NotPending = 3,
    /// Accept attempted after the claim window closed. (4)
    WindowExpired = 4,
    /// Refund attempted before the claim window closed. (5)
    WindowNotExpired = 5,
    /// `amount` did not exceed the rent-exempt minimum of the PDA. (6)
    AmountTooSmall = 6,
    /// `claim_window` was outside the allowed [10, 3600] second range. (7)
    InvalidClaimWindow = 7,
    /// A provided account did not match the expected PDA / address. (8)
    AccountMismatch = 8,
    /// The derived PDA did not match the account passed in. (9)
    InvalidPda = 9,
    /// A required signer did not sign. (10)
    MissingRequiredSignature = 10,
    /// The transfer account is already initialized. (11)
    AlreadyInitialized = 11,
    /// Arithmetic overflow/underflow while moving lamports. (12)
    ArithmeticOverflow = 12,
    /// An account was not owned by this program when it must be. (13)
    InvalidAccountOwner = 13,
    /// The account data was the wrong length to be a Transfer. (14)
    InvalidAccountData = 14,
}

impl From<ReversibleSendError> for ProgramError {
    fn from(e: ReversibleSendError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
