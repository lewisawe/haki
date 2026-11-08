//! Instruction processing — the on-chain enforcement of every FR5 invariant.
//!
//! Funds are held by the Transfer PDA itself. Exactly one terminal path drains
//! and closes it:
//!   - `cancel`  → sender  (requires Pending + signer == sender)
//!   - `accept`  → recipient (requires Pending + signer == recipient + now < expiry)
//!   - `refund`  → sender  (requires Pending + now >= expiry)
//! Closing zeroes the data and sweeps lamports, so no action can run twice.

use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint::ProgramResult,
    msg,
    program::invoke_signed,
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction,
    system_program,
    sysvar::Sysvar,
};

use crate::{
    errors::ReversibleSendError,
    instruction::ReversibleSendInstruction,
    state::{Transfer, TransferState},
};

/// Allowed claim-window bounds, in seconds.
const MIN_CLAIM_WINDOW: i64 = 10;
const MAX_CLAIM_WINDOW: i64 = 3600;

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let ix = ReversibleSendInstruction::unpack(instruction_data)?;
    match ix {
        ReversibleSendInstruction::CreateTransfer {
            amount,
            claim_window,
            seed,
        } => process_create_transfer(program_id, accounts, amount, claim_window, seed),
        ReversibleSendInstruction::CancelTransfer => process_cancel_transfer(program_id, accounts),
        ReversibleSendInstruction::AcceptTransfer => process_accept_transfer(program_id, accounts),
        ReversibleSendInstruction::RefundExpired => process_refund_expired(program_id, accounts),
    }
}

/// Derive the Transfer PDA for the given parties/seed and assert it matches the
/// account that was passed in. Returns the canonical bump.
fn derive_and_check_pda(
    program_id: &Pubkey,
    transfer_account: &AccountInfo,
    sender: &Pubkey,
    recipient: &Pubkey,
    seed: u64,
) -> Result<u8, ProgramError> {
    let (expected, bump) = Pubkey::find_program_address(
        &[
            Transfer::SEED_PREFIX,
            sender.as_ref(),
            recipient.as_ref(),
            &seed.to_le_bytes(),
        ],
        program_id,
    );
    if expected != *transfer_account.key {
        msg!("PDA mismatch: expected {}, got {}", expected, transfer_account.key);
        return Err(ReversibleSendError::InvalidPda.into());
    }
    Ok(bump)
}

/// Load and validate the stored Transfer from an account owned by this program.
fn load_transfer(
    program_id: &Pubkey,
    transfer_account: &AccountInfo,
) -> Result<Transfer, ProgramError> {
    if transfer_account.owner != program_id {
        return Err(ReversibleSendError::InvalidAccountOwner.into());
    }
    let data = transfer_account.try_borrow_data()?;
    Transfer::unpack_from(&data)
}

/// Move all lamports out of `from` into `to` and zero `from`'s data, effectively
/// closing the PDA. `from` must be owned by this program (so we may mutate its
/// lamports directly). After this returns, `from` has 0 lamports and 0-filled
/// data — it can never satisfy `is_pending` again, preventing replay.
fn close_and_pay(from: &AccountInfo, to: &AccountInfo) -> ProgramResult {
    let amount = from.lamports();

    // Credit the recipient of the funds.
    let mut to_lamports = to.try_borrow_mut_lamports()?;
    **to_lamports = to_lamports
        .checked_add(amount)
        .ok_or(ReversibleSendError::ArithmeticOverflow)?;
    drop(to_lamports);

    // Debit (empty) the PDA.
    let mut from_lamports = from.try_borrow_mut_lamports()?;
    **from_lamports = 0;
    drop(from_lamports);

    // Zero the data so a stale account can't be mis-read as Pending.
    let mut data = from.try_borrow_mut_data()?;
    for b in data.iter_mut() {
        *b = 0;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// create_transfer
// ---------------------------------------------------------------------------
fn process_create_transfer(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    amount: u64,
    claim_window: i64,
    seed: u64,
) -> ProgramResult {
    let account_iter = &mut accounts.iter();
    let sender = next_account_info(account_iter)?;
    let transfer_account = next_account_info(account_iter)?;
    let recipient = next_account_info(account_iter)?;
    let system_prog = next_account_info(account_iter)?;

    // Signer check: only the sender may create+fund.
    if !sender.is_signer {
        return Err(ReversibleSendError::MissingRequiredSignature.into());
    }

    // System program must be the real one (we CPI into it).
    if system_prog.key != &system_program::ID {
        return Err(ReversibleSendError::AccountMismatch.into());
    }

    // Validate claim window bounds.
    if claim_window < MIN_CLAIM_WINDOW || claim_window > MAX_CLAIM_WINDOW {
        return Err(ReversibleSendError::InvalidClaimWindow.into());
    }

    // Derive + verify the PDA.
    let bump = derive_and_check_pda(program_id, transfer_account, sender.key, recipient.key, seed)?;

    // The PDA must not already exist / be initialized.
    if transfer_account.owner != &system_program::ID || transfer_account.lamports() != 0 {
        return Err(ReversibleSendError::AlreadyInitialized.into());
    }

    // Rent-exempt minimum for the account. `amount` is the TOTAL lamports the PDA
    // will hold (it is the vault). We require amount to strictly exceed the rent
    // reserve so a terminal payout always moves a non-zero, spendable sum beyond
    // the reserve that is returned to the sender as rent.
    let rent = Rent::get()?;
    let rent_exempt = rent.minimum_balance(Transfer::LEN);
    if amount <= rent_exempt {
        msg!(
            "amount {} must exceed rent-exempt minimum {}",
            amount,
            rent_exempt
        );
        return Err(ReversibleSendError::AmountTooSmall.into());
    }

    // Create the PDA account with `amount` lamports, owned by this program.
    let signer_seeds: &[&[u8]] = &[
        Transfer::SEED_PREFIX,
        sender.key.as_ref(),
        recipient.key.as_ref(),
        &seed.to_le_bytes(),
        &[bump],
    ];
    invoke_signed(
        &system_instruction::create_account(
            sender.key,
            transfer_account.key,
            amount,
            Transfer::LEN as u64,
            program_id,
        ),
        &[sender.clone(), transfer_account.clone(), system_prog.clone()],
        &[signer_seeds],
    )?;

    // Stamp and persist state.
    let clock = Clock::get()?;
    let transfer = Transfer {
        sender: *sender.key,
        recipient: *recipient.key,
        amount,
        created_at: clock.unix_timestamp,
        claim_window,
        seed,
        bump,
        state: TransferState::Pending as u8,
    };
    let mut data = transfer_account.try_borrow_mut_data()?;
    transfer.pack_into(&mut data)?;

    msg!(
        "created transfer: {} lamports, window {}s, seed {}",
        amount,
        claim_window,
        seed
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// cancel_transfer
// ---------------------------------------------------------------------------
fn process_cancel_transfer(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let account_iter = &mut accounts.iter();
    let sender = next_account_info(account_iter)?;
    let transfer_account = next_account_info(account_iter)?;

    if !sender.is_signer {
        return Err(ReversibleSendError::MissingRequiredSignature.into());
    }

    let transfer = load_transfer(program_id, transfer_account)?;

    // Re-derive the PDA from stored parties to guarantee this is the real account.
    derive_and_check_pda(
        program_id,
        transfer_account,
        &transfer.sender,
        &transfer.recipient,
        transfer.seed,
    )?;

    // Must still be Pending (not already terminal).
    if !transfer.is_pending() {
        return Err(ReversibleSendError::NotPending.into());
    }

    // Only the recorded sender may cancel.
    if transfer.sender != *sender.key {
        return Err(ReversibleSendError::NotSender.into());
    }

    // Refund everything to the sender and close.
    close_and_pay(transfer_account, sender)?;
    msg!("cancelled transfer: refunded sender");
    Ok(())
}

// ---------------------------------------------------------------------------
// accept_transfer — the ONLY path funds reach the recipient
// ---------------------------------------------------------------------------
fn process_accept_transfer(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let account_iter = &mut accounts.iter();
    let recipient = next_account_info(account_iter)?;
    let transfer_account = next_account_info(account_iter)?;

    if !recipient.is_signer {
        return Err(ReversibleSendError::MissingRequiredSignature.into());
    }

    let transfer = load_transfer(program_id, transfer_account)?;

    derive_and_check_pda(
        program_id,
        transfer_account,
        &transfer.sender,
        &transfer.recipient,
        transfer.seed,
    )?;

    if !transfer.is_pending() {
        return Err(ReversibleSendError::NotPending.into());
    }

    // Only the recorded recipient may accept.
    if transfer.recipient != *recipient.key {
        return Err(ReversibleSendError::NotRecipient.into());
    }

    // Must be before expiry.
    let clock = Clock::get()?;
    let expiry = transfer
        .created_at
        .checked_add(transfer.claim_window)
        .ok_or(ReversibleSendError::ArithmeticOverflow)?;
    if clock.unix_timestamp >= expiry {
        return Err(ReversibleSendError::WindowExpired.into());
    }

    // Pay the recipient the full locked balance and close. The PDA holds exactly
    // `amount` lamports (it was created with `amount`), so the entire balance —
    // the full locked amount — goes to the recipient (no partial drain, FR5).
    close_and_pay(transfer_account, recipient)?;
    msg!("accepted transfer: paid recipient in full");
    Ok(())
}

// ---------------------------------------------------------------------------
// refund_expired — permissionless auto-refund to the sender
// ---------------------------------------------------------------------------
fn process_refund_expired(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let account_iter = &mut accounts.iter();
    let cranker = next_account_info(account_iter)?;
    let transfer_account = next_account_info(account_iter)?;
    let sender = next_account_info(account_iter)?;

    // Permissionless, but the crank tx still needs a fee payer / signer.
    if !cranker.is_signer {
        return Err(ReversibleSendError::MissingRequiredSignature.into());
    }

    let transfer = load_transfer(program_id, transfer_account)?;

    derive_and_check_pda(
        program_id,
        transfer_account,
        &transfer.sender,
        &transfer.recipient,
        transfer.seed,
    )?;

    if !transfer.is_pending() {
        return Err(ReversibleSendError::NotPending.into());
    }

    // The refund always pays the recorded sender — verify the passed account.
    if transfer.sender != *sender.key {
        return Err(ReversibleSendError::AccountMismatch.into());
    }

    // Must be at/after expiry.
    let clock = Clock::get()?;
    let expiry = transfer
        .created_at
        .checked_add(transfer.claim_window)
        .ok_or(ReversibleSendError::ArithmeticOverflow)?;
    if clock.unix_timestamp < expiry {
        return Err(ReversibleSendError::WindowNotExpired.into());
    }

    close_and_pay(transfer_account, sender)?;
    msg!("refunded expired transfer: paid sender in full");
    Ok(())
}
