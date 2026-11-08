"use client";

/**
 * Explorer link row (FR6): every state change renders a Solana Explorer (DevNet)
 * link — the credibility anchor that proves the on-chain reality is not mocked.
 * Rendered as a JetBrains-Mono "tx readout" for terminal texture.
 */
import { explorerTx } from "@/lib/program";

export interface TxEvent {
  kind: "create" | "cancel" | "accept" | "refund";
  signature: string;
  at: number; // Date.now()
}

const LABELS: Record<TxEvent["kind"], string> = {
  create: "CREATE",
  cancel: "CANCEL → refund sender",
  accept: "ACCEPT → pay recipient",
  refund: "REFUND_EXPIRED → sender",
};

export function ExplorerLink({ ev }: { ev: TxEvent }) {
  const sigShort = `${ev.signature.slice(0, 8)}…${ev.signature.slice(-8)}`;
  return (
    <a
      href={explorerTx(ev.signature)}
      target="_blank"
      rel="noopener noreferrer"
      className="tx-link block py-2 border-t hairline"
      style={{ borderTopWidth: 1 }}
    >
      <span style={{ color: "#bdff00" }}>{LABELS[ev.kind]}</span>{" "}
      <span style={{ opacity: 0.85 }}>{sigShort}</span>{" "}
      <span style={{ opacity: 0.5 }}>↗ explorer</span>
    </a>
  );
}

export function ExplorerLog({ events }: { events: TxEvent[] }) {
  if (events.length === 0) {
    return (
      <p className="mono" style={{ fontSize: 12, opacity: 0.5 }}>
        No on-chain actions yet. Every create / cancel / accept / refund will
        appear here with a DevNet Explorer link.
      </p>
    );
  }
  return (
    <div>
      {events
        .slice()
        .reverse()
        .map((ev) => (
          <ExplorerLink key={ev.signature} ev={ev} />
        ))}
    </div>
  );
}
