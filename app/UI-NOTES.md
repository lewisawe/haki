# Reversible Send — Phase 3 UI notes

A minimal, demo-focused web UI for the deployed native `reversible_send` Solana
program (Phase 1/2). It drives the full **send → cancel / accept → auto-refund**
loop live on DevNet, with a Solana Explorer link for every state change.

- **App directory:** `app/` (this folder), inside the workspace.
- **Program (LIVE, DevNet):** `EpwZhUpnRGfDf7mmST8DYXHyEVAJnT6rEgpiKdaQvNRh`
- **Stack:** Next.js 15 (App Router) + TypeScript + Tailwind v4 + `@solana/web3.js`
  (+ `@solana/wallet-adapter` installed). No Anchor — the frontend builds **raw
  instructions** matching `programs/reversible_send/NOTES.md` exactly.

## One-command run

```bash
cd app
npm install     # first time only (lean deps; retries configured for flaky network)
npm run dev     # starts Next.js dev server
```

Open the printed URL (e.g. `http://localhost:3000`, or the next free port such as
`3001`/`3005` if 3000 is taken — the terminal prints the actual one).

> The demo keypairs are already generated + funded and committed at
> `src/lib/demo-keypairs.json`, so the app boots ready-to-run. If balances run
> dry (or you want fresh parties), re-run `npm run fund` (see below).

### Production build (optional sanity)

```bash
cd app
npm run build   # compiles clean: route / ≈ 85 kB, no warnings
```

## How the two panels are driven on one machine (dev-mode keypair switch)

The brief allows "a dev-mode keypair switch so one machine can drive both
panels." That is what this app does, and it is the **default, reliable path**:

- The demo uses **local throwaway DevNet keypairs** (in `src/lib/demo-keypairs.json`),
  reconstructed client-side in `src/lib/demo-keypairs.ts`.
- The **SENDER panel** signs `create` / `cancel` / `refund` with the demo *sender*
  keypair. The **RECIPIENT panel** signs `accept` with the resolved *recipient*
  keypair. So a single browser tab acts as both sender and recipient — no wallet
  pop-ups, no second browser, which keeps the side-by-side demo legible.
- `@solana/wallet-adapter` is installed (per the brief) and can be layered on to
  let a real browser wallet act as the sender; the keypair-switch is the
  demo-practical default because a wallet pop-up can only sign for one connected
  wallet at a time, which makes the two-panel demo awkward on one machine.
- These keypairs are **DevNet-only, throwaway** — never real funds, never mainnet.

## Demo phone registry (FR7)

`src/lib/registry.ts` maps three demo numbers to the funded recipient keypairs:

| Phone number     | Recipient | Pubkey (DevNet)                                |
|------------------|-----------|------------------------------------------------|
| `+254712000001`  | Asha      | `6yoHPyi5Lz1TcfNYF3KbdRKwsKbGud6wocXCLc8zYPhn` |
| `+254712000002`  | Brian     | `DbWuCkT7nXihuMxqs3GLdC8J8YDMcbCVzTD3gzhSrAfh` |
| `+254712000003`  | Caro      | `J1oL2jkeDbtXoUkNB7Z4U2zQiuio5gA1ZDXVYXCorrtB` |

Demo **sender**: `641w84ZDBNzSZmmYHGu7tfKAzX2zAk9jkmFYp6o84eaM`.

Typing any **other** 9-digit number resolves to nothing — the "wrong number"
case — and **Send is blocked** (there is no mapped wallet). The no-wallet
claim-link (FR8) is a separate later phase and is intentionally NOT built here.

> The exact pubkeys above are whatever `npm run fund` last generated into
> `src/lib/demo-keypairs.json`. If you regenerate them, these addresses change
> (the registry re-reads the JSON automatically; the phone numbers stay the same).

## Driving each path manually in the browser

The keypad collects the 9 national digits after `+254`.

1. **create → cancel (wrong-number save):**
   - SENDER: key `712000001`, amount e.g. `0.02`, press **Send**.
   - The IN-FLIGHT hero card appears with a live countdown ring. Press
     **↩ Undo / Cancel** → funds return to sender, vault closes.
   - A `CREATE` and a `CANCEL` Explorer link appear in the **On-chain ledger**.

2. **create → accept (happy path):**
   - SENDER: key `712000002`, amount, **Send**.
   - RECIPIENT panel shows the incoming transfer → press **✓ Accept now** →
     recipient is paid, vault closes. `CREATE` + `ACCEPT` Explorer links appear.

3. **expiry → refund (auto-refund):**
   - SENDER: key `712000003`, amount, **Send**. Let the countdown run to zero
     **without** accepting. The hero flips to **EXPIRED / refund available**.
   - Press **⟲ Refund to sender** → funds return to sender, vault closes.
     `CREATE` + `REFUND_EXPIRED` Explorer links appear.
   - (Claim window is `40s` by default — see `CLAIM_WINDOW_SECS` in
     `src/app/page.tsx`; stays within the on-chain `[10, 3600]s` bound.)

The frontend only **builds + sends** transactions. Signer / window / state /
single-payout rules are enforced by the **on-chain program** (NFR1) — the UI
surfaces the program's `Custom(n)` error if you try something illegal.

## Re-funding / regenerating the demo keypairs

```bash
cd app
npm run fund            # generate fresh keypairs + fund from the CLI wallet, write the JSON
KEEP=1 npm run fund     # keep existing keypairs, only top up balances
```

Funding mirrors `programs/reversible_send/tests-client/src/bin/devnet_proof.rs`:
the DevNet **faucet is rate-limited**, so parties are funded from the already-
funded CLI wallet at `~/.config/solana/id.json`
(`2wb5vVPiXhU5Am4nsMT9ecb7vWedioDutqi5JqqXpEpe`) via a plain system transfer
(it tries the free faucet first, then falls back to the transfer). Defaults:
sender `0.05` SOL, each recipient `0.01` SOL (override with `FUND_SENDER_SOL` /
`FUND_RECIPIENT_SOL`). `RPC_URL` overrides the endpoint.

## Verifying the full loop without a browser

`npm run verify` (alias below) drives all three paths on DevNet using the EXACT
SAME instruction builders the browser imports (`src/lib/program.ts`), proving the
UI's encodings work against the live program:

```bash
cd app
node --experimental-strip-types scripts/verify-loop.ts
```

It prints a real Explorer link for every `create` / `cancel` / `accept` /
`refund` signature. (See "Proven DevNet runs" below for the links captured in
this build.)

## File map

| File | Purpose |
|------|---------|
| `src/lib/program.ts` | Raw-instruction client: PDA derivation, tags 0–3, LE encoders, account orders, Transfer decode, Explorer links, program-error decode — mirrors NOTES.md exactly. |
| `src/lib/connection.ts` | DevNet connection + `sendIx` (sign+send+confirm with a demo keypair). |
| `src/lib/demo-keypairs.ts` / `.json` | The demo sender + 3 recipient keypairs (dev-mode keypair switch). |
| `src/lib/registry.ts` | Phone-number → recipient pubkey demo registry (FR7). |
| `src/app/page.tsx` | The single page: orchestrates the flow + three panels + ledger. |
| `src/app/globals.css` | Assurestor design tokens (locked design system). |
| `src/components/SenderPanel.tsx` | Keypad + amount + Send (create_transfer). |
| `src/components/InFlightCard.tsx` | Hero: countdown ring + Undo/Cancel, then refund-on-expiry. |
| `src/components/CountdownRing.tsx` | The lime countdown-ring beat. |
| `src/components/RecipientPanel.tsx` | Incoming transfer + Accept now (accept_transfer). |
| `src/components/ExplorerLink.tsx` | The on-chain ledger of Explorer links (FR6). |
| `scripts/fund-demo-keypairs.ts` | Generate + fund demo keypairs. |
| `scripts/verify-loop.ts` | Drive all three paths on DevNet (encoding proof). |

## Design system — Assurestor (applied)

Dark "forest vault": bg Forest Canopy `#203400`, cards Vault Floor `#1b2d00`,
in-flight hero Canopy Mid `#335400`, the single accent Lime Phosphor `#bdff00`
(rationed to the countdown ring, the one primary action per viewport, active
dots), white text, hairline Moss `#586740` borders. Fonts: Space Grotesk
(display/UI) + JetBrains Mono (tx readouts), from Google Fonts. Pills (9999px)
for buttons, 32px radius cards, flat (no gradients/heavy shadows); the big amount
is the largest element.

## Proven DevNet runs (this build)

All three paths driven end-to-end on live DevNet (public RPC), each confirmed
`Status: Ok` with the program's own log message:

| Path | Action | Explorer (DevNet) | Program log |
|------|--------|-------------------|-------------|
| A | create | https://explorer.solana.com/tx/35NuTV5SP2x1f6MLjxaTCoGR2ZdtDQLHfxtC6q88x9BMnYyUN75BC9Q6b5wnRimypbjuGpwAYsHpRDQEE8AUGvjj?cluster=devnet | created transfer |
| A | **cancel** | https://explorer.solana.com/tx/4498VzfAyDVLPLtNz155u7cP9fjpo5RfKWP9r5NhBNiTu2XvHKLEJJQKz8UCu8sEFx88ttXVHZsUmq5X85qREtya?cluster=devnet | `cancelled transfer: refunded sender` |
| B | create | https://explorer.solana.com/tx/2A48oumbFH9T2Yoi7bnT9BRhoGMWtkFfTR31iAewrjMCyJRoiAj4HHqCQAwrZLYSy6ats9RF2mCB3qCiuVpPmhHe?cluster=devnet | created transfer |
| B | **accept** | https://explorer.solana.com/tx/2bttRTJQG4XV8YZM7gg4DwxWUAuURH1K7D3NiFWoryhqXcq9pNrSMcQSH3XbSxZMcwnUpPsHAGfpXPDscRaMJ8gu?cluster=devnet | `accepted transfer: paid recipient in full` |
| C | create (12s) | https://explorer.solana.com/tx/2SkGm5MPEm5VGeqtGvmBXxbvgbBxcug5yUwsAwkizoqLHbxw9sr9r7Mq31gajuwk2WBr1EsMoAe7gjn5jjWeZEUZ?cluster=devnet | created transfer |
| C | **refund_expired** | https://explorer.solana.com/tx/4qNCmc4pwynRicqB2uH6AYHpgAyjdJ6Brb9m72i98moUndYbYkbnawWyhanMbtuDQhwdaSeQu5WNnNVEusqPR7MA?cluster=devnet | `refunded expired transfer: paid sender in full` |

Observed settlement: cancel/refund returned principal + rent to the sender (net
≈ fee only); accept paid the recipient +0.008 SOL (minus its own accept fee);
the Transfer PDA was CLOSED after every terminal action.
