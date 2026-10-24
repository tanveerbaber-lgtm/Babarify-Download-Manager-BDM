import type { DownloadStatus } from "./types";

const UNITS = ["B", "KB", "MB", "GB", "TB"];

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null || !Number.isFinite(bytes) || bytes < 0) return "—";
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit++;
  }
  const digits = unit === 0 || value >= 100 ? 0 : 1;
  return `${value.toFixed(digits)} ${UNITS[unit]}`;
}

export function formatSpeed(bytesPerSecond: number | null | undefined): string {
  if (!bytesPerSecond) return "—";
  return `${formatBytes(bytesPerSecond)}/s`;
}

export function formatEta(seconds: number | null | undefined): string {
  if (seconds == null || !Number.isFinite(seconds) || seconds < 0) return "—";
  const s = Math.round(seconds);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${s % 60}s`;
  return `${s}s`;
}

/** 635 → "10:35", 3725 → "1:02:05" */
export function formatDuration(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

const dateFormat = new Intl.DateTimeFormat(undefined, {
  dateStyle: "medium",
  timeStyle: "short",
});

export function formatDate(epochMs: number): string {
  return dateFormat.format(new Date(epochMs));
}

export function progressPercent(downloaded: number, total: number | null): number | null {
  if (!total || total <= 0) return null;
  return Math.min(100, Math.max(0, (downloaded / total) * 100));
}

export const STATUS_LABELS: Record<DownloadStatus, string> = {
  pending: "Pending",
  queued: "Queued",
  downloading: "Downloading",
  paused: "Paused",
  completed: "Completed",
  failed: "Failed",
  cancelled: "Cancelled",
};
