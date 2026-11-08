"use client";

/**
 * IN-FLIGHT card — the hero. After a send it shows "Sending to +2547…", the live
 * countdown ring (claim window), and the big UNDO / CANCEL button (cancel_transfer
 * tag 1). When the window hits zero with no accept, it surfaces the
 * "refund available" state + a refund crank button (refund_expired tag 3).
 *
 * This is the demo's emotional beat: wrong number -> Undo -> funds return.
 */
import { LAMPORTS_PER_SOL } from "@solana/web3.js";
import { CountdownRing } from "./CountdownRing";

export type FlightStatus =
  | "pending" // in window
  | "expired" // window elapsed, still pending -> refund available
  | "cancelled" // sender undid
  | "accepted" // recipient claimed
  | "refunded"; // expiry refund settled to sender

interface Props {
  displayNumber: string;
  amountLamports: bigint;
  remaining: number;
  total: number;
  status: FlightStatus;
  busy: boolean;
  onCancel: () => void;
  onRefund: () => void;
  onReset: () => void;
}

export function InFlightCard({
  displayNumber,
  amountLamports,
  remaining,
  total,
  status,
  busy,
  onCancel,
  onRefund,
  onReset,
}: Props) {
  const amountSol = Number(amountLamports) / LAMPORTS_PER_SOL;
  const settled =
    status === "cancelled" || status === "accepted" || status === "refunded";

  return (
    <section
      className="card-elevated p-8 flex flex-col items-center gap-6 text-center"
      style={{ minHeight: 560, justifyContent: "center" }}
    >
      <div className="flex flex-col items-center gap-1">
        <span className="mono" style={{ fontSize: 12, opacity: 0.8 }}>
          {status === "pending"
            ? "SENDING TO"
            : status === "expired"
              ? "UNCLAIMED — SENDING TO"
              : "TRANSFER TO"}
        </span>
        <span className="display" style={{ fontSize: 30 }}>
          {displayNumber}
        </span>
      </div>

      <div className="display" style={{ fontSize: 72, lineHeight: 1 }}>
        {amountSol.toFixed(amountSol < 1 ? 3 : 2)}
        <span style={{ fontSize: 22, opacity: 0.7 }}> SOL</span>
      </div>

      {!settled && (
        <CountdownRing remaining={remaining} total={total} size={220} />
      )}

      {/* status / action zone */}
      {status === "pending" && (
        <button
          className="btn btn-primary"
          style={{ fontSize: 22, padding: "18px 48px" }}
          onClick={onCancel}
          disabled={busy}
          type="button"
        >
          {busy ? "Undoing…" : "↩ Undo / Cancel"}
        </button>
      )}

      {status === "expired" && (
        <div className="flex flex-col items-center gap-3">
          <p className="mono" style={{ fontSize: 13, opacity: 0.85 }}>
            claim window elapsed with no accept — the safe default is the money
            returns home
          </p>
          <button
            className="btn btn-primary"
            style={{ fontSize: 20, padding: "16px 40px" }}
            onClick={onRefund}
            disabled={busy}
            type="button"
          >
            {busy ? "Refunding…" : "⟲ Refund to sender"}
          </button>
        </div>
      )}

      {settled && (
        <div className="flex flex-col items-center gap-4">
          <span
            className="display"
            style={{ fontSize: 28, color: "#bdff00" }}
          >
            {status === "cancelled" && "↩ Cancelled — funds returned"}
            {status === "accepted" && "✓ Accepted — recipient paid"}
            {status === "refunded" && "⟲ Refunded — funds returned to sender"}
          </span>
          <p className="mono" style={{ fontSize: 12, opacity: 0.7 }}>
            the on-chain program moved the funds + closed the vault. See the
            Explorer link in the ledger below.
          </p>
          <button className="btn btn-ghost" onClick={onReset} type="button">
            New send
          </button>
        </div>
      )}
    </section>
  );
}

/** Idle placeholder shown before any send. */
export function InFlightIdle() {
  return (
    <section
      className="card-elevated p-8 flex flex-col items-center justify-center text-center gap-4"
      style={{ minHeight: 560 }}
    >
      <div className="flex gap-2">
        <span className="dot-idle" style={dot} />
        <span className="dot-idle" style={dot} />
        <span className="dot-idle" style={dot} />
      </div>
      <h2 className="display" style={{ fontSize: 28, opacity: 0.9 }}>
        In-flight
      </h2>
      <p className="mono" style={{ fontSize: 13, opacity: 0.6, maxWidth: 320 }}>
        Send money from the left. The transfer appears here with a live countdown
        — you can Undo it until the recipient accepts. Unclaimed transfers
        auto-refund to you.
      </p>
    </section>
  );
}

const dot: React.CSSProperties = {
  width: 10,
  height: 10,
  borderRadius: 9999,
  display: "inline-block",
};
