"use client";

/**
 * Reversible Send — single-page, two-panel demo UI.
 *
 * Left: SENDER (keypad + amount + Send -> create_transfer).
 * Center: IN-FLIGHT hero (countdown ring + Undo/Cancel, then refund-on-expiry).
 * Right: RECIPIENT (Accept now -> accept_transfer).
 * Bottom: on-chain LEDGER with a Solana Explorer (DevNet) link per state change.
 *
 * One machine drives both panels via local demo keypairs (dev-mode keypair
 * switch). The frontend only BUILDS + SENDS transactions; every rule (signer,
 * window, state, single-payout) is enforced by the on-chain program (NFR1).
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { LAMPORTS_PER_SOL, PublicKey } from "@solana/web3.js";
import { SenderPanel } from "@/components/SenderPanel";
import {
  InFlightCard,
  InFlightIdle,
  type FlightStatus,
} from "@/components/InFlightCard";
import { RecipientPanel } from "@/components/RecipientPanel";
import { ExplorerLog, type TxEvent } from "@/components/ExplorerLink";
import { getConnection } from "@/lib/connection";
import { sendIx } from "@/lib/connection";
import {
  DEMO_SENDER,
  recipientById,
} from "@/lib/demo-keypairs";
import {
  decodeProgramError,
  explorerAddress,
  fetchTransfer,
  ixAccept,
  ixCancel,
  ixCreate,
  ixRefund,
  randomSeed,
  transferPda,
  type TransferAccount,
} from "@/lib/program";

/** Demo claim window — must stay within the on-chain [10, 3600]s bound. */
const CLAIM_WINDOW_SECS = 40;

interface Flight {
  seed: bigint;
  recipientId: string;
  recipientName: string;
  recipientPubkey: PublicKey;
  displayNumber: string;
  amountLamports: bigint;
  pda: PublicKey;
  /** on-chain expiry (unix secs) once the account has been read; null until then */
  expiresAtUnix: number | null;
}

export default function Home() {
  const [flight, setFlight] = useState<Flight | null>(null);
  const [onchain, setOnchain] = useState<TransferAccount | null>(null);
  const [status, setStatus] = useState<FlightStatus>("pending");
  const [events, setEvents] = useState<TxEvent[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [nowTick, setNowTick] = useState(() => Date.now());
  const [senderBal, setSenderBal] = useState<number | null>(null);
  const [recipientBal, setRecipientBal] = useState<number | null>(null);

  const keypairMissing = DEMO_SENDER.pubkey.length === 0;

  // 1Hz ticker for the countdown + balance refresh.
  useEffect(() => {
    const t = setInterval(() => setNowTick(Date.now()), 500);
    return () => clearInterval(t);
  }, []);

  // Sender balance (always), recipient balance (when a flight exists).
  const refreshBalances = useCallback(async () => {
    const conn = getConnection();
    try {
      const sb = await conn.getBalance(DEMO_SENDER.keypair.publicKey, "confirmed");
      setSenderBal(sb / LAMPORTS_PER_SOL);
    } catch {
      /* ignore transient rpc errors */
    }
    if (flight) {
      try {
        const rb = await conn.getBalance(flight.recipientPubkey, "confirmed");
        setRecipientBal(rb / LAMPORTS_PER_SOL);
      } catch {
        /* ignore */
      }
    }
  }, [flight]);

  useEffect(() => {
    refreshBalances();
    const t = setInterval(refreshBalances, 3000);
    return () => clearInterval(t);
  }, [refreshBalances]);

  // Poll the transfer PDA while a flight is live and not yet settled.
  useEffect(() => {
    if (!flight) return;
    if (status === "cancelled" || status === "accepted" || status === "refunded")
      return;
    let cancelled = false;
    const conn = getConnection();

    async function poll() {
      try {
        const acc = await fetchTransfer(conn, flight!.pda);
        if (cancelled) return;
        setOnchain(acc);
        if (acc && acc.state === "pending") {
          // lock in the on-chain expiry once we can read it
          setFlight((f) =>
            f && f.expiresAtUnix === null
              ? { ...f, expiresAtUnix: Number(acc.expiresAt) }
              : f,
          );
        }
      } catch {
        /* transient; keep polling */
      }
    }
    poll();
    const t = setInterval(poll, 1500);
    return () => {
      cancelled = true;
      clearInterval(t);
    };
  }, [flight, status]);

  // Derive remaining seconds from the on-chain expiry (chain is the source of
  // truth); fall back to a client estimate until the first read lands.
  const remaining = useMemo(() => {
    if (!flight) return CLAIM_WINDOW_SECS;
    const nowUnix = Math.floor(nowTick / 1000);
    if (flight.expiresAtUnix !== null) {
      return Math.max(0, flight.expiresAtUnix - nowUnix);
    }
    return CLAIM_WINDOW_SECS;
  }, [flight, nowTick]);

  // Transition pending -> expired when the countdown hits zero and it's still
  // pending on-chain. (Settlement transitions are set by the action handlers.)
  useEffect(() => {
    if (!flight) return;
    if (status !== "pending") return;
    if (remaining <= 0 && onchain && onchain.state === "pending") {
      setStatus("expired");
    }
  }, [remaining, status, onchain, flight]);

  function pushEvent(kind: TxEvent["kind"], signature: string) {
    setEvents((e) => [...e, { kind, signature, at: Date.now() }]);
  }

  // ── actions ───────────────────────────────────────────────────────────────

  const onSend = useCallback(
    async (args: {
      recipientId: string;
      displayNumber: string;
      amountLamports: bigint;
    }) => {
      setError(null);
      const party = recipientById(args.recipientId);
      if (!party) {
        setError("recipient not in demo registry");
        return;
      }
      const seed = randomSeed();
      const sender = DEMO_SENDER.keypair.publicKey;
      const recipient = party.keypair.publicKey;
      const [pda] = transferPda(sender, recipient, seed);
      setBusy(true);
      try {
        const sig = await sendIx(
          DEMO_SENDER.keypair,
          ixCreate({
            sender,
            recipient,
            seed,
            amountLamports: args.amountLamports,
            claimWindowSecs: CLAIM_WINDOW_SECS,
          }),
        );
        pushEvent("create", sig);
        setFlight({
          seed,
          recipientId: party.id,
          recipientName: party.label,
          recipientPubkey: recipient,
          displayNumber: args.displayNumber,
          amountLamports: args.amountLamports,
          pda,
          expiresAtUnix: null,
        });
        setOnchain(null);
        setStatus("pending");
      } catch (e) {
        setError(formatErr(e, "create"));
      } finally {
        setBusy(false);
        refreshBalances();
      }
    },
    [refreshBalances],
  );

  const onCancel = useCallback(async () => {
    if (!flight) return;
    setError(null);
    setBusy(true);
    try {
      const party = recipientById(flight.recipientId)!;
      const sig = await sendIx(
        DEMO_SENDER.keypair,
        ixCancel({
          sender: DEMO_SENDER.keypair.publicKey,
          recipient: party.keypair.publicKey,
          seed: flight.seed,
        }),
      );
      pushEvent("cancel", sig);
      setStatus("cancelled");
    } catch (e) {
      setError(formatErr(e, "cancel"));
    } finally {
      setBusy(false);
      refreshBalances();
    }
  }, [flight, refreshBalances]);

  const onAccept = useCallback(async () => {
    if (!flight) return;
    setError(null);
    setBusy(true);
    try {
      const party = recipientById(flight.recipientId)!;
      const sig = await sendIx(
        party.keypair,
        ixAccept({
          sender: DEMO_SENDER.keypair.publicKey,
          recipient: party.keypair.publicKey,
          seed: flight.seed,
        }),
      );
      pushEvent("accept", sig);
      setStatus("accepted");
    } catch (e) {
      setError(formatErr(e, "accept"));
    } finally {
      setBusy(false);
      refreshBalances();
    }
  }, [flight, refreshBalances]);

  const onRefund = useCallback(async () => {
    if (!flight) return;
    setError(null);
    setBusy(true);
    try {
      const party = recipientById(flight.recipientId)!;
      // Permissionless crank — the sender's client triggers it here, paying the
      // stored sender. (In production a keeper or either party can call it.)
      const sig = await sendIx(
        DEMO_SENDER.keypair,
        ixRefund({
          cranker: DEMO_SENDER.keypair.publicKey,
          sender: DEMO_SENDER.keypair.publicKey,
          recipient: party.keypair.publicKey,
          seed: flight.seed,
        }),
      );
      pushEvent("refund", sig);
      setStatus("refunded");
    } catch (e) {
      setError(formatErr(e, "refund"));
    } finally {
      setBusy(false);
      refreshBalances();
    }
  }, [flight, refreshBalances]);

  const onReset = useCallback(() => {
    setFlight(null);
    setOnchain(null);
    setStatus("pending");
    setError(null);
  }, []);

  // Recipient panel view: only show an incoming transfer while pending/expired.
  const incoming = useMemo(() => {
    if (!flight) return null;
    if (status === "cancelled" || status === "refunded") return null;
    const canAccept = status === "pending" && remaining > 0;
    return {
      recipientName: flight.recipientName,
      recipientPubkey: flight.recipientPubkey.toBase58(),
      recipientBalanceSol: recipientBal,
      amountLamports: flight.amountLamports,
      remaining,
      canAccept,
    };
  }, [flight, status, remaining, recipientBal]);

  return (
    <main className="min-h-screen px-6 py-8" style={{ maxWidth: 1340, margin: "0 auto" }}>
      {/* header */}
      <header className="mb-8 flex items-end justify-between flex-wrap gap-4">
        <div>
          <h1 className="display" style={{ fontSize: 44 }}>
            Reversible Send
          </h1>
          <p className="mono" style={{ fontSize: 13, opacity: 0.75, marginTop: 6 }}>
            on-chain <span style={{ color: "#bdff00" }}>hakikisha</span> · send to
            a phone number · cancel until it&apos;s claimed · auto-refund if never
            claimed · DevNet
          </p>
        </div>
        <div className="mono" style={{ fontSize: 12, opacity: 0.7, textAlign: "right" }}>
          <div>program</div>
          <a
            className="tx-link"
            href={explorerAddress("EpwZhUpnRGfDf7mmST8DYXHyEVAJnT6rEgpiKdaQvNRh")}
            target="_blank"
            rel="noopener noreferrer"
          >
            EpwZhUpnRGfDf7mmST8DYXHyEVAJnT6rEgpiKdaQvNRh ↗
          </a>
        </div>
      </header>

      {keypairMissing && (
        <div
          className="card p-4 mb-6"
          style={{ borderColor: "#bdff00" }}
        >
          <p className="mono" style={{ fontSize: 13 }}>
            demo keypairs not funded — run{" "}
            <span style={{ color: "#bdff00" }}>npm run fund</span> once to
            generate + fund the demo parties, then reload.
          </p>
        </div>
      )}

      {error && (
        <div className="card p-4 mb-6" style={{ borderColor: "#bdff00" }}>
          <p className="mono" style={{ fontSize: 13, color: "#bdff00" }}>
            {error}
          </p>
        </div>
      )}

      {/* three panels */}
      <div
        className="grid gap-6"
        style={{ gridTemplateColumns: "repeat(auto-fit, minmax(340px, 1fr))" }}
      >
        <SenderPanel
          disabled={!!flight && status === "pending"}
          busy={busy && !flight}
          onSend={onSend}
          claimWindowSecs={CLAIM_WINDOW_SECS}
          senderBalanceSol={senderBal}
        />

        {flight ? (
          <InFlightCard
            displayNumber={flight.displayNumber}
            amountLamports={flight.amountLamports}
            remaining={remaining}
            total={CLAIM_WINDOW_SECS}
            status={status}
            busy={busy}
            onCancel={onCancel}
            onRefund={onRefund}
            onReset={onReset}
          />
        ) : (
          <InFlightIdle />
        )}

        <RecipientPanel incoming={incoming} busy={busy} onAccept={onAccept} />
      </div>

      {/* on-chain ledger (Explorer links — FR6) */}
      <section className="card p-6 mt-6">
        <div className="flex items-center justify-between mb-2">
          <h2 className="display" style={{ fontSize: 20 }}>
            On-chain ledger
          </h2>
          <span className="mono" style={{ fontSize: 11, opacity: 0.6 }}>
            every state change · Solana Explorer (DevNet)
          </span>
        </div>
        <ExplorerLog events={events} />
      </section>

      <footer className="mono" style={{ fontSize: 11, opacity: 0.5, marginTop: 24 }}>
        The UI only builds + sends transactions. Signer / window / state /
        single-payout rules are enforced by the on-chain program, not here —
        inspect {" "}
        <a
          className="tx-link"
          href={explorerAddress("EpwZhUpnRGfDf7mmST8DYXHyEVAJnT6rEgpiKdaQvNRh")}
          target="_blank"
          rel="noopener noreferrer"
          style={{ display: "inline" }}
        >
          the program
        </a>
        .
      </footer>
    </main>
  );
}

function formatErr(e: unknown, action: string): string {
  const code = decodeProgramError(e);
  if (code) {
    return `${action} rejected on-chain: ${code}`;
  }
  const msg = e instanceof Error ? e.message : String(e);
  return `${action} failed: ${msg}`;
}
