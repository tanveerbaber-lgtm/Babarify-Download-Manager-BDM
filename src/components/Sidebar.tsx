import {
  CircleAlert,
  CircleCheck,
  Clock,
  Download as DownloadIcon,
  Globe,
  Loader,
  MonitorPlay,
  Settings as SettingsIcon,
  type LucideIcon,
} from "lucide-react";
import { Logo } from "./Logo";
import { LIST_VIEW_TITLES, type ListView, type View } from "../lib/views";

const FILTERS: { view: ListView; icon: LucideIcon }[] = [
  { view: "active", icon: Loader },
  { view: "pending", icon: Clock },
  { view: "completed", icon: CircleCheck },
  { view: "failed", icon: CircleAlert },
];

interface SidebarProps {
  view: View;
  counts: Record<ListView, number>;
  onNavigate: (view: View) => void;
  /** Opens the Add dialog on the Video / audio tab. */
  onOpenVideo: () => void;
}

function NavButton({
  icon: Icon,
  label,
  current,
  count,
  onClick,
}: {
  icon: LucideIcon;
  label: string;
  current?: boolean;
  count?: number;
  onClick: () => void;
}) {
  return (
    <button type="button" className="nav-item" aria-current={current ? "page" : undefined} onClick={onClick}>
      <Icon size={17} aria-hidden />
      <span className="nav-label">{label}</span>
      {count != null && count > 0 && <span className="nav-count">{count}</span>}
    </button>
  );
}

export function Sidebar({ view, counts, onNavigate, onOpenVideo }: SidebarProps) {
  return (
    <aside className="sidebar">
      <div className="brand">
        <Logo size={36} />
        <div className="brand-text">
          <span className="brand-name">Babarify</span>
          <span className="brand-sub">Download Manager</span>
        </div>
      </div>

      <nav className="nav" aria-label="Main">
        <NavButton icon={DownloadIcon} label="Downloads" current={view === "all"} count={counts.all} onClick={() => onNavigate("all")} />
        <NavButton icon={MonitorPlay} label="Video downloader" onClick={onOpenVideo} />
        <NavButton icon={Globe} label="Browser integration" current={view === "browser"} onClick={() => onNavigate("browser")} />

        <div className="nav-heading">Show</div>
        {FILTERS.map(({ view: v, icon }) => (
          <NavButton key={v} icon={icon} label={LIST_VIEW_TITLES[v]} current={view === v} count={counts[v]} onClick={() => onNavigate(v)} />
        ))}
      </nav>

      <div className="sidebar-footer">
        <NavButton icon={SettingsIcon} label="Settings" current={view === "settings"} onClick={() => onNavigate("settings")} />
      </div>
    </aside>
  );
}
