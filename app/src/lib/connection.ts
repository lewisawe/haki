/**
 * DevNet connection + a tiny transaction sender that signs with a local demo
 * keypair (the dev-mode keypair switch). Each panel calls this with its own
 * keypair as the fee-payer/signer, so one browser can drive sender + recipient.
 */
import {
  Connection,
  Keypair,
  Transaction,
  TransactionInstruction,
  sendAndConfirmTransaction,
} from "@solana/web3.js";

export const RPC_URL =
  process.env.NEXT_PUBLIC_RPC_URL ?? "https://api.devnet.solana.com";

let _conn: Connection | null = null;

/** Shared confirmed-commitment DevNet connection. */
export function getConnection(): Connection {
  if (!_conn) {
    _conn = new Connection(RPC_URL, "confirmed");
  }
  return _conn;
}

/**
 * Sign + send + confirm a single-instruction transaction with `signer` as fee
 * payer and the only signer. Returns the confirmed signature.
 *
 * Throws on program error — the caller decodes the custom error so the UI shows
 * which on-chain invariant rejected it (NFR1: the chain enforces, not the UI).
 */
export async function sendIx(
  signer: Keypair,
  ix: TransactionInstruction,
): Promise<string> {
  const conn = getConnection();
  const tx = new Transaction().add(ix);
  tx.feePayer = signer.publicKey;
  const sig = await sendAndConfirmTransaction(conn, tx, [signer], {
    commitment: "confirmed",
    skipPreflight: false,
  });
  return sig;
}
