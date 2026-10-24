import { STATUS_LABELS } from "../lib/format";
import type { DownloadStatus } from "../lib/types";

export function StatusBadge({ status }: { status: DownloadStatus }) {
  return (
    <span className={`status status-${status}`}>
      <span className="status-dot" aria-hidden />
      {STATUS_LABELS[status]}
    </span>
  );
}
