/**
 * End-to-end verification driver — drives all THREE paths of the reversible-send
 * loop on DevNet using the EXACT SAME instruction builders the browser UI uses
 * (imported from ../src/lib/program.ts). This proves the UI's raw-instruction
 * encodings work against the live deployed program.
 *
 *   path A: create -> cancel     (sender gets funds back)
 *   path B: create -> accept     (recipient is paid)
 *   path C: create -> [expire] -> refund_expired  (sender gets funds back)
 *
 * Run:  node --experimental-strip-types scripts/verify-loop.ts
 * Prints a real Solana Explorer (DevNet) link for every signature.
 */
import { Connection, Keypair, LAMPORTS_PER_SOL } from "@solana/web3.js";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  explorerTx,
  fetchTransfer,
  ixAccept,
  ixCancel,
  ixCreate,
  ixRefund,
  randomSeed,
  transferPda,
} from "../src/lib/program.ts";
import { sendIx, getConnection } from "../src/lib/connection.ts";

const __dirname = dirname(fileURLToPath(import.meta.url));
const KP = JSON.parse(
  readFileSync(join(__dirname, "..", "src", "lib", "demo-keypairs.json"), "utf8"),
) as { sender: number[]; recipients: { id: string; label: string; secret: number[] }[] };

const sender = Keypair.fromSecretKey(Uint8Array.from(KP.sender));
const r = KP.recipients.map((x) => ({
  ...x,
  kp: Keypair.fromSecretKey(Uint8Array.from(x.secret)),
}));

const AMOUNT = BigInt(Math.round(0.008 * LAMPORTS_PER_SOL)); // small, rent-safe
const WINDOW_LONG = 120; // path A/B: comfortable window
const WINDOW_SHORT = 12; // path C: short so expiry is quick (>= 10s bound)

function url(sig: string) {
  return explorerTx(sig);
}

async function bal(conn: Connection, pk: import("@solana/web3.js").PublicKey) {
  return conn.getBalance(pk, "confirmed");
}

async function main() {
  const conn = getConnection();
  console.log("== verify-loop: all three paths on DevNet ==");
  console.log("sender :", sender.publicKey.toBase58());
  console.log("balance:", (await bal(conn, sender.publicKey)) / LAMPORTS_PER_SOL, "SOL\n");

  const results: Record<string, string> = {};

  // ── Path A: create -> cancel ────────────────────────────────────────────
  {
    const rec = r[0]; // Asha
    const seed = randomSeed();
    const [pda] = transferPda(sender.publicKey, rec.kp.publicKey, seed);
    console.log("── Path A: create -> cancel (recipient Asha) ──");
    const before = await bal(conn, sender.publicKey);
    const createSig = await sendIx(
      sender,
      ixCreate({
        sender: sender.publicKey,
        recipient: rec.kp.publicKey,
        seed,
        amountLamports: AMOUNT,
        claimWindowSecs: WINDOW_LONG,
      }),
    );
    console.log("  create:", url(createSig));
    results.A_create = createSig;
    const acc = await fetchTransfer(conn, pda);
    console.log("  pda state after create:", acc?.state, "lamports:", acc?.lamports);
    const cancelSig = await sendIx(
      sender,
      ixCancel({ sender: sender.publicKey, recipient: rec.kp.publicKey, seed }),
    );
    console.log("  cancel:", url(cancelSig));
    results.A_cancel = cancelSig;
    const closed = await fetchTransfer(conn, pda);
    const after = await bal(conn, sender.publicKey);
    console.log(
      "  pda after cancel:",
      closed === null ? "CLOSED" : closed.state,
      "| sender net:",
      (after - before) / LAMPORTS_PER_SOL,
      "SOL (≈ -fees only, principal + rent returned)\n",
    );
  }

  // ── Path B: create -> accept ────────────────────────────────────────────
  {
    const rec = r[1]; // Brian
    const seed = randomSeed();
    const [pda] = transferPda(sender.publicKey, rec.kp.publicKey, seed);
    console.log("── Path B: create -> accept (recipient Brian) ──");
    const rbBefore = await bal(conn, rec.kp.publicKey);
    const createSig = await sendIx(
      sender,
      ixCreate({
        sender: sender.publicKey,
        recipient: rec.kp.publicKey,
        seed,
        amountLamports: AMOUNT,
        claimWindowSecs: WINDOW_LONG,
      }),
    );
    console.log("  create:", url(createSig));
    results.B_create = createSig;
    const acceptSig = await sendIx(
      rec.kp,
      ixAccept({ sender: sender.publicKey, recipient: rec.kp.publicKey, seed }),
    );
    console.log("  accept:", url(acceptSig));
    results.B_accept = acceptSig;
    const closed = await fetchTransfer(conn, pda);
    const rbAfter = await bal(conn, rec.kp.publicKey);
    console.log(
      "  pda after accept:",
      closed === null ? "CLOSED" : closed.state,
      "| recipient net:",
      (rbAfter - rbBefore) / LAMPORTS_PER_SOL,
      "SOL (≈ +amount minus accept fee)\n",
    );
  }

  // ── Path C: create -> expire -> refund_expired ──────────────────────────
  {
    const rec = r[2]; // Caro
    const seed = randomSeed();
    const [pda] = transferPda(sender.publicKey, rec.kp.publicKey, seed);
    console.log("── Path C: create -> expire -> refund_expired (recipient Caro) ──");
    const before = await bal(conn, sender.publicKey);
    const createSig = await sendIx(
      sender,
      ixCreate({
        sender: sender.publicKey,
        recipient: rec.kp.publicKey,
        seed,
        amountLamports: AMOUNT,
        claimWindowSecs: WINDOW_SHORT,
      }),
    );
    console.log("  create (window " + WINDOW_SHORT + "s):", url(createSig));
    results.C_create = createSig;
    const acc = await fetchTransfer(conn, pda);
    const expiresAt = Number(acc!.expiresAt);
    console.log("  waiting for on-chain expiry (expiresAt=" + expiresAt + ")…");
    // Poll chain time until past expiry + margin.
    while (true) {
      const slot = await conn.getSlot("confirmed");
      const t = await conn.getBlockTime(slot);
      if (t !== null && t >= expiresAt + 2) break;
      await new Promise((r) => setTimeout(r, 1500));
    }
    const refundSig = await sendIx(
      sender,
      ixRefund({
        cranker: sender.publicKey,
        sender: sender.publicKey,
        recipient: rec.kp.publicKey,
        seed,
      }),
    );
    console.log("  refund:", url(refundSig));
    results.C_refund = refundSig;
    const closed = await fetchTransfer(conn, pda);
    const after = await bal(conn, sender.publicKey);
    console.log(
      "  pda after refund:",
      closed === null ? "CLOSED" : closed.state,
      "| sender net:",
      (after - before) / LAMPORTS_PER_SOL,
      "SOL (≈ -fees only, principal + rent returned)\n",
    );
  }

  console.log("== ALL THREE PATHS COMPLETE ==");
  for (const [k, v] of Object.entries(results)) {
    console.log(`${k.padEnd(10)} ${v}`);
    console.log(`${"".padEnd(10)} ${url(v)}`);
  }
}

main().catch((e) => {
  console.error("verify-loop FAILED:", e);
  process.exit(1);
});
