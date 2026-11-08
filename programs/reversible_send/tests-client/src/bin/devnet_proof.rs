//! DevNet happy-path proof driver.
//!
//! Runs ONE real end-to-end flow on Solana DevNet against the deployed program:
//!   1. fund a fresh `sender` and `recipient` (airdrop),
//!   2. `create_transfer` (sender locks funds into the PDA),
//!   3. `accept_transfer` (recipient claims — the only path funds reach them),
//! and prints the transaction signatures + Solana Explorer (devnet) URLs.
//!
//! Reuses the exact instruction builders from the test harness so the wire
//! format on DevNet is identical to what the invariant suite exercises.
//!
//! Run (from tests-client/):
//!   cargo run --offline --bin devnet_proof
//! Optionally override the RPC:  RPC_URL=https://api.devnet.solana.com

use std::time::{Duration, Instant};

use reversible_send_tests::*;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    native_token::LAMPORTS_PER_SOL,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
    transaction::Transaction,
};

fn explorer(sig: &solana_sdk::signature::Signature) -> String {
    format!("https://explorer.solana.com/tx/{sig}?cluster=devnet")
}

/// Load the funded CLI wallet (`~/.config/solana/id.json`) — the DevNet faucet is
/// rate-limited on this network, so we fund the demo parties from this already-
/// funded wallet (2wb5…pEpe) via a plain system transfer instead of airdropping.
fn load_funded_wallet() -> Keypair {
    let home = std::env::var("HOME").expect("HOME not set");
    let path = format!("{home}/.config/solana/id.json");
    solana_sdk::signature::read_keypair_file(&path)
        .unwrap_or_else(|e| panic!("could not read funded wallet at {path}: {e}"))
}

/// Fund `who` with `sol` SOL from `funder` via a confirmed system transfer.
/// Falls back to the faucet first (free) but does not depend on it.
fn fund(client: &RpcClient, funder: &Keypair, who: &Pubkey, sol: u64) {
    let want = sol * LAMPORTS_PER_SOL;
    if client.get_balance(who).unwrap_or(0) >= want {
        return;
    }
    // Try the free faucet once; ignore failure (it is usually rate-limited).
    if let Ok(sig) = client.request_airdrop(who, want) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if client.get_balance(who).unwrap_or(0) >= want {
                println!("  faucet funded {sol} SOL -> {who}");
                return;
            }
            let _ = client.confirm_transaction(&sig);
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    // Fund from the already-funded wallet (reliable).
    let blockhash = client.get_latest_blockhash().expect("blockhash");
    let ix = system_instruction::transfer(&funder.pubkey(), who, want);
    let mut tx = Transaction::new_with_payer(&[ix], Some(&funder.pubkey()));
    tx.sign(&[funder], blockhash);
    client
        .send_and_confirm_transaction(&tx)
        .unwrap_or_else(|e| panic!("transfer-fund of {who} failed: {e}"));
    println!("  wallet funded {sol} SOL -> {who}");
}

fn main() {
    let rpc = std::env::var("RPC_URL").unwrap_or_else(|_| "https://api.devnet.solana.com".to_string());
    println!("== Reversible Send — DevNet happy-path proof ==");
    println!("RPC: {rpc}");
    println!("Program: {PROGRAM_ID_STR}");

    let client = RpcClient::new_with_commitment(rpc, CommitmentConfig::confirmed());

    // Confirm the program is actually deployed before trying to use it.
    let prog = program_id();
    let acc = client
        .get_account(&prog)
        .expect("program account not found on devnet — deploy it first");
    assert!(acc.executable, "program account is not executable on devnet");
    println!("Program is live on devnet (executable, {} bytes data).", acc.data.len());

    // Fresh parties for a clean, verifiable demo.
    let funder = load_funded_wallet();
    println!("funder    = {} (funds the demo parties; faucet is rate-limited)", funder.pubkey());
    let sender = Keypair::new();
    let recipient = Keypair::new();
    println!("sender    = {}", sender.pubkey());
    println!("recipient = {}", recipient.pubkey());

    println!("\n[1/3] funding parties from the funded wallet…");
    fund(&client, &funder, &sender.pubkey(), 1);
    fund(&client, &funder, &recipient.pubkey(), 1);

    let seed: u64 = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs())
        & 0xffff_ffff;
    let amount = LAMPORTS_PER_SOL / 20; // 0.05 SOL
    let claim_window: i64 = 300; // 5 min — plenty of time to accept
    let (pda, _bump) = transfer_pda(&sender.pubkey(), &recipient.pubkey(), seed);
    println!("\ntransfer PDA = {pda}  (seed={seed}, amount={amount} lamports, window={claim_window}s)");

    println!("\n[2/3] create_transfer …");
    let create_sig = send_tx(
        &client,
        &sender,
        &[ix_create(&sender.pubkey(), &recipient.pubkey(), seed, amount, claim_window)],
        &[&sender],
    )
    .expect("create_transfer failed on devnet");
    assert!(pda_is_live_pending(&client, &pda), "PDA must be live Pending after create");
    println!("  create tx: {create_sig}");
    println!("  explorer : {}", explorer(&create_sig));
    println!("  vault now holds {} lamports", balance(&client, &pda));

    println!("\n[3/3] accept_transfer (recipient claims) …");
    let rcpt_before = balance(&client, &recipient.pubkey());
    let accept_sig = send_tx(
        &client,
        &recipient,
        &[ix_accept(&recipient.pubkey(), &sender.pubkey(), seed, None)],
        &[&recipient],
    )
    .expect("accept_transfer failed on devnet");
    assert!(pda_is_closed(&client, &pda), "PDA must be closed after accept");
    let rcpt_after = balance(&client, &recipient.pubkey());
    println!("  accept tx: {accept_sig}");
    println!("  explorer : {}", explorer(&accept_sig));
    println!(
        "  recipient balance: {rcpt_before} -> {rcpt_after} (+{} lamports, ≈ full amount minus fee)",
        rcpt_after.saturating_sub(rcpt_before)
    );

    println!("\n== PROOF COMPLETE ==");
    println!("CREATE_SIG={create_sig}");
    println!("CREATE_URL={}", explorer(&create_sig));
    println!("ACCEPT_SIG={accept_sig}");
    println!("ACCEPT_URL={}", explorer(&accept_sig));
}
