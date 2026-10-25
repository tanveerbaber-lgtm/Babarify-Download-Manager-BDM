import { Check, Clock, Minus, Pause, X, type LucideIcon } from "lucide-react";
import { STATUS_LABELS } from "../lib/format";
import type { DownloadStatus } from "../lib/types";

/** Icon inside the status chip's circle; downloading shows a pulsing dot instead. */
const ICONS: Partial<Record<DownloadStatus, LucideIcon>> = {
  completed: Check,
  paused: Pause,
  failed: X,
  queued: Clock,
  pending: Clock,
  cancelled: Minus,
};

/** Brandboard status chip: soft pill with a colored icon circle. */
export function StatusBadge({ status }: { status: DownloadStatus }) {
  const Icon = ICONS[status];
  return (
    <span className={`status status-${status}`}>
      <span className="status-icon" aria-hidden>
        {Icon ? <Icon size={10} strokeWidth={3.2} /> : <span className="status-pulse" />}
      </span>
      {STATUS_LABELS[status]}
    </span>
  );
}
