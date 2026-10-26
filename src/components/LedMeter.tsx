import { useEffect, useState } from "react";
import { formatSpeed } from "../lib/format";

/** Round scale maximums in MB/s; a meter's scale is the first one above its peak. */
const SCALES = [1, 2, 5, 10, 20, 50, 100, 200, 500, 1000];
const MB = 1024 * 1024;
/** Babarify blue to green, the logo's gradient direction. */
const FROM = [0x25, 0x63, 0xeb];
const TO = [0x10, 0xb9, 0x81];

function colors(count: number): string[] {
  return Array.from({ length: count }, (_, i) => {
    const t = count > 1 ? i / (count - 1) : 0;
    return `rgb(${FROM.map((v, k) => Math.round(v + (TO[k] - v) * t)).join(",")})`;
  });
}
const ROW_COLORS = colors(12);
const PANEL_COLORS = colors(24);

/** "07.5" MB/s, or "737" KB/s below 1 MB/s, for the digital readout. */
function digits(bytesPerSec: number): { value: string; unit: string } {
  if (bytesPerSec < MB) return { value: String(Math.round(bytesPerSec / 1024)).padStart(3, "0"), unit: "KB/s" };
  return { value: (bytesPerSec / MB).toFixed(1).padStart(4, "0"), unit: "MB/s" };
}

/** The download's peak speed while this meter is shown, and the scale it implies. */
function usePeak(speed: number, running: boolean) {
  const [peak, setPeak] = useState(0);
  useEffect(() => {
    if (running && speed > peak) setPeak(speed);
  }, [speed, running, peak]);
  const scaleMb = SCALES.find((s) => s * MB >= Math.max(peak, speed) * 1.1) ?? SCALES[SCALES.length - 1];
  return { peak, scaleMb };
}

function Leds({ palette, speed, peak, scaleMb, running }: { palette: string[]; speed: number; peak: number; scaleMb: number; running: boolean }) {
  const n = palette.length;
  const lit = running ? Math.round(Math.min(1, speed / (scaleMb * MB)) * n) : 0;
  const peakLed = peak > 0 ? Math.min(n - 1, Math.max(0, Math.round((peak / (scaleMb * MB)) * n) - 1)) : -1;
  return (
    <span className="leds" style={{ gridTemplateColumns: `repeat(${n}, minmax(0, 1fr))` }} aria-hidden>
      {palette.map((color, i) => (
        <span key={i} className={i === peakLed && i >= lit ? "led peak" : "led"} style={i < lit ? { background: color } : undefined} />
      ))}
    </span>
  );
}

/**
 * A download's speed as Babarify LED bars (blue to green). `row` is the
 * compact meter in front of the speed in the list; `panel` is the navy
 * meter with a digital readout in the download window.
 */
export function LedMeter({ speed = 0, running, variant }: { speed?: number; running: boolean; variant: "row" | "panel" }) {
  const { peak, scaleMb } = usePeak(speed, running);
  const label = running ? formatSpeed(speed) : "—";

  if (variant === "row") {
    return (
      <span className="led-row" role="meter" aria-label="Speed" aria-valuemin={0} aria-valuemax={scaleMb} aria-valuenow={Number((speed / MB).toFixed(1))}>
        <Leds palette={ROW_COLORS} speed={speed} peak={peak} scaleMb={scaleMb} running={running} />
        <span className="led-row-value">{label}</span>
      </span>
    );
  }

  const now = digits(running ? speed : 0);
  const best = digits(peak);
  return (
    <div className="led-panel" role="meter" aria-label="Download speed" aria-valuemin={0} aria-valuemax={scaleMb} aria-valuenow={Number((speed / MB).toFixed(1))}>
      <div className="led-panel-top">
        <span className="led-panel-label">Speed</span>
        <span className={running ? "led-panel-value" : "led-panel-value idle"}>{now.value}</span>
        <span className="led-panel-unit">{now.unit}</span>
      </div>
      <Leds palette={PANEL_COLORS} speed={speed} peak={peak} scaleMb={scaleMb} running={running} />
      <div className="led-panel-scale">
        <span>0</span>
        <span>{peak > 0 ? `peak ${best.value.replace(/^0+(?=\d)/, "")} ${best.unit}` : running ? "" : "Not downloading"}</span>
        <span>{scaleMb} MB/s</span>
      </div>
    </div>
  );
}
