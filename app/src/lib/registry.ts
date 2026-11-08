/**
 * Phone-number -> recipient pubkey demo registry (FR7).
 *
 * v1 scope: a small hardcoded map of a few demo phone numbers to the DevNet demo
 * keypairs. Sending to an unmapped number is a known v1 limitation (the no-wallet
 * claim-link, FR8, is the designed answer and a SEPARATE later phase — not built
 * here).
 *
 * The numbers are Kenyan-format (+2547...) to evoke the M-Pesa UX. The digits a
 * user keys on the keypad (last 9, after the +254) resolve to a recipient.
 */
import { DEMO_RECIPIENTS } from "./demo-keypairs";

export interface RegistryEntry {
  /** full display number, e.g. "+254712345678" */
  display: string;
  /** the 9 national digits the keypad collects (after +254), e.g. "712345678" */
  digits: string;
  /** recipient demo-party id (into DEMO_RECIPIENTS) */
  recipientId: string;
  /** recipient base58 pubkey (resolved) */
  pubkey: string;
  /** a friendly name for the demo */
  name: string;
}

// Three demo numbers. The "wrong number" in the demo script is simply any number
// NOT in this table (resolves to nothing -> Send is blocked), OR number #3 used
// as the fat-finger target that the sender then Undoes.
const DEMO_NUMBERS: Array<{
  display: string;
  digits: string;
  recipientId: string;
  name: string;
}> = [
  { display: "+254712000001", digits: "712000001", recipientId: "r1", name: "Asha" },
  { display: "+254712000002", digits: "712000002", recipientId: "r2", name: "Brian" },
  { display: "+254712000003", digits: "712000003", recipientId: "r3", name: "Caro" },
];

export const REGISTRY: RegistryEntry[] = DEMO_NUMBERS.map((n) => {
  const party = DEMO_RECIPIENTS.find((r) => r.id === n.recipientId);
  if (!party) {
    throw new Error(
      `registry: demo recipient "${n.recipientId}" missing from demo-keypairs.json — run \`npm run fund\``,
    );
  }
  return {
    display: n.display,
    digits: n.digits,
    recipientId: n.recipientId,
    pubkey: party.pubkey,
    name: n.name,
  };
});

/**
 * Resolve the digits typed on the keypad to a registry entry.
 * Matches against the last 9 national digits (ignores a leading 0 or 254/+254).
 */
export function resolveNumber(rawDigits: string): RegistryEntry | null {
  const d = normalizeDigits(rawDigits);
  return REGISTRY.find((e) => e.digits === d) ?? null;
}

/** Strip +, 254 country code, and a leading 0 to get the 9 national digits. */
export function normalizeDigits(raw: string): string {
  let d = raw.replace(/[^0-9]/g, "");
  if (d.startsWith("254")) d = d.slice(3);
  if (d.startsWith("0")) d = d.slice(1);
  return d;
}

/** Format national digits for display as +254 7XX XXX XXX. */
export function formatNumber(rawDigits: string): string {
  const d = normalizeDigits(rawDigits);
  if (d.length === 0) return "+254 ";
  const p1 = d.slice(0, 3);
  const p2 = d.slice(3, 6);
  const p3 = d.slice(6, 9);
  return `+254 ${p1}${p2 ? " " + p2 : ""}${p3 ? " " + p3 : ""}`;
}
