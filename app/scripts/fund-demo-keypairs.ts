/**
 * Generate + fund the demo keypairs for the two-panel demo, then write them to
 * src/lib/demo-keypairs.json so the UI boots with funded parties.
 *
 * Funding pattern mirrors programs/reversible_send/tests-client/src/bin/
 * devnet_proof.rs: the DevNet faucet is rate-limited on this network, so the
 * parties are funded from the already-funded CLI wallet (~/.config/solana/id.json,
 * the 2wb5…pEpe wallet) via a plain system transfer. It tries the free faucet
 * first and falls back to a transfer.
 *
 * Run:  npm run fund
 *   (optionally FUND_SENDER_SOL / FUND_RECIPIENT_SOL override the amounts;
 *    RPC_URL overrides the endpoint; KEEP=1 keeps existing keypairs and only tops
 *    them up instead of regenerating.)
 *
 * This is a Node script (run via `node --experimental-strip-types`), not part of
 * the Next build.
 */
import {
  Connection,
  Keypair,
  LAMPORTS_PER_SOL,
  PublicKey,
  SystemProgram,
  Transaction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { homedir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const RPC_URL = process.env.RPC_URL ?? "https://api.devnet.solana.com";
// Each demo party only needs enough for a couple of ~0.02 SOL transfers + fees +
// rent. Keep amounts small because the funder wallet balance is modest.
const SENDER_SOL = Number(process.env.FUND_SENDER_SOL ?? "0.05");
const RECIPIENT_SOL = Number(process.env.FUND_RECIPIENT_SOL ?? "0.01");
const KEEP = process.env.KEEP === "1";

const __dirname = dirname(fileURLToPath(import.meta.url));
const OUT_PATH = join(__dirname, "..", "src", "lib", "demo-keypairs.json");

function loadFunder(): Keypair {
  const path = join(homedir(), ".config", "solana", "id.json");
  if (!existsSync(path)) {
    throw new Error(
      `funder wallet not found at ${path} — fund a Solana CLI wallet on devnet first`,
    );
  }
  const secret = JSON.parse(readFileSync(path, "utf8")) as number[];
  return Keypair.fromSecretKey(Uint8Array.from(secret));
}

async function balance(conn: Connection, pk: PublicKey): Promise<number> {
  return conn.getBalance(pk, "confirmed");
}

async function fund(
  conn: Connection,
  funder: Keypair,
  who: PublicKey,
  sol: number,
): Promise<void> {
  const want = Math.round(sol * LAMPORTS_PER_SOL);
  const have = await balance(conn, who);
  if (have >= want) {
    console.log(`  already funded ${sol} SOL -> ${who.toBase58()} (have ${have})`);
    return;
  }
  const need = want - have;
  // Try the free faucet once; ignore failure (usually rate-limited).
  try {
    const sig = await conn.requestAirdrop(who, need);
    const bh = await conn.getLatestBlockhash();
    await conn.confirmTransaction({ signature: sig, ...bh }, "confirmed");
    console.log(`  faucet funded ${sol} SOL -> ${who.toBase58()}`);
    return;
  } catch {
    // fall through to transfer
  }
  const ix = SystemProgram.transfer({
    fromPubkey: funder.publicKey,
    toPubkey: who,
    lamports: need,
  });
  const tx = new Transaction().add(ix);
  const sig = await sendAndConfirmTransaction(conn, tx, [funder], {
    commitment: "confirmed",
  });
  console.log(`  wallet funded ${sol} SOL -> ${who.toBase58()} (sig ${sig})`);
}

interface RawKeypairs {
  sender: number[];
  recipients: { id: string; label: string; secret: number[] }[];
}

async function main() {
  const conn = new Connection(RPC_URL, "confirmed");
  const funder = loadFunder();
  const funderBal = await balance(conn, funder.publicKey);
  console.log("== fund demo keypairs ==");
  console.log(`RPC    : ${RPC_URL}`);
  console.log(
    `funder : ${funder.publicKey.toBase58()} (${(funderBal / LAMPORTS_PER_SOL).toFixed(4)} SOL)`,
  );

  let data: RawKeypairs;
  if (KEEP && existsSync(OUT_PATH)) {
    data = JSON.parse(readFileSync(OUT_PATH, "utf8")) as RawKeypairs;
    console.log("keeping existing keypairs (KEEP=1), topping up balances only");
  } else {
    const sender = Keypair.generate();
    const labels = [
      { id: "r1", label: "Asha" },
      { id: "r2", label: "Brian" },
      { id: "r3", label: "Caro" },
    ];
    data = {
      sender: Array.from(sender.secretKey),
      recipients: labels.map((l) => ({
        id: l.id,
        label: l.label,
        secret: Array.from(Keypair.generate().secretKey),
      })),
    };
    console.log("generated fresh keypairs");
  }

  const sender = Keypair.fromSecretKey(Uint8Array.from(data.sender));
  console.log(`\nsender    = ${sender.publicKey.toBase58()}`);
  for (const r of data.recipients) {
    const kp = Keypair.fromSecretKey(Uint8Array.from(r.secret));
    console.log(`recipient ${r.id} (${r.label}) = ${kp.publicKey.toBase58()}`);
  }

  console.log("\nfunding parties from the funder wallet…");
  await fund(conn, funder, sender.publicKey, SENDER_SOL);
  for (const r of data.recipients) {
    const kp = Keypair.fromSecretKey(Uint8Array.from(r.secret));
    await fund(conn, funder, kp.publicKey, RECIPIENT_SOL);
  }

  writeFileSync(OUT_PATH, JSON.stringify(data, null, 2) + "\n");
  console.log(`\nwrote ${OUT_PATH}`);
  console.log("== done ==");
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
