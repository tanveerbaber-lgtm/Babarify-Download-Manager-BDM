import type { StatusView } from "../lib/views";

interface DashboardStatsProps {
  counts: Record<StatusView, number>;
}

const TILES: { view: StatusView; label: string }[] = [
  { view: "all", label: "Total" },
  { view: "active", label: "Active" },
  { view: "pending", label: "Pending" },
  { view: "completed", label: "Completed" },
];

export function DashboardStats({ counts }: DashboardStatsProps) {
  return (
    <section className="stats" aria-label="Download summary">
      {TILES.map(({ view, label }) => (
        <div key={view} className={`stat stat-${view}`}>
          <span className="stat-label">{label}</span>
          <span className="stat-value">{counts[view]}</span>
        </div>
      ))}
    </section>
  );
}
