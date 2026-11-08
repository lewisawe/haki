/**
 * Demo keypairs for the two-panel, one-machine demo.
 *
 * WHY in-app keypairs (dev-mode keypair switch): a real wallet-adapter popup can
 * only sign for ONE connected wallet at a time, which makes the side-by-side
 * sender/recipient demo awkward on a single machine. So the demo drives both
 * panels with local keypairs that sign transactions directly. These are
 * DEVNET-ONLY throwaway keypairs — never real funds, never mainnet.
 *
 * SECURITY / BUILD NOTE: the secret keys are NOT committed to git
 * (demo-keypairs.json is gitignored). They are loaded at runtime from one of:
 *   1. the NEXT_PUBLIC_DEMO_KEYPAIRS env var (base64 of the JSON) — used in
 *      hosted/CI builds (e.g. Vercel), so the build never needs a secret file;
 *   2. the local src/lib/demo-keypairs.json, generated + funded by
 *      `npm run fund` — the convenient local-dev path.
 * If neither is present the app renders a "run npm run fund" hint instead of
 * crashing the build. This keeps secrets out of the repo AND lets the
 * production build succeed without them.
 */
import { Keypair } from "@solana/web3.js";

export interface DemoParty {
  id: string;
  pubkey: string;
  keypair: Keypair;
}

interface RawKeypairs {
  placeholder?: boolean;
  sender: number[];
  recipients: { id: string; label: string; secret: number[] }[];
}

/**
 * Load the raw keypair material without a static import of a (gitignored) file,
 * so the production build never fails on a missing secret file.
 */
function loadRaw(): RawKeypairs {
  // 1. Hosted/CI and local real keys: base64-encoded JSON in an env var.
  //    (Set NEXT_PUBLIC_DEMO_KEYPAIRS to base64 of a funded demo-keypairs.json.)
  const b64 = process.env.NEXT_PUBLIC_DEMO_KEYPAIRS;
  if (b64 && b64.trim().length > 0) {
    try {
      const json =
        typeof atob === "function"
          ? atob(b64)
          : Buffer.from(b64, "base64").toString("utf8");
      return JSON.parse(json) as RawKeypairs;
    } catch {
      // fall through to the committed placeholder
    }
  }
  // 2. Fallback: the committed all-zero placeholder. This always resolves at
  //    build time (no gitignored file needed), so the production build never
  //    fails. Its keys are all zeros -> DEMO_KEYS_READY is false and the UI
  //    shows a setup hint. For a real local run, `npm run fund` prints the
  //    base64 to set as NEXT_PUBLIC_DEMO_KEYPAIRS (or set it in .env.local).
  // eslint-disable-next-line @typescript-eslint/no-var-requires
  return require("./demo-keypairs.default.json") as RawKeypairs;
}

/** Real keys loaded when the source is NOT the committed placeholder. */
function isReal(r: RawKeypairs): boolean {
  return r.placeholder !== true;
}

const raw = loadRaw();

/** True when real demo keypairs are available; UI shows a setup hint when false. */
export const DEMO_KEYS_READY = isReal(raw);

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
export const DEMO_RECIPIENTS: Array<DemoParty & { label: string }> = raw.recipients.map(
  (r) => {
    const kp = fromSecret(r.secret);
    return { id: r.id, label: r.label, pubkey: kp.publicKey.toBase58(), keypair: kp };
  },
);

/** Look up a recipient demo party by its id. */
export function recipientById(id: string): (DemoParty & { label: string }) | undefined {
  return DEMO_RECIPIENTS.find((r) => r.id === id);
}
