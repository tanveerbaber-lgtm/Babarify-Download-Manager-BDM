import type { Download, DownloadStatus } from "./types";

export type StatusView = "all" | "active" | "pending" | "completed" | "failed";
/** A status filter, one queue (`queue:<id>`) or one category (`category:<id>`, `category:other`). */
export type ListView = StatusView | `queue:${string}` | `category:${string}`;
export type View = ListView | "settings" | "browser";

const VIEW_STATUSES: Record<Exclude<StatusView, "all">, DownloadStatus[]> = {
  active: ["downloading", "queued", "paused"],
  pending: ["pending"],
  completed: ["completed"],
  failed: ["failed", "cancelled"],
};

export const LIST_VIEW_TITLES: Record<StatusView, string> = {
  all: "Downloads",
  active: "Active",
  pending: "Pending",
  completed: "Completed",
  failed: "Failed",
};

export const OTHER_CATEGORY = "other";

export function isListView(view: View): view is ListView {
  return view !== "settings" && view !== "browser";
}

export function inView(download: Download, view: ListView): boolean {
  if (view === "all") return true;
  if (view.startsWith("queue:")) return download.queueId === view.slice(6);
  if (view.startsWith("category:")) return (download.category ?? OTHER_CATEGORY) === view.slice(9);
  return VIEW_STATUSES[view as Exclude<StatusView, "all">].includes(download.status);
}

export interface ViewCounts {
  status: Record<StatusView, number>;
  queue: Record<string, number>;
  category: Record<string, number>;
}

/** Counts per status view, per queue (unfinished only) and per category. */
export function countByView(downloads: Download[]): ViewCounts {
  const status: Record<StatusView, number> = { all: 0, active: 0, pending: 0, completed: 0, failed: 0 };
  const queue: Record<string, number> = {};
  const category: Record<string, number> = {};
  for (const d of downloads) {
    status.all++;
    for (const view of Object.keys(VIEW_STATUSES) as Exclude<StatusView, "all">[]) {
      if (VIEW_STATUSES[view].includes(d.status)) status[view]++;
    }
    if (d.status !== "completed" && d.status !== "cancelled") queue[d.queueId] = (queue[d.queueId] ?? 0) + 1;
    const c = d.category ?? OTHER_CATEGORY;
    category[c] = (category[c] ?? 0) + 1;
  }
  return { status, queue, category };
}
