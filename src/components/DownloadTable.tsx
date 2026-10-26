import { useRef, type KeyboardEvent, type MouseEvent } from "react";
import { ArrowDown, ArrowUp, FileArchive, FileAudio, FileImage, FileText, FileVideo, File, Gauge, type LucideIcon } from "lucide-react";
import { StatusBadge } from "./StatusBadge";
import { LedMeter } from "./LedMeter";
import { formatBytes, formatDate, formatEta, formatSpeed, progressPercent } from "../lib/format";
import type { Download, Progress, Queue } from "../lib/types";

const EXTENSION_ICONS: [RegExp, LucideIcon][] = [
  [/\.(zip|rar|7z|tar|gz|bz2|xz|iso)$/i, FileArchive],
  [/\.(mp4|mkv|webm|avi|mov|m4v)$/i, FileVideo],
  [/\.(mp3|m4a|flac|wav|ogg|opus|aac)$/i, FileAudio],
  [/\.(png|jpe?g|gif|webp|svg|bmp)$/i, FileImage],
  [/\.(pdf|txt|docx?|xlsx?|pptx?|md|csv)$/i, FileText],
];

function iconFor(fileName: string): LucideIcon {
  return EXTENSION_ICONS.find(([re]) => re.test(fileName))?.[1] ?? File;
}

function timeOf(ms: number): string {
  return new Date(ms).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

/** The second line under a file name. */
function subtitleFor(d: Download, live: Progress | undefined, queue: Queue | undefined): string {
  if (live?.stage) return live.stage;
  if (d.status === "queued" && queue && !queue.running) return `Waiting for the ${queue.name} queue`;
  if (d.status === "failed" && d.retryAt) return `${d.errorMessage ?? "Failed"} · Retrying at ${timeOf(d.retryAt)}`;
  return d.errorMessage ?? (d.status === "completed" ? d.savedPath : null) ?? hostOf(d.url);
}

function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

interface DownloadTableProps {
  downloads: Download[];
  progress: Record<string, Progress>;
  selected: Set<string>;
  onSelectionChange: (selected: Set<string>) => void;
  onDeleteRequest: () => void;
  /** Double-click or Enter on a row. */
  onActivate: (download: Download) => void;
  queues: Record<string, Queue>;
  onContextMenu: (x: number, y: number, download: Download) => void;
}

export function DownloadTable({ downloads, progress, selected, onSelectionChange, onDeleteRequest, onActivate, queues, onContextMenu }: DownloadTableProps) {
  const anchor = useRef<number | null>(null);

  function handleRowClick(e: MouseEvent, index: number, id: string) {
    if (e.shiftKey && anchor.current !== null) {
      const [from, to] = [anchor.current, index].sort((a, b) => a - b);
      onSelectionChange(new Set(downloads.slice(from, to + 1).map((d) => d.id)));
      return;
    }
    if (e.ctrlKey || e.metaKey) {
      const next = new Set(selected);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      onSelectionChange(next);
    } else {
      onSelectionChange(new Set([id]));
    }
    anchor.current = index;
  }

  function handleKeyDown(e: KeyboardEvent<HTMLTableSectionElement>) {
    if (e.key === "Delete" && selected.size > 0) {
      e.preventDefault();
      onDeleteRequest();
    } else if (e.key === "a" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      onSelectionChange(new Set(downloads.map((d) => d.id)));
    } else if (e.key === "Escape") {
      onSelectionChange(new Set());
    } else if (e.key === "Enter") {
      const row = downloads.find((d) => selected.has(d.id));
      if (row) onActivate(row);
    }
  }

  return (
    <div className="table-wrap">
      <table className="downloads" aria-label="Downloads" aria-multiselectable="true">
        <colgroup>
          <col className="col-name" />
          <col className="col-size" />
          <col className="col-progress" />
          <col className="col-status" />
          <col className="col-speed" />
          <col className="col-eta" />
          <col className="col-added" />
        </colgroup>
        <thead>
          <tr>
            <th scope="col">Name</th>
            <th scope="col" className="num">Size</th>
            <th scope="col">Progress</th>
            <th scope="col">Status</th>
            <th scope="col" className="num">Speed</th>
            <th scope="col" className="num">Time left</th>
            <th scope="col">Added</th>
          </tr>
        </thead>
        <tbody onKeyDown={handleKeyDown}>
          {downloads.map((d, index) => {
            const Icon = iconFor(d.fileName);
            // Live numbers only apply while the download is actually running.
            const live = d.status === "downloading" ? progress[d.id] : undefined;
            const done = live?.downloadedBytes ?? d.downloadedBytes;
            const total = live?.totalBytes ?? d.totalBytes;
            const pct = d.status === "completed" ? 100 : progressPercent(done, total);
            const isSelected = selected.has(d.id);
            const subtitle = subtitleFor(d, live, queues[d.queueId]);
            return (
              <tr
                key={d.id}
                tabIndex={0}
                aria-selected={isSelected}
                className={isSelected ? "selected" : undefined}
                onClick={(e) => handleRowClick(e, index, d.id)}
                onDoubleClick={() => onActivate(d)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  if (!selected.has(d.id)) {
                    onSelectionChange(new Set([d.id]));
                    anchor.current = index;
                  }
                  onContextMenu(e.clientX, e.clientY, d);
                }}
                title={d.errorMessage ?? d.url}
              >
                <td>
                  <div className="name-cell">
                    <Icon size={18} className="file-icon" aria-hidden />
                    <div className="name-text">
                      <span className="file-name">
                        {d.priority === 1 && <ArrowUp size={13} className="priority-high" aria-label="High priority" />}
                        {d.priority === -1 && <ArrowDown size={13} className="priority-low" aria-label="Low priority" />}
                        {d.fileName}
                        {d.speedLimit != null && (
                          <span className="limit-tag" title="This download has its own speed limit">
                            <Gauge size={12} aria-hidden />
                            {formatSpeed(d.speedLimit)}
                          </span>
                        )}
                      </span>
                      <span className={live?.stage ? "file-host file-stage" : d.errorMessage ? "file-host file-error" : "file-host"} title={subtitle}>{subtitle}</span>
                    </div>
                  </div>
                </td>
                <td className="num">
                  {d.status === "downloading" || d.status === "paused" ? (
                    <span className="size-progress">
                      {total == null ? formatBytes(done) : `${formatBytes(done)} / ${formatBytes(total)}`}
                    </span>
                  ) : (
                    formatBytes(total)
                  )}
                </td>
                <td>
                  <div className={`progress progress-${d.status}`} role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct ?? undefined} aria-label="Progress">
                    <div className="progress-bar" style={{ width: `${pct ?? 0}%` }} />
                  </div>
                  <span className="progress-text">{pct == null ? "—" : `${pct.toFixed(0)}%`}</span>
                </td>
                <td>
                  <StatusBadge status={d.status} scan={d.scanResult} />
                </td>
                <td className="num">
                  <LedMeter speed={live?.speed} running={d.status === "downloading"} variant="row" />
                </td>
                <td className="num">{formatEta(live?.etaSeconds)}</td>
                <td className="muted">{formatDate(d.createdAt)}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
