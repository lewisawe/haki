//! FR5 correctness / invariant suite for `reversible_send`.
//!
//! Each `#[test]` runs against a real local `solana-test-validator` with the
//! program preloaded. The whole suite shares ONE validator (spawned lazily, torn
//! down when the process exits) and ONE funded payer, because spinning a fresh
//! validator per test is slow and unnecessary — every test uses fresh keypairs
//! and a unique PDA `seed`, so they never collide on state.
//!
//! Tests run SINGLE-THREADED (`--test-threads=1`, documented in TESTS.md) so the
//! shared validator and the wall-clock window tests are deterministic.
//!
//! "Passing" for a negative test means the expected `ProgramError::Custom(n)` was
//! observed (the chain refused the illegal action) — see `assert_custom_error`.
//!
//! Error-code mapping (errors.rs / NOTES.md): 1 NotSender · 2 NotRecipient ·
//! 3 NotPending · 4 WindowExpired · 5 WindowNotExpired · 6 AmountTooSmall ·
//! 7 InvalidClaimWindow · 8 AccountMismatch · 9 InvalidPda · 11 AlreadyInitialized ·
//! 13 InvalidAccountOwner.

use std::sync::atomic::{AtomicU64, Ordering};

use reversible_send_tests::*;
use solana_sdk::{
    native_token::LAMPORTS_PER_SOL,
    signature::{Keypair, Signer},
};

// --- Each test gets its OWN validator (local RAII) --------------------------
//
// A local `TestValidator` is dropped at the end of each `#[test]` — on success
// AND on panic unwind — so `Drop` (which kills the child) always runs and no
// orphaned validator is left holding a port. (A `static`/shared validator is
// faster but Rust never runs `Drop` on statics at process exit, which orphaned a
// validator every run; correctness + clean teardown win here.) Each validator
// also uses OS-assigned free ports, so even a hard external kill never blocks a
// rerun on a busy port.
//
// Tests still run SINGLE-THREADED (`--test-threads=1`, see TESTS.md): the
// wall-clock window tests need a calm machine, and booting 18 validators in
// parallel would thrash. Per-test boot adds ~2s/test (~90s total).

/// A monotonic PDA-seed source (unique transfer account per create, even within
/// a test that makes several).
static SEED_CTR: AtomicU64 = AtomicU64::new(1);

fn next_seed() -> u64 {
    SEED_CTR.fetch_add(1, Ordering::SeqCst)
}

/// A standard amount comfortably above rent (rent for 98 bytes ≈ 0.0016 SOL).
const AMT: u64 = LAMPORTS_PER_SOL / 10; // 0.1 SOL

// ===========================================================================
// HAPPY PATHS
// ===========================================================================

#[test]
fn happy_create_then_cancel_refunds_sender_and_closes() {
    let v = TestValidator::start();
    let client = v.client();
    let sender = funded_keypair(&client, 2);
    let recipient = Keypair::new(); // need not be funded
    let seed = next_seed();
    let (pda, _) = transfer_pda(&sender.pubkey(), &recipient.pubkey(), seed);

    let before = balance(&client, &sender.pubkey());

    send_tx(
        &client,
        &sender,
        &[ix_create(&sender.pubkey(), &recipient.pubkey(), seed, AMT, 60)],
        &[&sender],
    )
    .expect("create should succeed");

    assert!(pda_is_live_pending(&client, &pda), "PDA must be live Pending after create");
    // Sender debited roughly AMT (+ fees) while funds are locked.
    let during = balance(&client, &sender.pubkey());
    assert!(during <= before - AMT, "sender should be debited the locked amount");

    send_tx(
        &client,
        &sender,
        &[ix_cancel(&sender.pubkey(), &recipient.pubkey(), seed, None)],
        &[&sender],
    )
    .expect("cancel should succeed");

    assert!(pda_is_closed(&client, &pda), "PDA must be closed after cancel");
    // Sender got the full locked amount back (minus only tx fees).
    let after = balance(&client, &sender.pubkey());
    assert!(
        after > during,
        "sender balance must increase after refund-cancel (during={during}, after={after})"
    );
    assert!(
        after >= before - 2 * 5_000, // at most ~2 signatures of fees across 2 txns
        "sender should be whole minus only fees (before={before}, after={after})"
    );
    // Recipient never received anything.
    assert_eq!(balance(&client, &recipient.pubkey()), 0, "recipient must stay at 0 on cancel");
}

#[test]
fn happy_create_then_accept_before_expiry_pays_recipient_and_closes() {
    let v = TestValidator::start();
    let client = v.client();
    let sender = funded_keypair(&client, 2);
    let recipient = funded_keypair(&client, 1); // funded so it can pay its own accept fee
    let seed = next_seed();
    let (pda, _) = transfer_pda(&sender.pubkey(), &recipient.pubkey(), seed);

    let rcpt_before = balance(&client, &recipient.pubkey());

    send_tx(
        &client,
        &sender,
        &[ix_create(&sender.pubkey(), &recipient.pubkey(), seed, AMT, 60)],
        &[&sender],
    )
    .expect("create should succeed");
    assert!(pda_is_live_pending(&client, &pda));

    send_tx(
        &client,
        &recipient,
        &[ix_accept(&recipient.pubkey(), &sender.pubkey(), seed, None)],
        &[&recipient],
    )
    .expect("accept before expiry should succeed");

    assert!(pda_is_closed(&client, &pda), "PDA must be closed after accept");
    let rcpt_after = balance(&client, &recipient.pubkey());
    // Recipient received the full locked amount (minus just its own accept fee).
    assert!(
        rcpt_after >= rcpt_before + AMT - 5_000,
        "recipient must receive ~full amount (before={rcpt_before}, after={rcpt_after}, amt={AMT})"
    );
}

#[test]
fn happy_create_then_refund_after_window_pays_sender_and_closes() {
    let v = TestValidator::start();
    let client = v.client();
    let sender = funded_keypair(&client, 2);
    let recipient = Keypair::new();
    let cranker_kp = funded_keypair(&client, 1); // permissionless: a neutral third party cranks
    let cranker = &cranker_kp;
    let seed = next_seed();
    let (pda, _) = transfer_pda(&sender.pubkey(), &recipient.pubkey(), seed);

    send_tx(
        &client,
        &sender,
        &[ix_create(&sender.pubkey(), &recipient.pubkey(), seed, AMT, 10)], // min window
        &[&sender],
    )
    .expect("create should succeed");
    assert!(pda_is_live_pending(&client, &pda));

    let sender_before_refund = balance(&client, &sender.pubkey());

    // Deterministically wait until the on-chain clock has passed expiry (reads
    // the real on-chain created_at, so confirmation latency can't race the 10s
    // window the way a fixed sleep did).
    wait_until_past_expiry(&client, &pda);

    send_tx(
        &client,
        cranker,
        &[ix_refund(
            &cranker.pubkey(),
            &sender.pubkey(),
            &recipient.pubkey(),
            seed,
            None,
            None,
        )],
        &[cranker],
    )
    .expect("refund after window should succeed");

    assert!(pda_is_closed(&client, &pda), "PDA must be closed after refund");
    let sender_after_refund = balance(&client, &sender.pubkey());
    // Sender (NOT the cranker, NOT the recipient) received the funds.
    assert!(
        sender_after_refund >= sender_before_refund + AMT - 1_000,
        "sender must receive the refund (before={sender_before_refund}, after={sender_after_refund})"
    );
    assert_eq!(balance(&client, &recipient.pubkey()), 0, "recipient must stay at 0 on refund");
}

// ===========================================================================
// INVARIANTS — each MUST fail with the exact error
// ===========================================================================

#[test]
fn inv_cancel_by_non_sender_fails_not_sender() {
    let v = TestValidator::start();
    let client = v.client();
    let sender = funded_keypair(&client, 2);
    let recipient = Keypair::new();
    let attacker = funded_keypair(&client, 1);
    let seed = next_seed();

    send_tx(
        &client,
        &sender,
        &[ix_create(&sender.pubkey(), &recipient.pubkey(), seed, AMT, 60)],
        &[&sender],
    )
    .expect("create should succeed");

    // Attacker signs a cancel but is not the recorded sender. The cancel ix takes
    // the "sender" slot as a signer, so the attacker passes itself there; the
    // program re-derives the PDA from STORED parties, so passing the attacker as
    // sender makes the derived PDA differ from the real account -> InvalidPda?
    // No: cancel loads the stored Transfer from the passed PDA first, so we pass
    // the REAL pda but a different signer; the stored sender != signer -> NotSender.
    let real_pda = transfer_pda(&sender.pubkey(), &recipient.pubkey(), seed).0;
    let ix = ix_cancel(&attacker.pubkey(), &recipient.pubkey(), seed, Some(real_pda));
    let res = send_tx(&client, &attacker, &[ix], &[&attacker]);
    assert_custom_error(&res, E_NOT_SENDER, "cancel-by-non-sender");

    // And the transfer is still live (nothing drained).
    assert!(pda_is_live_pending(&client, &real_pda), "transfer must remain pending after failed cancel");
}

#[test]
fn inv_accept_by_non_recipient_fails_not_recipient() {
    let v = TestValidator::start();
    let client = v.client();
    let sender = funded_keypair(&client, 2);
    let recipient = Keypair::new();
    let attacker = funded_keypair(&client, 1);
    let seed = next_seed();

    send_tx(
        &client,
        &sender,
        &[ix_create(&sender.pubkey(), &recipient.pubkey(), seed, AMT, 60)],
        &[&sender],
    )
    .expect("create should succeed");

    let real_pda = transfer_pda(&sender.pubkey(), &recipient.pubkey(), seed).0;
    // Attacker (not the recorded recipient) tries to accept, pointing at the real PDA.
    let ix = ix_accept(&attacker.pubkey(), &sender.pubkey(), seed, Some(real_pda));
    let res = send_tx(&client, &attacker, &[ix], &[&attacker]);
    assert_custom_error(&res, E_NOT_RECIPIENT, "accept-by-non-recipient");
    assert!(pda_is_live_pending(&client, &real_pda), "transfer must remain pending after failed accept");
}

#[test]
fn inv_accept_after_window_fails_window_expired() {
    let v = TestValidator::start();
    let client = v.client();
    let sender = funded_keypair(&client, 2);
    let recipient = funded_keypair(&client, 1);
    let seed = next_seed();
    let (pda, _) = transfer_pda(&sender.pubkey(), &recipient.pubkey(), seed);

    send_tx(
        &client,
        &sender,
        &[ix_create(&sender.pubkey(), &recipient.pubkey(), seed, AMT, 10)],
        &[&sender],
    )
    .expect("create should succeed");

    wait_until_past_expiry(&client, &pda); // window (10s) has now provably passed

    let res = send_tx(
        &client,
        &recipient,
        &[ix_accept(&recipient.pubkey(), &sender.pubkey(), seed, None)],
        &[&recipient],
    );
    assert_custom_error(&res, E_WINDOW_EXPIRED, "accept-after-expiry");
    assert!(pda_is_live_pending(&client, &pda), "transfer must remain pending after failed late accept");
}

#[test]
fn inv_refund_before_window_fails_window_not_expired() {
    let v = TestValidator::start();
    let client = v.client();
    let sender = funded_keypair(&client, 2);
    let recipient = Keypair::new();
    let cranker_kp = funded_keypair(&client, 1);
    let cranker = &cranker_kp;
    let seed = next_seed();
    let (pda, _) = transfer_pda(&sender.pubkey(), &recipient.pubkey(), seed);

    send_tx(
        &client,
        &sender,
        &[ix_create(&sender.pubkey(), &recipient.pubkey(), seed, AMT, 3600)], // long window
        &[&sender],
    )
    .expect("create should succeed");

    // No sleep — we are well inside the window.
    let res = send_tx(
        &client,
        cranker,
        &[ix_refund(&cranker.pubkey(), &sender.pubkey(), &recipient.pubkey(), seed, None, None)],
        &[cranker],
    );
    assert_custom_error(&res, E_WINDOW_NOT_EXPIRED, "refund-before-expiry");
    assert!(pda_is_live_pending(&client, &pda), "transfer must remain pending after failed early refund");
}

#[test]
fn inv_double_cancel_second_fails_closed() {
    let v = TestValidator::start();
    let client = v.client();
    let sender = funded_keypair(&client, 2);
    let recipient = Keypair::new();
    let seed = next_seed();
    let (pda, _) = transfer_pda(&sender.pubkey(), &recipient.pubkey(), seed);

    send_tx(
        &client,
        &sender,
        &[ix_create(&sender.pubkey(), &recipient.pubkey(), seed, AMT, 60)],
        &[&sender],
    )
    .expect("create should succeed");

    // First cancel succeeds + closes.
    send_tx(
        &client,
        &sender,
        &[ix_cancel(&sender.pubkey(), &recipient.pubkey(), seed, None)],
        &[&sender],
    )
    .expect("first cancel should succeed");
    assert!(pda_is_closed(&client, &pda));

    // Second cancel on the now-closed account must fail. The account is purged /
    // not program-owned, so load_transfer rejects it (InvalidAccountOwner) — a
    // terminal action on an already-terminal transfer is impossible (replay-proof).
    let res = send_tx(
        &client,
        &sender,
        &[ix_cancel(&sender.pubkey(), &recipient.pubkey(), seed, None)],
        &[&sender],
    );
    assert_terminal_blocked(&res, "double-cancel");
}

#[test]
fn inv_accept_after_cancel_fails_closed() {
    let v = TestValidator::start();
    let client = v.client();
    let sender = funded_keypair(&client, 2);
    let recipient = funded_keypair(&client, 1);
    let seed = next_seed();
    let (pda, _) = transfer_pda(&sender.pubkey(), &recipient.pubkey(), seed);

    send_tx(
        &client,
        &sender,
        &[ix_create(&sender.pubkey(), &recipient.pubkey(), seed, AMT, 60)],
        &[&sender],
    )
    .expect("create should succeed");
    send_tx(
        &client,
        &sender,
        &[ix_cancel(&sender.pubkey(), &recipient.pubkey(), seed, None)],
        &[&sender],
    )
    .expect("cancel should succeed");
    assert!(pda_is_closed(&client, &pda));

    // Recipient now tries to accept the cancelled (closed) transfer: impossible.
    let res = send_tx(
        &client,
        &recipient,
        &[ix_accept(&recipient.pubkey(), &sender.pubkey(), seed, None)],
        &[&recipient],
    );
    assert_terminal_blocked(&res, "accept-after-cancel");
}

#[test]
fn inv_create_amount_at_or_below_rent_fails_amount_too_small() {
    let v = TestValidator::start();
    let client = v.client();
    let sender = funded_keypair(&client, 2);
    let recipient = Keypair::new();
    let seed = next_seed();

    let rent = transfer_rent(&client);

    // amount == rent exactly must be rejected (program requires amount > rent).
    let res_eq = send_tx(
        &client,
        &sender,
        &[ix_create(&sender.pubkey(), &recipient.pubkey(), seed, rent, 60)],
        &[&sender],
    );
    assert_custom_error(&res_eq, E_AMOUNT_TOO_SMALL, "amount==rent");

    // amount just below rent must also be rejected.
    let seed2 = next_seed();
    let res_lt = send_tx(
        &client,
        &sender,
        &[ix_create(&sender.pubkey(), &recipient.pubkey(), seed2, rent - 1, 60)],
        &[&sender],
    );
    assert_custom_error(&res_lt, E_AMOUNT_TOO_SMALL, "amount<rent");
}

#[test]
fn inv_create_claim_window_out_of_bounds_fails_invalid_claim_window() {
    let v = TestValidator::start();
    let client = v.client();
    let sender = funded_keypair(&client, 2);
    let recipient = Keypair::new();

    // Below MIN (10): 9s.
    let res_low = send_tx(
        &client,
        &sender,
        &[ix_create(&sender.pubkey(), &recipient.pubkey(), next_seed(), AMT, 9)],
        &[&sender],
    );
    assert_custom_error(&res_low, E_INVALID_CLAIM_WINDOW, "window=9 (below min)");

    // Above MAX (3600): 3601s.
    let res_high = send_tx(
        &client,
        &sender,
        &[ix_create(&sender.pubkey(), &recipient.pubkey(), next_seed(), AMT, 3601)],
        &[&sender],
    );
    assert_custom_error(&res_high, E_INVALID_CLAIM_WINDOW, "window=3601 (above max)");

    // Negative window.
    let res_neg = send_tx(
        &client,
        &sender,
        &[ix_create(&sender.pubkey(), &recipient.pubkey(), next_seed(), AMT, -5)],
        &[&sender],
    );
    assert_custom_error(&res_neg, E_INVALID_CLAIM_WINDOW, "window=-5 (negative)");
}

#[test]
fn inv_create_with_forged_pda_fails_invalid_pda() {
    let v = TestValidator::start();
    let client = v.client();
    let sender = funded_keypair(&client, 2);
    let recipient = Keypair::new();
    let seed = next_seed();

    // Build a create ix but SWAP in a bogus PDA (a random pubkey that is not the
    // real derived address). On create, the program derives+checks the PDA BEFORE
    // creating the account, so a forged PDA => InvalidPda (9).
    let mut ix = ix_create(&sender.pubkey(), &recipient.pubkey(), seed, AMT, 60);
    let forged = Keypair::new().pubkey();
    // account[1] is the transfer PDA slot.
    ix.accounts[1].pubkey = forged;
    let res = send_tx(&client, &sender, &[ix], &[&sender]);
    assert_custom_error(&res, E_INVALID_PDA, "create-with-forged-pda");
}

#[test]
fn inv_terminal_action_on_forged_pda_fails_invalid_owner() {
    // A terminal action (cancel) pointed at a random/forged account that the
    // program does not own: load_transfer rejects it with InvalidAccountOwner (13)
    // before any payout. This protects against passing an attacker-controlled or
    // non-existent account to drain funds.
    let v = TestValidator::start();
    let client = v.client();
    let sender = funded_keypair(&client, 2);
    let recipient = Keypair::new();
    let seed = next_seed();

    let forged = Keypair::new().pubkey(); // not program-owned, doesn't exist
    let ix = ix_cancel(&sender.pubkey(), &recipient.pubkey(), seed, Some(forged));
    let res = send_tx(&client, &sender, &[ix], &[&sender]);
    assert_custom_error(&res, E_INVALID_ACCOUNT_OWNER, "cancel-on-forged-pda");
}

#[test]
fn inv_create_duplicate_fails_already_initialized() {
    let v = TestValidator::start();
    let client = v.client();
    let sender = funded_keypair(&client, 2);
    let recipient = Keypair::new();
    let seed = next_seed();

    send_tx(
        &client,
        &sender,
        &[ix_create(&sender.pubkey(), &recipient.pubkey(), seed, AMT, 60)],
        &[&sender],
    )
    .expect("first create should succeed");

    // Second create with the SAME (sender, recipient, seed) targets the same PDA
    // which already exists / is program-owned -> AlreadyInitialized (11).
    let res = send_tx(
        &client,
        &sender,
        &[ix_create(&sender.pubkey(), &recipient.pubkey(), seed, AMT, 60)],
        &[&sender],
    );
    assert_custom_error(&res, E_ALREADY_INITIALIZED, "duplicate-create");
}

#[test]
fn inv_refund_to_wrong_sender_account_fails_account_mismatch() {
    let v = TestValidator::start();
    let client = v.client();
    let sender = funded_keypair(&client, 2);
    let recipient = Keypair::new();
    let cranker_kp = funded_keypair(&client, 1);
    let cranker = &cranker_kp;
    let wrong_payee = Keypair::new();
    let seed = next_seed();
    let (pda, _) = transfer_pda(&sender.pubkey(), &recipient.pubkey(), seed);

    send_tx(
        &client,
        &sender,
        &[ix_create(&sender.pubkey(), &recipient.pubkey(), seed, AMT, 10)],
        &[&sender],
    )
    .expect("create should succeed");

    wait_until_past_expiry(&client, &pda);

    // Refund after expiry BUT paying a wrong "sender" account -> AccountMismatch (8).
    let res = send_tx(
        &client,
        cranker,
        &[ix_refund(
            &cranker.pubkey(),
            &sender.pubkey(),
            &recipient.pubkey(),
            seed,
            Some(wrong_payee.pubkey()), // wrong payee account
            None,
        )],
        &[cranker],
    );
    assert_custom_error(&res, E_ACCOUNT_MISMATCH, "refund-to-wrong-sender");
    // Nothing drained — still pending, can still be refunded to the real sender.
    assert!(pda_is_live_pending(&client, &pda), "transfer must remain pending after failed mis-addressed refund");
}

// ===========================================================================
// SETTLEMENT invariants — exactly one payout, full amount, never both/neither
// ===========================================================================

#[test]
fn settle_accept_pays_only_recipient_full_amount_not_sender() {
    let v = TestValidator::start();
    let client = v.client();
    let sender = funded_keypair(&client, 2);
    let recipient = funded_keypair(&client, 1);
    let seed = next_seed();
    let (pda, _) = transfer_pda(&sender.pubkey(), &recipient.pubkey(), seed);

    send_tx(
        &client,
        &sender,
        &[ix_create(&sender.pubkey(), &recipient.pubkey(), seed, AMT, 120)],
        &[&sender],
    )
    .expect("create should succeed");

    let sender_after_create = balance(&client, &sender.pubkey());
    let rcpt_before = balance(&client, &recipient.pubkey());
    let locked = balance(&client, &pda);
    // The PDA holds exactly the created amount (it is the vault).
    assert_eq!(locked, AMT, "PDA must hold exactly the locked amount");

    send_tx(
        &client,
        &recipient,
        &[ix_accept(&recipient.pubkey(), &sender.pubkey(), seed, None)],
        &[&recipient],
    )
    .expect("accept should succeed");

    // Exactly one payout: recipient got the FULL amount; sender got NOTHING back.
    let rcpt_after = balance(&client, &recipient.pubkey());
    let sender_final = balance(&client, &sender.pubkey());
    assert!(
        rcpt_after >= rcpt_before + AMT - 5_000 && rcpt_after <= rcpt_before + AMT,
        "recipient must get the full amount, no more no less (before={rcpt_before}, after={rcpt_after}, amt={AMT})"
    );
    assert!(
        sender_final <= sender_after_create,
        "sender must NOT be credited on accept (after_create={sender_after_create}, final={sender_final})"
    );
    assert!(pda_is_closed(&client, &pda), "vault emptied + closed (never neither)");
}

#[test]
fn settle_no_partial_drain_full_amount_moves() {
    // Prove the whole locked balance moves in one terminal action (no dust left,
    // no partial drain) — here via cancel back to the sender.
    let v = TestValidator::start();
    let client = v.client();
    let sender = funded_keypair(&client, 2);
    let recipient = Keypair::new();
    let seed = next_seed();
    let (pda, _) = transfer_pda(&sender.pubkey(), &recipient.pubkey(), seed);

    let big = LAMPORTS_PER_SOL / 2; // 0.5 SOL, well above rent
    send_tx(
        &client,
        &sender,
        &[ix_create(&sender.pubkey(), &recipient.pubkey(), seed, big, 60)],
        &[&sender],
    )
    .expect("create should succeed");
    assert_eq!(balance(&client, &pda), big, "PDA must hold exactly the locked amount");

    send_tx(
        &client,
        &sender,
        &[ix_cancel(&sender.pubkey(), &recipient.pubkey(), seed, None)],
        &[&sender],
    )
    .expect("cancel should succeed");

    // Vault fully drained to 0 — not a partial amount.
    assert_eq!(balance(&client, &pda), 0, "vault must be fully drained (no partial / no dust)");
    assert!(pda_is_closed(&client, &pda));
}

#[test]
fn settle_pending_vault_is_neither_paid_until_terminal_action() {
    // While pending (no terminal action), funds are with NEITHER party — they sit
    // locked in the vault. This is the "never neither while the account exists"
    // boundary: the account exists and holds the funds; no party has them yet.
    let v = TestValidator::start();
    let client = v.client();
    let sender = funded_keypair(&client, 2);
    let recipient = funded_keypair(&client, 1);
    let seed = next_seed();
    let (pda, _) = transfer_pda(&sender.pubkey(), &recipient.pubkey(), seed);

    send_tx(
        &client,
        &sender,
        &[ix_create(&sender.pubkey(), &recipient.pubkey(), seed, AMT, 60)],
        &[&sender],
    )
    .expect("create should succeed");

    // Funds are in the vault; recipient has not been paid; sender has been debited.
    assert_eq!(balance(&client, &pda), AMT, "funds held by the vault while pending");
    assert!(pda_is_live_pending(&client, &pda), "exactly one live Pending record exists");

    // Clean up by accepting so we also confirm the single-payout transition works.
    send_tx(
        &client,
        &recipient,
        &[ix_accept(&recipient.pubkey(), &sender.pubkey(), seed, None)],
        &[&recipient],
    )
    .expect("accept should succeed");
    assert!(pda_is_closed(&client, &pda));
}

// ===========================================================================
// Helpers local to the tests
// ===========================================================================

/// Assert a terminal action on an already-terminal / closed transfer was blocked.
/// Depending on whether the validator has purged the zero-lamport account yet,
/// the program rejects it either as `InvalidAccountOwner` (account purged / not
/// program-owned) or `NotPending` (zeroed data still readable as non-Pending) —
/// both mean "no second payout" (replay-proof), which is the invariant.
fn assert_terminal_blocked(
    res: &Result<solana_sdk::signature::Signature, solana_sdk::transaction::TransactionError>,
    context: &str,
) {
    match res {
        Ok(sig) => panic!("[{context}] terminal action on closed transfer SUCCEEDED (sig {sig}) — replay NOT prevented"),
        Err(e) => match custom_code(e) {
            Some(E_NOT_PENDING) | Some(E_INVALID_ACCOUNT_OWNER) | Some(E_INVALID_ACCOUNT_DATA) => {}
            Some(other) => panic!("[{context}] expected NotPending/InvalidAccountOwner, got Custom({other})"),
            None => panic!("[{context}] expected a custom error, got: {e:?}"),
        },
    }
}
