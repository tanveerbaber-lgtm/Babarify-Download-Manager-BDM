import type { Download, DownloadStatus } from "./types";

export type ListView = "all" | "active" | "pending" | "completed" | "failed";
export type View = ListView | "settings" | "browser";

const VIEW_STATUSES: Record<Exclude<ListView, "all">, DownloadStatus[]> = {
  active: ["downloading", "queued", "paused"],
  pending: ["pending"],
  completed: ["completed"],
  failed: ["failed", "cancelled"],
};

export const LIST_VIEW_TITLES: Record<ListView, string> = {
  all: "Downloads",
  active: "Active",
  pending: "Pending",
  completed: "Completed",
  failed: "Failed",
};

export function inView(download: Download, view: ListView): boolean {
  return view === "all" || VIEW_STATUSES[view].includes(download.status);
}

export function countByView(downloads: Download[]): Record<ListView, number> {
  const counts: Record<ListView, number> = { all: 0, active: 0, pending: 0, completed: 0, failed: 0 };
  for (const d of downloads) {
    counts.all++;
    for (const view of Object.keys(VIEW_STATUSES) as Exclude<ListView, "all">[]) {
      if (VIEW_STATUSES[view].includes(d.status)) counts[view]++;
    }
  }
  return counts;
}
