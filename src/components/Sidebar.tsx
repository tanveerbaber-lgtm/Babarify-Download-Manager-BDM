import { useState } from "react";
import {
  CalendarClock,
  ChevronDown,
  CircleAlert,
  CircleCheck,
  Clock,
  Download as DownloadIcon,
  FileArchive,
  FileAudio,
  FileQuestion,
  FileText,
  FileVideo,
  Globe,
  Layers,
  Loader,
  MonitorPlay,
  Package,
  Pause,
  Play,
  Plus,
  Settings as SettingsIcon,
  Tag,
  type LucideIcon,
} from "lucide-react";
import { Logo } from "./Logo";
import { LIST_VIEW_TITLES, OTHER_CATEGORY, type StatusView, type View, type ViewCounts } from "../lib/views";
import type { Category, Queue } from "../lib/types";

const FILTERS: { view: StatusView; icon: LucideIcon }[] = [
  { view: "active", icon: Loader },
  { view: "pending", icon: Clock },
  { view: "completed", icon: CircleCheck },
  { view: "failed", icon: CircleAlert },
];

const CATEGORY_ICONS: Record<string, LucideIcon> = {
  videos: FileVideo,
  music: FileAudio,
  documents: FileText,
  compressed: FileArchive,
  programs: Package,
};

interface SidebarProps {
  view: View;
  counts: ViewCounts;
  queues: Queue[];
  categories: Category[];
  onNavigate: (view: View) => void;
  /** Opens the Add dialog on the Video / audio tab. */
  onOpenVideo: () => void;
  onToggleQueue: (queue: Queue) => void;
  onEditQueue: (queue: Queue | null) => void;
}

function NavButton({
  icon: Icon,
  label,
  current,
  count,
  onClick,
  title,
}: {
  icon: LucideIcon;
  label: string;
  current?: boolean;
  count?: number;
  onClick: () => void;
  title?: string;
}) {
  return (
    <button type="button" className="nav-item" aria-current={current ? "page" : undefined} onClick={onClick} title={title ?? label}>
      <Icon size={17} aria-hidden />
      <span className="nav-label">{label}</span>
      {count != null && count > 0 && <span className="nav-count">{count}</span>}
    </button>
  );
}

type Section = "show" | "queues" | "categories";
const COLLAPSED_KEY = "bdm.sidebar.collapsed";

function loadCollapsed(): Record<Section, boolean> {
  try {
    return { show: false, queues: false, categories: false, ...JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? "{}") };
  } catch {
    return { show: false, queues: false, categories: false };
  }
}

/** A section heading that folds its items away. */
function SectionHeading({
  label,
  collapsed,
  onToggle,
  action,
}: {
  label: string;
  collapsed: boolean;
  onToggle: () => void;
  action?: React.ReactNode;
}) {
  return (
    <div className="nav-heading nav-heading-action">
      <button type="button" className="nav-fold" aria-expanded={!collapsed} onClick={onToggle}>
        <ChevronDown size={12} className={collapsed ? "fold-icon folded" : "fold-icon"} aria-hidden />
        {label}
      </button>
      {action}
    </div>
  );
}

/** "Mon–Fri 01:00–06:00" style summary of a queue's schedule. */
export function describeSchedule(queue: Queue): string {
  const s = queue.schedule;
  if (!s.enabled) return queue.running ? "Running" : "Stopped";
  const names = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
  const days = s.days.length === 7 ? "Daily" : s.days.join(",") === "0,1,2,3,4" ? "Mon–Fri" : s.days.map((d) => names[d]).join(" ");
  return `${days} ${s.start}${s.stop ? `–${s.stop}` : ""}`;
}

export function Sidebar({ view, counts, queues, categories, onNavigate, onOpenVideo, onToggleQueue, onEditQueue }: SidebarProps) {
  const others = counts.category[OTHER_CATEGORY] ?? 0;
  const [collapsed, setCollapsed] = useState(loadCollapsed);
  const toggle = (section: Section) =>
    setCollapsed((prev) => {
      const next = { ...prev, [section]: !prev[section] };
      try {
        localStorage.setItem(COLLAPSED_KEY, JSON.stringify(next));
      } catch {
        // Only a convenience.
      }
      return next;
    });
  return (
    <aside className="sidebar">
      <div className="brand">
        <Logo size={30} />
        <div className="brand-text">
          <span className="brand-name">Babarify</span>
          <span className="brand-sub">Download Manager</span>
        </div>
      </div>

      <nav className="nav" aria-label="Main">
        <NavButton icon={DownloadIcon} label="Downloads" current={view === "all"} count={counts.status.all} onClick={() => onNavigate("all")} />
        <NavButton icon={MonitorPlay} label="Video Downloader" onClick={onOpenVideo} />
        <NavButton icon={Globe} label="Browser Integration" current={view === "browser"} onClick={() => onNavigate("browser")} />

        <SectionHeading label="Show" collapsed={collapsed.show} onToggle={() => toggle("show")} />
        {!collapsed.show && FILTERS.map(({ view: v, icon }) => (
          <NavButton key={v} icon={icon} label={LIST_VIEW_TITLES[v]} current={view === v} count={counts.status[v]} onClick={() => onNavigate(v)} />
        ))}

        <SectionHeading
          label="Queues"
          collapsed={collapsed.queues}
          onToggle={() => toggle("queues")}
          action={
            <button type="button" className="nav-add" onClick={() => onEditQueue(null)} title="New queue" aria-label="New queue">
              <Plus size={14} />
            </button>
          }
        />
        {!collapsed.queues && queues.map((q) => {
          const current = view === `queue:${q.id}`;
          return (
            <div key={q.id} className="queue-item" aria-current={current ? "page" : undefined}>
              <button
                type="button"
                className="nav-item"
                aria-current={current ? "page" : undefined}
                onClick={() => onNavigate(`queue:${q.id}`)}
                onDoubleClick={() => onEditQueue(q)}
                title={`${describeSchedule(q)} — double-click to edit`}
              >
                {q.schedule.enabled ? <CalendarClock size={17} aria-hidden /> : <Layers size={17} aria-hidden />}
                <span className="nav-label">
                  {q.name}
                  <span className={q.running ? "queue-state running" : "queue-state"}>{describeSchedule(q)}</span>
                </span>
                {(counts.queue[q.id] ?? 0) > 0 && <span className="nav-count">{counts.queue[q.id]}</span>}
              </button>
              <button
                type="button"
                className="queue-toggle"
                onClick={() => onToggleQueue(q)}
                title={q.running ? `Stop ${q.name}` : `Start ${q.name}`}
                aria-label={q.running ? `Stop ${q.name}` : `Start ${q.name}`}
              >
                {q.running ? <Pause size={13} /> : <Play size={13} />}
              </button>
            </div>
          );
        })}

        <SectionHeading label="Categories" collapsed={collapsed.categories} onToggle={() => toggle("categories")} />
        {!collapsed.categories && categories.map((c) => (
          <NavButton
            key={c.id}
            icon={CATEGORY_ICONS[c.id] ?? Tag}
            label={c.name}
            current={view === `category:${c.id}`}
            count={counts.category[c.id]}
            onClick={() => onNavigate(`category:${c.id}`)}
          />
        ))}
        {!collapsed.categories && others > 0 && (
          <NavButton
            icon={FileQuestion}
            label="Other"
            current={view === `category:${OTHER_CATEGORY}`}
            count={others}
            onClick={() => onNavigate(`category:${OTHER_CATEGORY}`)}
          />
        )}
      </nav>

      <div className="sidebar-footer">
        <NavButton icon={SettingsIcon} label="Settings" current={view === "settings"} onClick={() => onNavigate("settings")} />
      </div>
    </aside>
  );
}
