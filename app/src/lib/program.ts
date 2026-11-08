/**
 * Raw-instruction client for the `reversible_send` native Solana program.
 *
 * There is NO Anchor IDL — the on-chain program (see
 * `programs/reversible_send/NOTES.md`) uses a 1-byte instruction tag followed by
 * little-endian packed args, and fixed account orders. These helpers reproduce
 * that wire format EXACTLY. They are the frontend mirror of the Rust test
 * harness instruction builders in
 * `programs/reversible_send/tests-client/src/lib.rs`.
 *
 * The frontend is NOT the enforcer (NFR1): every rule (signer, window, state,
 * single-payout) is checked on-chain. These helpers only *build* the transaction
 * the user signs; the program decides whether it is legal.
 */
import {
  Connection,
  PublicKey,
  SystemProgram,
  TransactionInstruction,
} from "@solana/web3.js";

/**
 * Program id — LIVE on DevNet, verified executable; declare_id! matches.
 * (programs/reversible_send/NOTES.md "Program ID".)
 */
export const PROGRAM_ID = new PublicKey(
  "EpwZhUpnRGfDf7mmST8DYXHyEVAJnT6rEgpiKdaQvNRh",
);

/** PDA seed prefix — `state::Transfer::SEED_PREFIX` = b"transfer". */
export const SEED_PREFIX = new TextEncoder().encode("transfer");

/** Transfer account byte length (NOTES.md account layout). */
export const TRANSFER_LEN = 98;

/** On-chain account-layout offsets (little-endian; NOTES.md). */
const OFF_SENDER = 0; // 32B Pubkey
const OFF_RECIPIENT = 32; // 32B Pubkey
const OFF_AMOUNT = 64; // 8B u64
const OFF_CREATED_AT = 72; // 8B i64
const OFF_CLAIM_WINDOW = 80; // 8B i64
const OFF_SEED = 88; // 8B u64
const OFF_BUMP = 96; // 1B u8
const OFF_STATE = 97; // 1B u8 (0=Pending,1=Cancelled,2=Released)

/** Instruction tags (NOTES.md "Instruction tag bytes"). */
export const TAG_CREATE = 0;
export const TAG_CANCEL = 1;
export const TAG_ACCEPT = 2;
export const TAG_REFUND = 3;

/** Program error codes -> human labels (NOTES.md "Error codes"). */
export const ERROR_CODES: Record<number, string> = {
  0: "InvalidInstruction",
  1: "NotSender",
  2: "NotRecipient",
  3: "NotPending",
  4: "WindowExpired",
  5: "WindowNotExpired",
  6: "AmountTooSmall",
  7: "InvalidClaimWindow",
  8: "AccountMismatch",
  9: "InvalidPda",
  10: "MissingRequiredSignature",
  11: "AlreadyInitialized",
  12: "ArithmeticOverflow",
  13: "InvalidAccountOwner",
  14: "InvalidAccountData",
};

/** Encode a bigint (or number) as 8 little-endian bytes (u64/i64 share LE bytes). */
export function u64le(value: bigint | number): Uint8Array {
  let v = BigInt(value);
  const out = new Uint8Array(8);
  for (let i = 0; i < 8; i++) {
    out[i] = Number(v & 0xffn);
    v >>= 8n;
  }
  return out;
}

/** Read 8 little-endian bytes as a bigint. */
function readU64le(buf: Uint8Array, offset: number): bigint {
  let v = 0n;
  for (let i = 7; i >= 0; i--) {
    v = (v << 8n) | BigInt(buf[offset + i]);
  }
  return v;
}

/** Read 8 little-endian bytes as a signed bigint (two's complement i64). */
function readI64le(buf: Uint8Array, offset: number): bigint {
  const u = readU64le(buf, offset);
  // interpret top bit as sign
  return u >= 1n << 63n ? u - (1n << 64n) : u;
}

/**
 * Derive the Transfer PDA. Seeds: ["transfer", sender(32B), recipient(32B),
 * seed_u64 LE(8B)] — matches `transfer_pda` in the test harness.
 */
export function transferPda(
  sender: PublicKey,
  recipient: PublicKey,
  seed: bigint | number,
): [PublicKey, number] {
  return PublicKey.findProgramAddressSync(
    [SEED_PREFIX, sender.toBytes(), recipient.toBytes(), u64le(seed)],
    PROGRAM_ID,
  );
}

/** A random u64 seed for a fresh transfer (fits JS safe range, LE-packed on chain). */
export function randomSeed(): bigint {
  // 48 bits of entropy is plenty to avoid collisions for a demo and stays well
  // within what crypto.getRandomValues gives us cheaply.
  const buf = new Uint8Array(6);
  crypto.getRandomValues(buf);
  let v = 0n;
  for (let i = 5; i >= 0; i--) v = (v << 8n) | BigInt(buf[i]);
  return v;
}

/**
 * tag 0: CreateTransfer.
 * data = [0][amount u64 LE][claim_window i64 LE][seed u64 LE]  (25 bytes).
 * accounts = [sender(signer,w), transfer_pda(w), recipient(r), system_program(r)].
 */
export function ixCreate(params: {
  sender: PublicKey;
  recipient: PublicKey;
  seed: bigint;
  amountLamports: bigint;
  claimWindowSecs: number;
}): TransactionInstruction {
  const { sender, recipient, seed, amountLamports, claimWindowSecs } = params;
  const [pda] = transferPda(sender, recipient, seed);
  const data = new Uint8Array(25);
  data[0] = TAG_CREATE;
  data.set(u64le(amountLamports), 1);
  data.set(u64le(BigInt(claimWindowSecs)), 9);
  data.set(u64le(seed), 17);
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      { pubkey: sender, isSigner: true, isWritable: true },
      { pubkey: pda, isSigner: false, isWritable: true },
      { pubkey: recipient, isSigner: false, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data: Buffer.from(data),
  });
}

/**
 * tag 1: CancelTransfer. data = [1]. accounts = [sender(signer,w), pda(w)].
 */
export function ixCancel(params: {
  sender: PublicKey;
  recipient: PublicKey;
  seed: bigint;
}): TransactionInstruction {
  const { sender, recipient, seed } = params;
  const [pda] = transferPda(sender, recipient, seed);
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      { pubkey: sender, isSigner: true, isWritable: true },
      { pubkey: pda, isSigner: false, isWritable: true },
    ],
    data: Buffer.from([TAG_CANCEL]),
  });
}

/**
 * tag 2: AcceptTransfer. data = [2]. accounts = [recipient(signer,w), pda(w)].
 */
export function ixAccept(params: {
  sender: PublicKey;
  recipient: PublicKey;
  seed: bigint;
}): TransactionInstruction {
  const { sender, recipient, seed } = params;
  const [pda] = transferPda(sender, recipient, seed);
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      { pubkey: recipient, isSigner: true, isWritable: true },
      { pubkey: pda, isSigner: false, isWritable: true },
    ],
    data: Buffer.from([TAG_ACCEPT]),
  });
}

/**
 * tag 3: RefundExpired (permissionless crank). data = [3].
 * accounts = [cranker(signer,r), pda(w), sender(w)].
 * The refund always pays the stored sender; `cranker` can be anyone.
 */
export function ixRefund(params: {
  cranker: PublicKey;
  sender: PublicKey;
  recipient: PublicKey;
  seed: bigint;
}): TransactionInstruction {
  const { cranker, sender, recipient, seed } = params;
  const [pda] = transferPda(sender, recipient, seed);
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      { pubkey: cranker, isSigner: true, isWritable: false },
      { pubkey: pda, isSigner: false, isWritable: true },
      { pubkey: sender, isSigner: false, isWritable: true },
    ],
    data: Buffer.from([TAG_REFUND]),
  });
}

export type TransferState = "pending" | "cancelled" | "released" | "closed";

export interface TransferAccount {
  sender: PublicKey;
  recipient: PublicKey;
  amount: bigint;
  createdAt: bigint; // unix seconds
  claimWindow: bigint; // seconds
  seed: bigint;
  bump: number;
  state: TransferState;
  lamports: number; // account balance (the vault)
  expiresAt: bigint; // createdAt + claimWindow
}

/**
 * Fetch + decode a Transfer PDA. Returns null if the account is closed/absent
 * (the program zeroes data + sets lamports to 0 on any terminal action).
 */
export async function fetchTransfer(
  connection: Connection,
  pda: PublicKey,
): Promise<TransferAccount | null> {
  const acc = await connection.getAccountInfo(pda, "confirmed");
  if (!acc) return null;
  if (acc.lamports === 0) return null;
  const data = acc.data;
  if (data.length < TRANSFER_LEN) return null;
  // all-zero data => closed vault that hasn't been purged yet
  if (data.every((b) => b === 0)) return null;

  const stateByte = data[OFF_STATE];
  const state: TransferState =
    stateByte === 0
      ? "pending"
      : stateByte === 1
        ? "cancelled"
        : stateByte === 2
          ? "released"
          : "closed";

  const createdAt = readI64le(data, OFF_CREATED_AT);
  const claimWindow = readI64le(data, OFF_CLAIM_WINDOW);

  return {
    sender: new PublicKey(data.slice(OFF_SENDER, OFF_SENDER + 32)),
    recipient: new PublicKey(data.slice(OFF_RECIPIENT, OFF_RECIPIENT + 32)),
    amount: readU64le(data, OFF_AMOUNT),
    createdAt,
    claimWindow,
    seed: readU64le(data, OFF_SEED),
    bump: data[OFF_BUMP],
    state,
    lamports: acc.lamports,
    expiresAt: createdAt + claimWindow,
  };
}

/** Build the Solana Explorer (DevNet) link for a signature (FR6). */
export function explorerTx(signature: string): string {
  return `https://explorer.solana.com/tx/${signature}?cluster=devnet`;
}

/** Build the Solana Explorer (DevNet) link for an address. */
export function explorerAddress(address: string): string {
  return `https://explorer.solana.com/address/${address}?cluster=devnet`;
}

/**
 * Pull a `ProgramError::Custom(n)` code + label out of a thrown send error so
 * the UI can show *which* on-chain invariant rejected the action (proving the
 * chain, not the UI, enforces the rules — NFR1).
 */
export function decodeProgramError(err: unknown): string | null {
  const text =
    err instanceof Error ? err.message : typeof err === "string" ? err : "";
  // web3.js surfaces custom program errors as "custom program error: 0xN"
  const hex = text.match(/custom program error:\s*0x([0-9a-fA-F]+)/);
  if (hex) {
    const code = parseInt(hex[1], 16);
    return ERROR_CODES[code] ?? `Custom(${code})`;
  }
  const dec = text.match(/Custom\((\d+)\)/);
  if (dec) {
    const code = parseInt(dec[1], 10);
    return ERROR_CODES[code] ?? `Custom(${code})`;
  }
  return null;
}
