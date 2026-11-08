//! Test harness for the `reversible_send` native Solana program.
//!
//! The harness spawns a real `solana-test-validator` with the program preloaded
//! at genesis (`--bpf-program <ID> <so>`), talks to it over RPC with the blocking
//! `solana_client::rpc_client::RpcClient`, and exposes:
//!   - instruction builders matching the program's wire format EXACTLY (see
//!     `programs/reversible_send/NOTES.md`),
//!   - the PDA derivation (seeds `["transfer", sender, recipient, seed_le]`),
//!   - error-decode helpers that pull the `ProgramError::Custom(n)` code out of a
//!     failed transaction so tests can assert the *exact* invariant that fired.
//!
//! Why this harness (and not `solana-program-test` / LiteSVM / Mollusk / Anchor):
//! none of those are in the local cargo cache, and pulling them needs a long
//! multi-crate fetch that this machine's network drops (same failure that blocked
//! the Anchor install — see requirements.md "Toolchain status"). `solana-client`
//! + `solana-sdk` 1.18.26 ARE cached, so this harness builds fully offline, and
//! running against a real local validator is high-fidelity (real BPF execution,
//! real `Clock`, real rent).

use std::process::{Child, Command};
use std::time::{Duration, Instant};

use solana_client::client_error::ClientErrorKind;
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_request::{RpcError, RpcResponseErrorData};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction, InstructionError},
    native_token::LAMPORTS_PER_SOL,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_program,
    transaction::{Transaction, TransactionError},
};
use std::str::FromStr;

/// The program's on-chain id (must match `declare_id!` in the program's lib.rs and
/// the keypair at `target/deploy/reversible_send-keypair.json`).
pub const PROGRAM_ID_STR: &str = "EpwZhUpnRGfDf7mmST8DYXHyEVAJnT6rEgpiKdaQvNRh";

/// PDA seed prefix — `state::Transfer::SEED_PREFIX`.
pub const SEED_PREFIX: &[u8] = b"transfer";

// --- Error codes (ProgramError::Custom(n)) — mirror of errors.rs. -----------
pub const E_INVALID_INSTRUCTION: u32 = 0;
pub const E_NOT_SENDER: u32 = 1;
pub const E_NOT_RECIPIENT: u32 = 2;
pub const E_NOT_PENDING: u32 = 3;
pub const E_WINDOW_EXPIRED: u32 = 4;
pub const E_WINDOW_NOT_EXPIRED: u32 = 5;
pub const E_AMOUNT_TOO_SMALL: u32 = 6;
pub const E_INVALID_CLAIM_WINDOW: u32 = 7;
pub const E_ACCOUNT_MISMATCH: u32 = 8;
pub const E_INVALID_PDA: u32 = 9;
pub const E_MISSING_SIGNATURE: u32 = 10;
pub const E_ALREADY_INITIALIZED: u32 = 11;
#[allow(dead_code)]
pub const E_ARITHMETIC_OVERFLOW: u32 = 12;
pub const E_INVALID_ACCOUNT_OWNER: u32 = 13;
#[allow(dead_code)]
pub const E_INVALID_ACCOUNT_DATA: u32 = 14;

/// Offset of the `state` byte inside the 98-byte Transfer account (NOTES.md).
pub const TRANSFER_LEN: usize = 98;

pub fn program_id() -> Pubkey {
    Pubkey::from_str(PROGRAM_ID_STR).unwrap()
}

/// Ask the OS for a free TCP port (bind to :0, read the assigned port, release).
/// There is an inherent race between release and the validator re-binding, but in
/// practice the window is tiny and each call returns a distinct port, which is
/// enough to keep concurrent/retried validator spawns from colliding.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("could not bind an ephemeral port")
        .local_addr()
        .expect("local_addr failed")
        .port()
}

// ---------------------------------------------------------------------------
// Validator lifecycle
// ---------------------------------------------------------------------------

/// A running `solana-test-validator`, killed on drop.
pub struct TestValidator {
    child: Child,
    pub rpc_url: String,
    #[allow(dead_code)]
    ledger_dir: String,
}

impl TestValidator {
    /// Spawn a fresh validator with the program preloaded, on a unique RPC port,
    /// and block until RPC answers. Panics if it cannot come up in time.
    pub fn start() -> TestValidator {
        let solana_bin_dir = format!(
            "{}/.local/share/solana/install/active_release/bin",
            std::env::var("HOME").expect("HOME not set")
        );
        let validator_bin = format!("{solana_bin_dir}/solana-test-validator");

        // Resolve the .so relative to this crate (…/tests-client/) -> program root.
        let manifest_dir = env!("CARGO_MANIFEST_DIR"); // …/programs/reversible_send/tests-client
        let program_root = std::path::Path::new(manifest_dir)
            .parent()
            .expect("tests-client has a parent")
            .to_path_buf();
        let so_path = program_root.join("target/deploy/reversible_send.so");
        assert!(
            so_path.exists(),
            "program .so not found at {} — build it first (cargo-build-sbf)",
            so_path.display()
        );

        // Pick a guaranteed-free RPC port by letting the OS assign one, then
        // releasing it immediately before the validator binds it. This avoids the
        // pid-collision / stale-validator problem a fixed pid-derived port had.
        let rpc_port = free_port();
        let gossip_port = free_port();
        let dyn_lo = free_port();
        let dyn_hi = dyn_lo + 50;

        let ledger_dir = format!("/tmp/rs-test-ledger-{}-{}", std::process::id(), rpc_port);

        let mut cmd = Command::new(&validator_bin);
        cmd.args([
                "--reset",
                "--quiet",
                "--ledger",
                &ledger_dir,
                "--rpc-port",
                &rpc_port.to_string(),
                "--bind-address",
                "127.0.0.1",
                "--gossip-port",
                &gossip_port.to_string(),
                "--dynamic-port-range",
                &format!("{dyn_lo}-{dyn_hi}"),
                "--bpf-program",
                PROGRAM_ID_STR,
                so_path.to_str().unwrap(),
                // Faster clock so wall-clock window tests spend less real time
                // while still advancing Clock.unix_timestamp monotonically.
                "--ticks-per-slot",
                "8",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());

        let child = cmd
            .spawn()
            .unwrap_or_else(|e| panic!("failed to spawn {validator_bin}: {e}"));

        let rpc_url = format!("http://127.0.0.1:{rpc_port}");
        let mut tv = TestValidator {
            child,
            rpc_url: rpc_url.clone(),
            ledger_dir,
        };

        // Wait for RPC + the preloaded program to be queryable (up to ~60s). Fail
        // fast if the validator process has already exited (e.g. a port it needed
        // was taken) instead of blocking the whole timeout.
        let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());
        let deadline = Instant::now() + Duration::from_secs(60);
        let prog = program_id();
        loop {
            if let Ok(Some(status)) = tv.child.try_wait() {
                panic!("solana-test-validator exited early ({status}) before RPC came up — see if a port was busy");
            }
            if Instant::now() > deadline {
                panic!("validator RPC did not become ready within 60s");
            }
            if let Ok(acc) = client.get_account(&prog) {
                if acc.executable {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        tv
    }

    /// A fresh confirmed-commitment RPC client for this validator.
    pub fn client(&self) -> RpcClient {
        RpcClient::new_with_commitment(self.rpc_url.clone(), CommitmentConfig::confirmed())
    }
}

impl Drop for TestValidator {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// RPC convenience
// ---------------------------------------------------------------------------

/// Create a funded keypair (airdrop `sol` SOL, confirmed).
pub fn funded_keypair(client: &RpcClient, sol: u64) -> Keypair {
    let kp = Keypair::new();
    airdrop(client, &kp.pubkey(), sol);
    kp
}

/// Airdrop and wait until the balance reflects it.
pub fn airdrop(client: &RpcClient, who: &Pubkey, sol: u64) {
    let lamports = sol * LAMPORTS_PER_SOL;
    let sig = client
        .request_airdrop(who, lamports)
        .unwrap_or_else(|e| panic!("airdrop request failed for {who}: {e}"));
    // Confirm the airdrop landed.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(true) = client.confirm_transaction(&sig) {
            break;
        }
        if client.get_balance(who).unwrap_or(0) >= lamports {
            break;
        }
        if Instant::now() > deadline {
            panic!("airdrop to {who} not confirmed within 30s");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

pub fn balance(client: &RpcClient, who: &Pubkey) -> u64 {
    client.get_balance(who).unwrap_or(0)
}

/// Send a transaction signed by `signers` with `payer` as fee payer. Returns the
/// confirmed signature on success, or the `TransactionError` on failure.
pub fn send_tx(
    client: &RpcClient,
    payer: &Keypair,
    ixs: &[Instruction],
    signers: &[&Keypair],
) -> Result<solana_sdk::signature::Signature, TransactionError> {
    let blockhash = client
        .get_latest_blockhash()
        .expect("get_latest_blockhash failed");
    let mut tx = Transaction::new_with_payer(ixs, Some(&payer.pubkey()));
    tx.sign(signers, blockhash);
    match client.send_and_confirm_transaction(&tx) {
        Ok(sig) => Ok(sig),
        Err(e) => Err(extract_tx_error(e)),
    }
}

/// Pull the `TransactionError` out of a client error (preflight-simulated or
/// landed). Panics only for a genuinely unexpected shape (e.g. an RPC/network
/// error) so an infra problem is never silently read as a program error.
fn extract_tx_error(e: solana_client::client_error::ClientError) -> TransactionError {
    match e.kind {
        ClientErrorKind::TransactionError(te) => te,
        ClientErrorKind::RpcError(RpcError::RpcResponseError { data, .. }) => match data {
            RpcResponseErrorData::SendTransactionPreflightFailure(sim) => sim
                .err
                .expect("preflight failure without a TransactionError"),
            other => panic!("unexpected RPC response error data: {other:?}"),
        },
        other => panic!("unexpected (non-program) client error: {other:?}"),
    }
}

/// Extract the `ProgramError::Custom(n)` code from a `TransactionError`, if the
/// failing instruction was a custom program error.
pub fn custom_code(err: &TransactionError) -> Option<u32> {
    match err {
        TransactionError::InstructionError(_, InstructionError::Custom(code)) => Some(*code),
        _ => None,
    }
}

/// Assert a tx result failed with a specific `ProgramError::Custom(expected)`.
pub fn assert_custom_error(
    res: &Result<solana_sdk::signature::Signature, TransactionError>,
    expected: u32,
    context: &str,
) {
    match res {
        Ok(sig) => panic!(
            "[{context}] expected Custom({expected}) but tx SUCCEEDED (sig {sig}) — invariant NOT enforced"
        ),
        Err(e) => match custom_code(e) {
            Some(code) if code == expected => { /* correct error */ }
            Some(code) => panic!(
                "[{context}] expected Custom({expected}) but got Custom({code}) — wrong invariant fired"
            ),
            None => panic!("[{context}] expected Custom({expected}) but got non-custom error: {e:?}"),
        },
    }
}

// ---------------------------------------------------------------------------
// PDA + instruction builders (wire format per NOTES.md)
// ---------------------------------------------------------------------------

/// Derive the Transfer PDA: seeds `["transfer", sender(32), recipient(32), seed_le(8)]`.
pub fn transfer_pda(sender: &Pubkey, recipient: &Pubkey, seed: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            SEED_PREFIX,
            sender.as_ref(),
            recipient.as_ref(),
            &seed.to_le_bytes(),
        ],
        &program_id(),
    )
}

/// tag 0: CreateTransfer. data = [0][amount u64 LE][claim_window i64 LE][seed u64 LE] (25B).
/// Accounts: [sender(signer,w), transfer_pda(w), recipient, system_program].
pub fn ix_create(
    sender: &Pubkey,
    recipient: &Pubkey,
    seed: u64,
    amount: u64,
    claim_window: i64,
) -> Instruction {
    let (pda, _bump) = transfer_pda(sender, recipient, seed);
    let mut data = Vec::with_capacity(25);
    data.push(0u8);
    data.extend_from_slice(&amount.to_le_bytes());
    data.extend_from_slice(&claim_window.to_le_bytes());
    data.extend_from_slice(&seed.to_le_bytes());
    Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(*sender, true),
            AccountMeta::new(pda, false),
            AccountMeta::new_readonly(*recipient, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data,
    }
}

/// tag 1: CancelTransfer. Accounts: [sender(signer,w), transfer_pda(w)].
/// `pda_override` lets a test pass a forged/wrong PDA; `None` derives the real one.
pub fn ix_cancel(sender: &Pubkey, recipient: &Pubkey, seed: u64, pda_override: Option<Pubkey>) -> Instruction {
    let pda = pda_override.unwrap_or_else(|| transfer_pda(sender, recipient, seed).0);
    Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(*sender, true),
            AccountMeta::new(pda, false),
        ],
        data: vec![1u8],
    }
}

/// tag 2: AcceptTransfer. Accounts: [recipient(signer,w), transfer_pda(w)].
pub fn ix_accept(
    recipient: &Pubkey,
    sender: &Pubkey,
    seed: u64,
    pda_override: Option<Pubkey>,
) -> Instruction {
    let pda = pda_override.unwrap_or_else(|| transfer_pda(sender, recipient, seed).0);
    Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(*recipient, true),
            AccountMeta::new(pda, false),
        ],
        data: vec![2u8],
    }
}

/// tag 3: RefundExpired. Accounts: [cranker(signer), transfer_pda(w), sender(w)].
/// `sender_account` is the account the refund is paid to (tests can pass a wrong one).
pub fn ix_refund(
    cranker: &Pubkey,
    sender: &Pubkey,
    recipient: &Pubkey,
    seed: u64,
    sender_account: Option<Pubkey>,
    pda_override: Option<Pubkey>,
) -> Instruction {
    let pda = pda_override.unwrap_or_else(|| transfer_pda(sender, recipient, seed).0);
    let pay_to = sender_account.unwrap_or(*sender);
    Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new_readonly(*cranker, true),
            AccountMeta::new(pda, false),
            AccountMeta::new(pay_to, false),
        ],
        data: vec![3u8],
    }
}

// ---------------------------------------------------------------------------
// State inspection
// ---------------------------------------------------------------------------

/// Return true if the Transfer PDA still exists on-chain as a live Pending vault:
/// owned by the program, 98 bytes, state byte == 0 (Pending), lamports > 0.
pub fn pda_is_live_pending(client: &RpcClient, pda: &Pubkey) -> bool {
    match client.get_account(pda) {
        Ok(acc) => {
            acc.owner == program_id()
                && acc.data.len() == TRANSFER_LEN
                && acc.lamports > 0
                && acc.data.get(97) == Some(&0u8)
        }
        Err(_) => false,
    }
}

/// Return true if the Transfer account is closed (gone, or zero-lamport / zeroed).
/// The program's `close_and_pay` sweeps lamports to 0 and zeroes the data.
pub fn pda_is_closed(client: &RpcClient, pda: &Pubkey) -> bool {
    match client.get_account(pda) {
        // Account fully garbage-collected (0 lamports => purged) => closed.
        Err(_) => true,
        Ok(acc) => acc.lamports == 0 || acc.data.iter().all(|&b| b == 0),
    }
}

/// The on-chain rent-exempt minimum for a Transfer account.
pub fn transfer_rent(client: &RpcClient) -> u64 {
    client
        .get_minimum_balance_for_rent_exemption(TRANSFER_LEN)
        .expect("rent query failed")
}

/// Read `(created_at, claim_window)` from a live Transfer PDA (offsets 72 and 80,
/// i64 LE — see the account layout in NOTES.md / state.rs).
pub fn transfer_times(client: &RpcClient, pda: &Pubkey) -> (i64, i64) {
    let acc = client.get_account(pda).expect("transfer PDA not found");
    assert_eq!(acc.data.len(), TRANSFER_LEN, "unexpected Transfer account size");
    let created_at = i64::from_le_bytes(acc.data[72..80].try_into().unwrap());
    let claim_window = i64::from_le_bytes(acc.data[80..88].try_into().unwrap());
    (created_at, claim_window)
}

/// The validator's current on-chain unix time (block time of the latest slot).
fn current_chain_time(client: &RpcClient) -> Option<i64> {
    let slot = client.get_slot().ok()?;
    client.get_block_time(slot).ok()
}

/// Block until the on-chain `Clock` has provably passed `expiry` (unix seconds),
/// then a little beyond, so a subsequent terminal action deterministically sees
/// `now >= expiry`. This replaces a fixed wall-clock sleep (which raced the 10s
/// window by only ~2s and flaked): we read the ACTUAL on-chain `created_at`, so
/// client/confirmation latency can no longer eat the margin.
pub fn wait_until_past_expiry(client: &RpcClient, pda: &Pubkey) {
    let (created_at, claim_window) = transfer_times(client, pda);
    let expiry = created_at + claim_window;
    // Require the chain clock to be at least 2s beyond expiry for a safe margin.
    let target = expiry + 2;
    let deadline = Instant::now() + Duration::from_secs((claim_window as u64) + 60);
    loop {
        if let Some(now) = current_chain_time(client) {
            if now >= target {
                return;
            }
        }
        if Instant::now() > deadline {
            panic!(
                "on-chain clock never passed expiry (created_at={created_at}, window={claim_window})"
            );
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Block until the on-chain `Clock` is still safely BEFORE `expiry`, i.e. confirm
/// we are inside the window (used by the "refund before window" negative test so
/// it never races the boundary from the other side). Returns immediately if there
/// is comfortable margin; panics if we are already too close to expiry to trust.
pub fn assert_inside_window(client: &RpcClient, pda: &Pubkey, min_remaining_secs: i64) {
    let (created_at, claim_window) = transfer_times(client, pda);
    let expiry = created_at + claim_window;
    if let Some(now) = current_chain_time(client) {
        assert!(
            now + min_remaining_secs < expiry,
            "expected to be well inside the window (now={now}, expiry={expiry}) — test would race"
        );
    }
}
