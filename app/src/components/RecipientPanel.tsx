"use client";

/**
 * RECIPIENT panel — shows the incoming transfer for the resolved recipient and
 * an ACCEPT NOW button (accept_transfer tag 2), signed by that recipient's demo
 * keypair (the dev-mode keypair switch lets one browser act as the recipient).
 */
import { LAMPORTS_PER_SOL } from "@solana/web3.js";

interface IncomingView {
  recipientName: string;
  recipientPubkey: string;
  recipientBalanceSol: number | null;
  amountLamports: bigint;
  remaining: number;
  canAccept: boolean; // pending + inside window
}

interface Props {
  incoming: IncomingView | null;
  busy: boolean;
  onAccept: () => void;
}

export function RecipientPanel({ incoming, busy, onAccept }: Props) {
  return (
    <section className="card p-7 flex flex-col gap-5" style={{ minHeight: 560 }}>
      <header className="flex items-center justify-between">
        <h2 className="display" style={{ fontSize: 24 }}>
          Recipient
        </h2>
        <span className="mono" style={{ fontSize: 12, opacity: 0.7 }}>
          {incoming?.recipientBalanceSol == null
            ? ""
            : `balance ${incoming.recipientBalanceSol.toFixed(4)} SOL`}
        </span>
      </header>

      {!incoming ? (
        <div className="flex flex-1 flex-col items-center justify-center text-center gap-3">
          <p className="mono" style={{ fontSize: 13, opacity: 0.6, maxWidth: 300 }}>
            No incoming transfer. When money is sent to this recipient&apos;s
            number, it appears here with an{" "}
            <span style={{ color: "#bdff00" }}>Accept now</span> button.
          </p>
        </div>
      ) : (
        <div className="flex flex-1 flex-col gap-5">
          <div>
            <span className="mono" style={{ fontSize: 11, opacity: 0.7 }}>
              INCOMING TO {incoming.recipientName.toUpperCase()}
            </span>
            <div className="display" style={{ fontSize: 56, lineHeight: 1 }}>
              {(Number(incoming.amountLamports) / LAMPORTS_PER_SOL).toFixed(3)}
              <span style={{ fontSize: 18, opacity: 0.7 }}> SOL</span>
            </div>
            <span className="mono" style={{ fontSize: 12, opacity: 0.7 }}>
              {incoming.recipientPubkey.slice(0, 6)}…
              {incoming.recipientPubkey.slice(-6)}
            </span>
          </div>

          <div
            className="card p-4 flex items-center justify-between"
            style={{ background: "rgba(0,0,0,0.12)" }}
          >
            <span className="mono" style={{ fontSize: 13 }}>
              {incoming.canAccept
                ? `${incoming.remaining}s left to claim`
                : "window closed"}
            </span>
            <span
              className="dot-active pulse"
              style={{ width: 10, height: 10, borderRadius: 9999 }}
            />
          </div>

          <button
            className="btn btn-primary mt-auto"
            style={{ fontSize: 20, padding: "16px 40px" }}
            onClick={onAccept}
            disabled={busy || !incoming.canAccept}
            type="button"
          >
            {busy
              ? "Accepting…"
              : incoming.canAccept
                ? "✓ Accept now"
                : "Window closed"}
          </button>
        </div>
      )}
    </section>
  );
}
