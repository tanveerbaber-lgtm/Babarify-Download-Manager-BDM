import { useRef, type KeyboardEvent, type MouseEvent } from "react";
import { FileArchive, FileAudio, FileImage, FileText, FileVideo, File, type LucideIcon } from "lucide-react";
import { StatusBadge } from "./StatusBadge";
import { formatBytes, formatDate, formatEta, formatSpeed, progressPercent } from "../lib/format";
import type { Download, Progress } from "../lib/types";

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
}

export function DownloadTable({ downloads, progress, selected, onSelectionChange, onDeleteRequest, onActivate }: DownloadTableProps) {
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
            const subtitle = live?.stage ?? d.errorMessage ?? (d.status === "completed" ? d.savedPath : null) ?? hostOf(d.url);
            return (
              <tr
                key={d.id}
                tabIndex={0}
                aria-selected={isSelected}
                className={isSelected ? "selected" : undefined}
                onClick={(e) => handleRowClick(e, index, d.id)}
                onDoubleClick={() => onActivate(d)}
                title={d.errorMessage ?? d.url}
              >
                <td>
                  <div className="name-cell">
                    <Icon size={18} className="file-icon" aria-hidden />
                    <div className="name-text">
                      <span className="file-name">{d.fileName}</span>
                      <span className={live?.stage ? "file-host file-stage" : d.errorMessage ? "file-host file-error" : "file-host"}>{subtitle}</span>
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
                  <StatusBadge status={d.status} />
                </td>
                <td className="num">{formatSpeed(live?.speed)}</td>
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
