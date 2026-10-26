import { Check, Clock, Minus, Pause, ShieldAlert, ShieldCheck, X, type LucideIcon } from "lucide-react";
import { STATUS_LABELS } from "../lib/format";
import type { DownloadStatus, ScanResult } from "../lib/types";

/** Icon inside the status chip's circle; downloading shows a pulsing dot instead. */
const ICONS: Partial<Record<DownloadStatus, LucideIcon>> = {
  completed: Check,
  paused: Pause,
  failed: X,
  queued: Clock,
  pending: Clock,
  cancelled: Minus,
};

/** Brandboard status chip: soft pill with a colored icon circle. A finished
 * download also shows its virus scan: "Scanning…", a shield when clean, or
 * "Threat Found". */
export function StatusBadge({ status, scan = null }: { status: DownloadStatus; scan?: ScanResult | null }) {
  if (status === "completed" && scan === "threat") {
    return (
      <span className="status status-failed" title="Microsoft Defender found a threat in this file and dealt with it.">
        <span className="status-icon" aria-hidden>
          <ShieldAlert size={10} strokeWidth={3} />
        </span>
        Threat Found
      </span>
    );
  }
  if (status === "completed" && scan === "scanning") {
    return (
      <span className="status status-downloading" title="Microsoft Defender is checking this file.">
        <span className="status-icon" aria-hidden>
          <span className="status-pulse" />
        </span>
        Scanning…
      </span>
    );
  }
  const Icon = status === "completed" && scan === "clean" ? ShieldCheck : ICONS[status];
  return (
    <span className={`status status-${status}`} title={scan === "clean" ? "Scanned by Microsoft Defender: no threats found." : undefined}>
      <span className="status-icon" aria-hidden>
        {Icon ? <Icon size={10} strokeWidth={3.2} /> : <span className="status-pulse" />}
      </span>
      {STATUS_LABELS[status]}
    </span>
  );
}
