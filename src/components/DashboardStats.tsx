import { CircleCheck, Clock, Download, Loader, type LucideIcon } from "lucide-react";
import type { StatusView } from "../lib/views";

interface DashboardStatsProps {
  counts: Record<StatusView, number>;
}

const TILES: { view: StatusView; label: string; icon: LucideIcon }[] = [
  { view: "all", label: "Total Downloads", icon: Download },
  { view: "active", label: "Active Downloads", icon: Loader },
  { view: "pending", label: "Pending", icon: Clock },
  { view: "completed", label: "Completed", icon: CircleCheck },
];

export function DashboardStats({ counts }: DashboardStatsProps) {
  return (
    <section className="stats" aria-label="Download summary">
      {TILES.map(({ view, label, icon: Icon }) => (
        <div key={view} className={`stat stat-${view}`}>
          <span className="stat-icon" aria-hidden>
            <Icon size={18} />
          </span>
          <div className="stat-text">
            <span className="stat-label">{label}</span>
            <span className="stat-value">{counts[view]}</span>
          </div>
        </div>
      ))}
    </section>
  );
}
