"use client";

/**
 * SENDER panel — mobile-money-style phone keypad + amount + Send.
 * Resolves the typed number to a recipient pubkey via the demo registry (FR7)
 * and builds+sends create_transfer (tag 0) signed by the demo sender keypair.
 */
import { useMemo, useState } from "react";
import { LAMPORTS_PER_SOL } from "@solana/web3.js";
import { formatNumber, normalizeDigits, resolveNumber, REGISTRY } from "@/lib/registry";

interface Props {
  disabled: boolean;
  busy: boolean;
  onSend: (args: {
    recipientId: string;
    displayNumber: string;
    amountLamports: bigint;
  }) => void;
  claimWindowSecs: number;
  senderBalanceSol: number | null;
}

const KEYS = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "", "0", "⌫"];

export function SenderPanel({
  disabled,
  busy,
  onSend,
  claimWindowSecs,
  senderBalanceSol,
}: Props) {
  const [digits, setDigits] = useState("");
  const [amountSol, setAmountSol] = useState("0.02");

  const resolved = useMemo(() => resolveNumber(digits), [digits]);
  const national = normalizeDigits(digits);
  const amountNum = Number(amountSol);
  const amountValid = Number.isFinite(amountNum) && amountNum > 0;
  const amountLamports = amountValid
    ? BigInt(Math.round(amountNum * LAMPORTS_PER_SOL))
    : 0n;

  const canSend =
    !disabled && !busy && !!resolved && amountValid && national.length >= 9;

  function press(k: string) {
    if (k === "⌫") {
      setDigits((d) => d.slice(0, -1));
    } else if (k !== "") {
      setDigits((d) => (normalizeDigits(d).length >= 9 ? d : d + k));
    }
  }

  return (
    <section className="card p-7 flex flex-col gap-5" style={{ minHeight: 560 }}>
      <header className="flex items-center justify-between">
        <h2 className="display" style={{ fontSize: 24 }}>
          Send money
        </h2>
        <span className="mono" style={{ fontSize: 12, opacity: 0.7 }}>
          {senderBalanceSol === null
            ? "…"
            : `balance ${senderBalanceSol.toFixed(4)} SOL`}
        </span>
      </header>

      {/* number display */}
      <div>
        <label className="mono" style={{ fontSize: 11, opacity: 0.7 }}>
          RECIPIENT PHONE
        </label>
        <div
          className="field mono"
          style={{
            fontSize: 24,
            letterSpacing: 1,
            minHeight: 56,
            display: "flex",
            alignItems: "center",
          }}
        >
          {formatNumber(digits)}
        </div>
        <div className="mono" style={{ fontSize: 12, marginTop: 6, minHeight: 18 }}>
          {national.length === 0 ? (
            <span style={{ opacity: 0.5 }}>
              try {REGISTRY.map((r) => r.display).join(" · ")}
            </span>
          ) : resolved ? (
            <span style={{ color: "#bdff00" }}>
              ✓ {resolved.name} — {resolved.pubkey.slice(0, 4)}…
              {resolved.pubkey.slice(-4)}
            </span>
          ) : national.length >= 9 ? (
            <span style={{ opacity: 0.8 }}>
              unknown number (no mapped wallet) — wrong number, Send is blocked
            </span>
          ) : (
            <span style={{ opacity: 0.5 }}>keep typing…</span>
          )}
        </div>
      </div>

      {/* keypad */}
      <div className="grid grid-cols-3 gap-2">
        {KEYS.map((k, i) =>
          k === "" ? (
            <div key={i} />
          ) : (
            <button
              key={i}
              className="keypad-key"
              onClick={() => press(k)}
              disabled={disabled || busy}
              type="button"
            >
              {k}
            </button>
          ),
        )}
      </div>

      {/* amount */}
      <div>
        <label className="mono" style={{ fontSize: 11, opacity: 0.7 }}>
          AMOUNT (SOL)
        </label>
        <input
          className="field display"
          style={{ fontSize: 40, fontWeight: 700 }}
          value={amountSol}
          onChange={(e) => setAmountSol(e.target.value)}
          inputMode="decimal"
          disabled={disabled || busy}
        />
        <div className="mono" style={{ fontSize: 11, marginTop: 6, opacity: 0.6 }}>
          claim window {claimWindowSecs}s · recipient must accept before it
          expires, else it auto-refunds to you
        </div>
      </div>

      <button
        className="btn btn-primary mt-auto"
        onClick={() =>
          resolved &&
          onSend({
            recipientId: resolved.recipientId,
            displayNumber: resolved.display,
            amountLamports,
          })
        }
        disabled={!canSend}
        type="button"
      >
        {busy ? "Sending…" : "Send"}
      </button>
    </section>
  );
}
