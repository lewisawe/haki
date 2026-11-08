//! On-chain state for `reversible_send`.
//!
//! One `Transfer` account exists per send. It is a PDA that *is* the vault — the
//! locked lamports are held by this account directly. On any terminal action the
//! lamports are drained to the correct party and the account is closed (its data
//! zeroed, lamports swept, so it can never be re-used / replayed).
//!
//! Serialization is manual, fixed-layout, little-endian. We avoid a derive-macro
//! dependency (keeps the crate graph to just solana-program) and get a known,
//! stable byte layout for a fixed-size account.

use solana_program::{program_error::ProgramError, pubkey::Pubkey};

use crate::errors::ReversibleSendError;

/// Lifecycle state of a transfer.
///
/// In practice only `Pending` is ever persisted: every terminal action closes the
/// account rather than leaving a `Cancelled` / `Released` record. The enum still
/// exists so the on-chain byte carries explicit meaning and future variants (e.g.
/// a non-closing terminal state) remain representable.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferState {
    /// Funds locked, awaiting accept / cancel / expiry-refund.
    Pending = 0,
    /// Sender cancelled (not persisted; account is closed).
    Cancelled = 1,
    /// Recipient accepted or funds refunded (not persisted; account is closed).
    Released = 2,
}

impl TransferState {
    fn from_u8(v: u8) -> Result<Self, ProgramError> {
        match v {
            0 => Ok(TransferState::Pending),
            1 => Ok(TransferState::Cancelled),
            2 => Ok(TransferState::Released),
            _ => Err(ReversibleSendError::InvalidAccountData.into()),
        }
    }
}

/// The per-transfer account stored at the PDA.
///
/// Fixed byte layout (little-endian), total [`Transfer::LEN`] bytes:
///
/// | offset | size | field       |
/// |--------|------|-------------|
/// | 0      | 32   | sender      |
/// | 32     | 32   | recipient   |
/// | 64     | 8    | amount      |
/// | 72     | 8    | created_at  |
/// | 80     | 8    | claim_window|
/// | 88     | 8    | seed        |
/// | 96     | 1    | bump        |
/// | 97     | 1    | state       |
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Transfer {
    /// Who created and funded the transfer — the only party who can cancel, and
    /// the party refunded on expiry.
    pub sender: Pubkey,
    /// Designated receiver — the only party who can accept.
    pub recipient: Pubkey,
    /// Lamports locked (held by this account).
    pub amount: u64,
    /// Unix timestamp (from `Clock`) at creation.
    pub created_at: i64,
    /// Seconds the recipient has to accept; after this, refund-to-sender opens.
    pub claim_window: i64,
    /// Uniqueness seed used in PDA derivation (lets one sender have many
    /// concurrent transfers to the same recipient).
    pub seed: u64,
    /// PDA bump.
    pub bump: u8,
    /// Lifecycle state (see [`TransferState`]).
    pub state: u8,
}

impl Transfer {
    /// Fixed serialized size in bytes.
    pub const LEN: usize = 32 + 32 + 8 + 8 + 8 + 8 + 1 + 1;

    /// First PDA seed literal.
    pub const SEED_PREFIX: &'static [u8] = b"transfer";

    /// Serialize into a fixed-size byte buffer (must be exactly [`Transfer::LEN`]).
    pub fn pack_into(&self, dst: &mut [u8]) -> Result<(), ProgramError> {
        if dst.len() != Self::LEN {
            return Err(ReversibleSendError::InvalidAccountData.into());
        }
        dst[0..32].copy_from_slice(self.sender.as_ref());
        dst[32..64].copy_from_slice(self.recipient.as_ref());
        dst[64..72].copy_from_slice(&self.amount.to_le_bytes());
        dst[72..80].copy_from_slice(&self.created_at.to_le_bytes());
        dst[80..88].copy_from_slice(&self.claim_window.to_le_bytes());
        dst[88..96].copy_from_slice(&self.seed.to_le_bytes());
        dst[96] = self.bump;
        dst[97] = self.state;
        Ok(())
    }

    /// Deserialize from a byte buffer (must be exactly [`Transfer::LEN`]).
    pub fn unpack_from(src: &[u8]) -> Result<Self, ProgramError> {
        if src.len() != Self::LEN {
            return Err(ReversibleSendError::InvalidAccountData.into());
        }
        let mut sender = [0u8; 32];
        sender.copy_from_slice(&src[0..32]);
        let mut recipient = [0u8; 32];
        recipient.copy_from_slice(&src[32..64]);

        let amount = u64::from_le_bytes(src[64..72].try_into().unwrap());
        let created_at = i64::from_le_bytes(src[72..80].try_into().unwrap());
        let claim_window = i64::from_le_bytes(src[80..88].try_into().unwrap());
        let seed = u64::from_le_bytes(src[88..96].try_into().unwrap());
        let bump = src[96];
        let state = src[97];

        // Validate the state byte is a known variant.
        let _ = TransferState::from_u8(state)?;

        Ok(Transfer {
            sender: Pubkey::new_from_array(sender),
            recipient: Pubkey::new_from_array(recipient),
            amount,
            created_at,
            claim_window,
            seed,
            bump,
            state,
        })
    }

    /// Returns true if the transfer is still Pending.
    pub fn is_pending(&self) -> bool {
        self.state == TransferState::Pending as u8
    }
}
