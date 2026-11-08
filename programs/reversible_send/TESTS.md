# reversible_send — Phase 2: correctness tests + DevNet proof

Phase 2 deliverable for the Reversible Send program: a full FR5-invariant test
suite (the DeFi correctness bar) plus a live DevNet deployment with a proven
happy-path transaction. Phase 1 (the program) is in `src/` and is already
reviewed APPROVED; this phase adds **tests only** — the program source was not
changed.

## 1. Test harness chosen — and why

**Harness: Rust integration tests driving a real local `solana-test-validator`
over RPC**, using the blocking `solana_client::rpc_client::RpcClient` +
`solana-sdk` (all pinned to `=1.18.26`).

Why not the "first choice" in-process SVMs:

| Candidate | Status on this machine | Verdict |
|-----------|------------------------|---------|
| `solana-program-test` (BanksClient) | **NOT cached**; pulls `solana-runtime` + `solana-banks-*` + `solana-accounts-db` — a large uncached tree | Rejected: needs the exact long multi-crate fetch this network drops (same failure that blocked Anchor — see `requirements.md` "Toolchain status") |
| LiteSVM | **NOT cached** | Rejected: uncached fetch |
| Mollusk | **NOT cached** | Rejected: uncached fetch |
| **`solana-client` + `solana-sdk` 1.18.26 → local `solana-test-validator`** | **Both crates already in the cargo cache**; validator binary installed (4.3.2) | **Chosen** |

**Did it need a network fetch? NO.** The harness compiles **fully offline**
(`cargo build --offline` / `cargo test --offline`) — every dependency resolved
from the local cargo cache. Verified: a clean build finished with zero network
access. This is also high-fidelity: tests run against a real validator with real
BPF execution, real `Clock`, and real rent, i.e. the same runtime as DevNet.

The harness code is `tests-client/src/lib.rs` (validator lifecycle, RPC helpers,
PDA derivation, instruction builders matching `NOTES.md` wire format exactly, and
error-code decoding). The tests are `tests-client/tests/invariants.rs`.

## 2. How to run the tests

```bash
export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"   # solana-test-validator
cd programs/reversible_send/tests-client
cargo test --offline --test invariants -- --test-threads=1
```

- `--offline` — all crates are cached; this avoids any network fetch.
- `--test-threads=1` — the window tests use wall-clock sleeps and each test boots
  its own validator, so the suite runs single-threaded (deterministic; avoids
  thrashing the machine with parallel validators).
- Each `#[test]` spawns its OWN `solana-test-validator` (fresh ledger under
  `/tmp/rs-test-ledger-<pid>-<port>`, OS-assigned free RPC/gossip ports) with the
  program preloaded via `--bpf-program`, and the validator is killed when the
  test's local `TestValidator` is dropped (on success AND on panic unwind), so no
  orphaned validator is left behind. It does NOT touch DevNet.
- The program `.so` must exist at `target/deploy/reversible_send.so` (Phase-1
  build; rebuild with the `cargo-build-sbf` command in `NOTES.md` if missing).
- Runtime ≈ 70s (18 validator boots; three window tests poll the on-chain clock
  until ~2s past the 10s claim window — deterministic, not a fixed sleep).

## 3. Pass summary

**18 tests, 18 passed, 0 failed** (`test result: ok. 18 passed; 0 failed`).
A negative test "passes" when the chain rejected the illegal action with the
*exact* `ProgramError::Custom(n)` for that invariant.

Error codes (from `errors.rs`): 1 NotSender · 2 NotRecipient · 3 NotPending ·
4 WindowExpired · 5 WindowNotExpired · 6 AmountTooSmall · 7 InvalidClaimWindow ·
8 AccountMismatch · 9 InvalidPda · 11 AlreadyInitialized · 13 InvalidAccountOwner.

### Happy paths (3)
| Test | Asserts |
|------|---------|
| `happy_create_then_cancel_refunds_sender_and_closes` | create → cancel: sender refunded ~in full, recipient stays 0, PDA closed |
| `happy_create_then_accept_before_expiry_pays_recipient_and_closes` | create → accept (before expiry): recipient paid full amount, PDA closed |
| `happy_create_then_refund_after_window_pays_sender_and_closes` | create → refund_expired (after 10s window): **SENDER** refunded (not cranker/recipient), recipient stays 0, PDA closed |

### Invariants — each fails with the exact error (11)
| Test | Expected error |
|------|----------------|
| `inv_cancel_by_non_sender_fails_not_sender` | `NotSender` (1) |
| `inv_accept_by_non_recipient_fails_not_recipient` | `NotRecipient` (2) |
| `inv_accept_after_window_fails_window_expired` | `WindowExpired` (4) |
| `inv_refund_before_window_fails_window_not_expired` | `WindowNotExpired` (5) |
| `inv_double_cancel_second_fails_closed` | terminal/closed → `InvalidAccountOwner` (13) or `NotPending` (3) |
| `inv_accept_after_cancel_fails_closed` | terminal/closed → `InvalidAccountOwner` (13) or `NotPending` (3) |
| `inv_create_amount_at_or_below_rent_fails_amount_too_small` | `AmountTooSmall` (6) — tested at `amount == rent` and `amount == rent-1` |
| `inv_create_claim_window_out_of_bounds_fails_invalid_claim_window` | `InvalidClaimWindow` (7) — tested at 9s, 3601s, and -5s |
| `inv_create_with_forged_pda_fails_invalid_pda` | `InvalidPda` (9) |
| `inv_terminal_action_on_forged_pda_fails_invalid_owner` | `InvalidAccountOwner` (13) |
| `inv_create_duplicate_fails_already_initialized` | `AlreadyInitialized` (11) |
| `inv_refund_to_wrong_sender_account_fails_account_mismatch` | `AccountMismatch` (8) |

> Note on "wrong/forged PDA": the program checks the PDA in two different places
> depending on the instruction. On **create** it re-derives and compares the PDA
> *before* touching the account, so a forged PDA address yields `InvalidPda` (9)
> — `inv_create_with_forged_pda_fails_invalid_pda`. On a **terminal** action
> (cancel/accept/refund) the program first loads the stored `Transfer` and so a
> forged/random account that the program does not own is rejected earlier as
> `InvalidAccountOwner` (13) — `inv_terminal_action_on_forged_pda_fails_invalid_owner`.
> Both paths are tested; both refuse to touch funds.

### Settlement invariants — exactly one payout, full amount, never both/neither (4, incl. the window test)
| Test | Asserts |
|------|---------|
| `settle_accept_pays_only_recipient_full_amount_not_sender` | exactly one payout: recipient gets the FULL amount, sender is NOT credited, vault emptied + closed |
| `settle_no_partial_drain_full_amount_moves` | the whole 0.5 SOL moves in one action; vault balance → 0 (no partial drain / no dust) |
| `settle_pending_vault_is_neither_paid_until_terminal_action` | while Pending, funds are held by the vault — neither party has them yet (the "never neither while the account exists" boundary) |
| (`happy_create_then_refund_after_window…` above) | the expiry-refund settlement path pays only the sender |

Coverage vs. the Phase-2 task checklist:
- HAPPY create→cancel / create→accept-before-expiry / create→refund-after-window ✅
- cancel-by-non-sender→NotSender, accept-by-non-recipient→NotRecipient,
  accept-after-expiry→WindowExpired, refund-before-window→WindowNotExpired ✅
- terminal action on terminal/closed transfer → NotPending / account-closed ✅
- amount ≤ rent → AmountTooSmall ✅
- claim_window outside [10,3600] → InvalidClaimWindow ✅
- wrong/forged PDA → InvalidPda (and InvalidAccountOwner on terminal) ✅
- settlement: exactly one payout, full amount, never both/neither ✅
- time manipulation for window boundaries — handled deterministically: the test
  reads the transfer's real on-chain `created_at`, then polls the validator's
  on-chain clock (`get_block_time`) until it is provably ≥ `created_at + window`
  before the terminal action (an earlier fixed 12s sleep raced the 10s window and
  flaked ~1 in 4 runs; the clock-poll is race-free — verified over many
  consecutive green runs) ✅

## 4. DevNet deploy

**Deployed successfully.**

- Command (run from `programs/reversible_send/`):
  ```bash
  solana program deploy \
    --program-id target/deploy/reversible_send-keypair.json \
    target/deploy/reversible_send.so
  ```
- **On-chain program ID: `EpwZhUpnRGfDf7mmST8DYXHyEVAJnT6rEgpiKdaQvNRh`** — matches
  the declared `declare_id!` exactly (deployed with the program keypair, so no
  realignment of `declare_id!` / the frontend is needed).
- Deploy signature: `3Ai3QbiWLCgSqLmc6E6x5xaj2t4b6jYdoZzj9udD3xRcZuFBgdx8BcBkXvKY2vBuzeyg6XtsA1RBtcgZ7Lw4wqvf` (Finalized).
- Upgrade authority: `2wb5vVPiXhU5Am4nsMT9ecb7vWedioDutqi5JqqXpEpe` (the funded wallet). Data length 76,728 bytes.
- `solana program show EpwZhUpnRGfDf7mmST8DYXHyEVAJnT6rEgpiKdaQvNRh` confirms it is live + executable.

## 5. Live happy-path proof on DevNet (demo assets)

One real end-to-end flow (`create_transfer` → `accept_transfer`) between two fresh
DevNet keypairs, driven by `tests-client/src/bin/devnet_proof.rs`:

```bash
export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"
cd programs/reversible_send/tests-client
cargo run --offline --bin devnet_proof
```

> The DevNet **faucet is rate-limited** on this network, so the driver funds the
> two demo parties from the already-funded wallet (`2wb5…pEpe`, ~2.1 SOL) via a
> plain system transfer instead of airdropping. The on-chain program flow itself
> is 100% real.

Result — recipient received the full 0.05 SOL (minus only its own accept fee),
vault PDA closed:

| Step | Signature | Solana Explorer (DevNet) |
|------|-----------|--------------------------|
| **create_transfer** | `3bBbZ6rpnveZZ3WjbyHoj1aE7CBJaZBAGNy6BGnVDGKKKY8c4ZeLAJDvCVvWRY3dci7okbPKX2ZQL1EyPKpti3Ax` | https://explorer.solana.com/tx/3bBbZ6rpnveZZ3WjbyHoj1aE7CBJaZBAGNy6BGnVDGKKKY8c4ZeLAJDvCVvWRY3dci7okbPKX2ZQL1EyPKpti3Ax?cluster=devnet |
| **accept_transfer** | `4xoNFgKWQk22HiX8nv4shGnhSoBmBfzTPJJJUxApEgtRhwyabvHwgYM6atoG8qpRtN4wRYKyw7UTmNDWvEC1jGyD` | https://explorer.solana.com/tx/4xoNFgKWQk22HiX8nv4shGnhSoBmBfzTPJJJUxApEgtRhwyabvHwgYM6atoG8qpRtN4wRYKyw7UTmNDWvEC1jGyD?cluster=devnet |

Independently confirmed with `solana confirm -v <sig>`:
- create tx log: `Program log: created transfer: 50000000 lamports, window 300s, seed 1794127449` — Status Ok.
- accept tx log: `Program log: accepted transfer: paid recipient in full` — Status Ok.

Demo context used: sender `BKe9nX5ymESBmE5c7h9ErWsZ7prTRdKdPTSBXbdC7tJc`,
recipient `783z66o3ECcc999aHgXgtbgepvhauHJ3dJh59LWc46LJ`, transfer PDA
`7Za3a2eqnwXwnFX35ByxMvezUPEoYoky59gxZDerALP3`, amount 50,000,000 lamports (0.05 SOL),
window 300s, seed 1794127449.

## 6. File locations

| Thing | Path |
|-------|------|
| Test harness lib | `programs/reversible_send/tests-client/src/lib.rs` |
| Invariant test suite | `programs/reversible_send/tests-client/tests/invariants.rs` |
| DevNet proof driver | `programs/reversible_send/tests-client/src/bin/devnet_proof.rs` |
| Test crate manifest | `programs/reversible_send/tests-client/Cargo.toml` |

The test crate is standalone (not in the program's build tree), so the on-chain
program keeps its single-dependency (`solana-program`) graph untouched.
