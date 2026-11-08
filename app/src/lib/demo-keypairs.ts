/**
 * Demo keypairs for the two-panel, one-machine demo.
 *
 * WHY in-app keypairs (dev-mode keypair switch): the brief sanctions "a dev-mode
 * keypair switch so one machine can drive both panels." A real wallet-adapter
 * popup can only sign for ONE connected wallet at a time, which makes the
 * side-by-side sender/recipient demo awkward on a single machine. So the demo
 * drives both panels with local keypairs that sign transactions directly. These
 * are DEVNET-ONLY throwaway keypairs — never real funds, never mainnet.
 *
 * (A real wallet can still be connected via wallet-adapter as the SENDER — see
 * WalletContext + the "Use my wallet" affordance — but the default, reliable
 * one-machine path is these demo keypairs.)
 *
 * The secret keys live in `src/lib/demo-keypairs.json`, generated + funded by
 * `scripts/fund-demo-keypairs.ts` (run `npm run fund`). That file is committed
 * for the demo so the app boots ready-to-run; regenerate it any time with the
 * fund script. If it is missing, the app shows a "run npm run fund" hint.
 */
import { Keypair } from "@solana/web3.js";

// The generated secret keys (Solana CLI byte-array format: 64 numbers each).
// Imported from JSON so the demo boots with funded parties already in place.
import rawKeypairs from "./demo-keypairs.json";

export interface DemoParty {
  /** stable id used in the registry + UI */
  id: string;
  /** base58 pubkey (for display / registry) */
  pubkey: string;
  /** reconstructed signer */
  keypair: Keypair;
}

interface RawKeypairs {
  sender: number[];
  recipients: { id: string; label: string; secret: number[] }[];
}

const raw = rawKeypairs as unknown as RawKeypairs;

function fromSecret(secret: number[]): Keypair {
  return Keypair.fromSecretKey(Uint8Array.from(secret));
}

/** The single demo sender (the person sending money). */
export const DEMO_SENDER: DemoParty = {
  id: "sender",
  pubkey: fromSecret(raw.sender).publicKey.toBase58(),
  keypair: fromSecret(raw.sender),
};

/** The demo recipients (people a phone number can resolve to). */
export const DEMO_RECIPIENTS: Array<DemoParty & { label: string }> =
  raw.recipients.map((r) => {
    const kp = fromSecret(r.secret);
    return {
      id: r.id,
      label: r.label,
      pubkey: kp.publicKey.toBase58(),
      keypair: kp,
    };
  });

/** Look up a recipient demo party by its id. */
export function recipientById(id: string): (DemoParty & { label: string }) | undefined {
  return DEMO_RECIPIENTS.find((r) => r.id === id);
}
