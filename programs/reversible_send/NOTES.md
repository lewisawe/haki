# reversible_send — Phase 1 build notes (native Solana program)

Native (`solana-program`) implementation of the Reversible Send program. This is the
non-Anchor build decided in `.kiro/specs/reversible-send/requirements.md` (Toolchain
status). Same account model, same four instructions, same FR5 invariants as the Anchor
design in `design.md`.

## Module layout

```
programs/reversible_send/
├── Cargo.toml          # crate-type = ["cdylib","lib"]; dep: solana-program =1.18.26
└── src/
    ├── lib.rs          # entrypoint! + declare_id!, re-exports modules
    ├── errors.rs       # ReversibleSendError -> ProgramError::Custom(u32)
    ├── state.rs        # Transfer account, TransferState enum, manual (LE) serialization
    ├── instruction.rs  # ReversibleSendInstruction, 1-byte tag + LE arg decoding
    └── processor.rs     # process_instruction + the four handlers (invariant enforcement)
```

## Program ID

`EpwZhUpnRGfDf7mmST8DYXHyEVAJnT6rEgpiKdaQvNRh`
(keypair: `target/deploy/reversible_send-keypair.json`; declared via `declare_id!` in `lib.rs`.)

## PDA seed scheme

```
seeds = ["transfer", sender_pubkey (32B), recipient_pubkey (32B), seed_u64.to_le_bytes() (8B)]
```

- `SEED_PREFIX = b"transfer"` (see `state::Transfer::SEED_PREFIX`).
- `seed` is a client-supplied `u64` nonce so one sender can have many concurrent
  transfers to the same recipient.
- The canonical bump is found with `find_program_address` and stored in the account;
  the PDA is re-derived and checked on every instruction.

## The Transfer account (vault)

Fixed size `Transfer::LEN = 98` bytes, little-endian manual layout:

| offset | size | field        | type   |
|--------|------|--------------|--------|
| 0      | 32   | sender       | Pubkey |
| 32     | 32   | recipient    | Pubkey |
| 64     | 8    | amount       | u64    |
| 72     | 8    | created_at   | i64    |
| 80     | 8    | claim_window | i64    |
| 88     | 8    | seed         | u64    |
| 96     | 1    | bump         | u8     |
| 97     | 1    | state        | u8 (0=Pending,1=Cancelled,2=Released) |

The PDA itself holds the locked lamports (it is the vault — no second account). On any
terminal action the lamports are swept to the correct party and the data is zeroed +
lamports set to 0 (account closed), so no instruction can run twice (replay-proof).

## Instruction tag bytes

Wire format: `instruction_data[0]` = tag; following bytes = LE-packed args.

| tag | instruction      | args (after tag)                              |
|-----|------------------|-----------------------------------------------|
| 0   | CreateTransfer   | amount:u64 \| claim_window:i64 \| seed:u64 (24B LE) |
| 1   | CancelTransfer   | (none)                                        |
| 2   | AcceptTransfer   | (none)                                        |
| 3   | RefundExpired    | (none)                                        |

### Account orders
- **CreateTransfer**: `[sender(signer,w), transfer_pda(w), recipient, system_program]`
- **CancelTransfer**: `[sender(signer,w), transfer_pda(w)]`
- **AcceptTransfer**: `[recipient(signer,w), transfer_pda(w)]`
- **RefundExpired**:  `[cranker(signer), transfer_pda(w), sender(w)]`

## FR5 invariants enforced (in `processor.rs`)

- **Exactly one payout**: funds leave the PDA only in cancel (→sender), accept
  (→recipient), or refund (→sender); each closes the account.
- **No double-action / replay**: `close_and_pay` zeroes data + sets lamports to 0, so a
  closed account can never read back as Pending.
- **Signer/role checks**: cancel requires signer == stored sender (`NotSender`); accept
  requires signer == stored recipient (`NotRecipient`); refund is permissionless but pays
  the stored sender (account checked, `AccountMismatch` otherwise).
- **Time checks**: accept requires `now < created_at + claim_window` (`WindowExpired`);
  refund requires `now >= created_at + claim_window` (`WindowNotExpired`).
- **State check**: every terminal action requires `state == Pending` (`NotPending`).
- **Full amount only**: the whole PDA balance is swept (no partial drain). `amount` must
  exceed the rent-exempt minimum at create (`AmountTooSmall`).
- **Claim window bounds**: `claim_window` in `[10, 3600]` seconds (`InvalidClaimWindow`).
- PDA re-derived + verified on every instruction (`InvalidPda`); PDA must be owned by the
  program for terminal actions (`InvalidAccountOwner`).

## Error codes (`ProgramError::Custom(n)`)

0 InvalidInstruction · 1 NotSender · 2 NotRecipient · 3 NotPending · 4 WindowExpired ·
5 WindowNotExpired · 6 AmountTooSmall · 7 InvalidClaimWindow · 8 AccountMismatch ·
9 InvalidPda · 10 MissingRequiredSignature · 11 AlreadyInitialized · 12 ArithmeticOverflow ·
13 InvalidAccountOwner · 14 InvalidAccountData.

## Build

### Exact command that worked

```bash
export PATH="$HOME/.cache/solana/v1.58/platform-tools/rust/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
export CARGO_NET_GIT_FETCH_WITH_CLI=true CARGO_NET_RETRY=10 CARGO_HTTP_TIMEOUT=300 CARGO_HTTP_MULTIPLEXING=false
cd programs/reversible_send
cargo-build-sbf --no-rustup-override --skip-tools-install
```

Why the flags:
- This machine has **no rustup** (Rust is a Fedora system package). Default
  `cargo-build-sbf` tries to invoke `rustup` for a `+solana` toolchain override and fails
  immediately with `Failed to execute rustup`. `--no-rustup-override` disables that and
  uses the Solana `rustc` from `$PATH` instead — so the platform-tools `rust/bin` must be
  on `$PATH` (it provides the `sbf-solana-solana` target).
- `--skip-tools-install` because platform-tools are already extracted locally (see below).

### Result

- `.so`: `target/deploy/reversible_send.so` — **76,728 bytes** — build `Finished` in ~55s, exit 0.
- Two warnings only (`custom-heap` / `custom-panic` unexpected-cfg from solana-program
  1.18's `entrypoint!` macro) — harmless, no errors.
- Host `cargo check --lib` also passes (same cfg warnings only).

### Network-timeout / retry status

- The **platform-tools v1.58 toolchain download hit the flaky-network issue**: the cached
  tarball at `~/.cache/solana/v1.58/platform-tools/` was truncated to 271,946,052 bytes
  (`bzip2 -t` → "file ends unexpectedly") from a prior cut-off download.
- Fixed by re-downloading it directly with `curl -L -C - --retry 15 --retry-all-errors`
  (resumable, retrying). Full size **531,406,444 bytes**; `bzip2 -t` OK; extracted to get
  `rust/bin/rustc` with the `sbf-solana-solana` target. **1 clean curl run** completed it
  (resume/retry available but not needed once curl was driving it).
- The crate dependency tree (solana-program 1.18.26 + its transitive borsh/serde/etc.) was
  already in the cargo cache, so **no crate fetch timed out**; compilation ran offline.

## Dependencies beyond solana-program

**None.** The only direct dependency is `solana-program =1.18.26` (pinned to the
cache-present version to avoid a network fetch). `borsh` is available transitively via
solana-program but is not used — serialization is manual byte packing in `state.rs`, which
keeps the layout fixed/known and avoids a derive-macro dependency.

## Phase 2 status (tests + DevNet deploy) — DONE

See **`TESTS.md`** for the full write-up. Summary:

- **Tests:** 18/18 passing FR5-invariant suite. Harness = Rust integration tests
  driving a local `solana-test-validator` over RPC with cached `solana-client` +
  `solana-sdk` `=1.18.26` (compiles fully **offline** — `solana-program-test` /
  LiteSVM / Mollusk were all uncached and would need the flaky long fetch).
  Run: `cd tests-client && cargo test --offline --test invariants -- --test-threads=1`.
- **DevNet deploy:** live at `EpwZhUpnRGfDf7mmST8DYXHyEVAJnT6rEgpiKdaQvNRh`
  (matches `declare_id!`); deploy sig
  `3Ai3QbiWLCgSqLmc6E6x5xaj2t4b6jYdoZzj9udD3xRcZuFBgdx8BcBkXvKY2vBuzeyg6XtsA1RBtcgZ7Lw4wqvf` (Finalized).
- **Live proof (create → accept):** Explorer (DevNet):
  - create: https://explorer.solana.com/tx/3bBbZ6rpnveZZ3WjbyHoj1aE7CBJaZBAGNy6BGnVDGKKKY8c4ZeLAJDvCVvWRY3dci7okbPKX2ZQL1EyPKpti3Ax?cluster=devnet
  - accept: https://explorer.solana.com/tx/4xoNFgKWQk22HiX8nv4shGnhSoBmBfzTPJJJUxApEgtRhwyabvHwgYM6atoG8qpRtN4wRYKyw7UTmNDWvEC1jGyD?cluster=devnet

## Not done in this phase (by design)

- No frontend (Phase 3).
- No git repo / commit.
