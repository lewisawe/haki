"use client";

/**
 * The signature beat: a lime-phosphor countdown ring on the dark in-flight card.
 * Shows the claim window draining. At zero it flips to a "refund available"
 * visual (the ring empties and pulses). SVG stroke-dashoffset animation, no
 * heavy deps.
 */
interface Props {
  /** seconds remaining (clamped >= 0) */
  remaining: number;
  /** total window seconds (for the ratio) */
  total: number;
  /** size in px */
  size?: number;
}

export function CountdownRing({ remaining, total, size = 220 }: Props) {
  const r = clampSecs(remaining);
  const ratio = total > 0 ? Math.max(0, Math.min(1, r / total)) : 0;
  const stroke = 12;
  const radius = (size - stroke) / 2;
  const circumference = 2 * Math.PI * radius;
  const dash = circumference * ratio;
  const expired = r <= 0;

  return (
    <div
      style={{ width: size, height: size }}
      className="relative flex items-center justify-center"
    >
      <svg width={size} height={size} className={expired ? "pulse" : ""}>
        {/* track */}
        <circle
          cx={size / 2}
          cy={size / 2}
          r={radius}
          fill="none"
          stroke="#586740"
          strokeWidth={stroke}
          opacity={0.5}
        />
        {/* progress (lime) — starts full, drains clockwise from 12 o'clock */}
        <circle
          cx={size / 2}
          cy={size / 2}
          r={radius}
          fill="none"
          stroke="#bdff00"
          strokeWidth={stroke}
          strokeLinecap="round"
          strokeDasharray={`${dash} ${circumference}`}
          transform={`rotate(-90 ${size / 2} ${size / 2})`}
          style={{ transition: "stroke-dasharray 0.9s linear" }}
        />
      </svg>
      <div className="absolute flex flex-col items-center justify-center text-center">
        {expired ? (
          <>
            <span
              className="display"
              style={{ fontSize: 28, color: "#bdff00" }}
            >
              EXPIRED
            </span>
            <span className="mono" style={{ fontSize: 12, opacity: 0.8 }}>
              refund available
            </span>
          </>
        ) : (
          <>
            <span className="display" style={{ fontSize: 64, color: "#ffffff" }}>
              {r}
            </span>
            <span className="mono" style={{ fontSize: 12, opacity: 0.8 }}>
              seconds to claim
            </span>
          </>
        )}
      </div>
    </div>
  );
}

function clampSecs(n: number): number {
  if (!Number.isFinite(n)) return 0;
  return Math.max(0, Math.ceil(n));
}
