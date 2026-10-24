import type { Download, DownloadStatus } from "./types";

const RESUMABLE: DownloadStatus[] = ["pending", "paused", "failed", "cancelled"];
const PAUSABLE: DownloadStatus[] = ["downloading", "queued"];
const CANCELLABLE: DownloadStatus[] = ["downloading", "queued", "paused", "pending", "failed"];

export const canResume = (d: Download) => RESUMABLE.includes(d.status);
export const canPause = (d: Download) => PAUSABLE.includes(d.status);
export const canCancel = (d: Download) => CANCELLABLE.includes(d.status);
export const canRedownload = (d: Download) => d.status === "completed" || d.status === "failed" || d.status === "cancelled";

/** Ids from `selected` for which `predicate` holds. */
export function eligible(downloads: Download[], selected: Set<string>, predicate: (d: Download) => boolean) {
  return downloads.filter((d) => selected.has(d.id) && predicate(d)).map((d) => d.id);
}
