# haki

**Send money you can take back — until the right person accepts it.** A self-custodial, reversible payment on Solana.

> Built for the Solana Hackathon (Nov 2026). Tracks: DeFi, Overall.

---

## The problem

Mobile money across East Africa has one brutal failure mode: **irreversibility**. Send to the wrong number — one mistyped digit — and the money is gone. Telcos generally cannot reverse a wrong-number transfer; funds spent or cleared against an overdraft are unrecoverable. A custodial ledger structurally can't undo a send once it's made.

## The idea

`haki` holds the money under a condition instead of sending it outright. You send to a phone number, but the funds are **locked by an on-chain Solana program** and only move when the right person accepts:

- **Cancel** before it's accepted → funds return to you.
- **Accept** (recipient only, before the window closes) → settles in ~1 second.
- **Auto-refund** if nobody claims it → the money comes back to the sender when the window expires.

The funds are never held by us or a telco. They sit in a program-owned account, and who can take them — when, and back to whom — is enforced by Solana, not by trust. This is something a custodial ledger cannot offer: **money that isn't final until it's accepted.**

## How it works

A native Solana program (Rust, no Anchor) with one `Transfer` account held in a PDA that *is* the vault, and four instructions:

| Instruction | Who | Effect |
|---|---|---|
| `create_transfer` | sender | Locks `amount` to a recipient for a claim window |
| `accept_transfer` | recipient only, before expiry | Releases funds to the recipient (the only path they receive) |
| `cancel_transfer` | sender only, before accept | Returns funds to the sender |
| `refund_expired` | anyone, after expiry | Returns funds to the sender (permissionless crank) |

Every safety rule is enforced **in the program, not the UI**: exactly one payout per transfer, no double-spend (the account closes on any terminal action), and signer/role/time/state checks on every instruction. See `programs/reversible_send/NOTES.md` for the account layout, PDA seeds, instruction encoding, and error codes.

## Live on DevNet

- **Program ID:** `EpwZhUpnRGfDf7mmST8DYXHyEVAJnT6rEgpiKdaQvNRh` ([Explorer](https://explorer.solana.com/address/EpwZhUpnRGfDf7mmST8DYXHyEVAJnT6rEgpiKdaQvNRh?cluster=devnet))
- Proven with real transactions (create → accept); signatures + Explorer links in `programs/reversible_send/TESTS.md`.
- **18/18 invariant tests pass**, each asserting the specific expected error — not just "a transaction failed."

## Repo layout

```
programs/reversible_send/   # native Solana program (Rust) — the product
  src/                      # lib, state, instruction, processor, errors
  tests-client/             # invariant test suite + DevNet proof
  NOTES.md                  # account model, PDA seeds, instruction encoding, build cmd
  TESTS.md                  # test summary + deployed program id + Explorer links
app/                        # Next.js UI (the demo skin)
  src/                      # two-panel sender/recipient, countdown + Undo, Explorer links
```

## Run it

### Web app (demo)
```bash
cd app
npm install
npm run fund     # generates + funds throwaway DevNet demo keypairs (needed on a fresh clone)
npm run dev      # prints the local URL
```
> `npm run fund` creates `src/lib/demo-keypairs.json` (gitignored — never committed). A template is at `src/lib/demo-keypairs.example.json`.

### Program (build / test)
```bash
cd programs/reversible_send
# requires the Solana CLI + platform-tools on PATH; this machine has no rustup, hence the flag:
cargo-build-sbf --no-rustup-override --skip-tools-install
# invariant tests:
cd tests-client && cargo test
```

## Honest scope

This is a weekend build. Deliberately:
- **DevNet only.** Demo keypairs and amounts are throwaway, clearly labeled. No mainnet, no real funds.
- **No real users yet.**
- **Not a fiat ramp.** `haki` is the on-chain settlement layer; cash-out to mobile money is designed, not built.
- **No-wallet recipients** (a claim-link flow so a recipient needs no wallet) is designed, not built — the honest next step.
- **No Anchor:** built native because Anchor's installer repeatedly failed on a flaky network; the native program has a tiny dependency tree and keeps the chain fully load-bearing.

## License

[add a license if you want one — MIT is a safe default for a hackathon]
