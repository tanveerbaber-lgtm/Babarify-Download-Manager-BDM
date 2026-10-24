import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Check, ChevronRight } from "lucide-react";

export interface MenuItem {
  label: string;
  onSelect?: () => void;
  disabled?: boolean;
  checked?: boolean;
  danger?: boolean;
  /** Opens a submenu on hover. */
  items?: MenuItem[];
}

/** `null` draws a divider. */
export type MenuEntry = MenuItem | null;

interface ContextMenuProps {
  x: number;
  y: number;
  entries: MenuEntry[];
  onClose: () => void;
}

/** A right-click menu at (x, y), kept inside the window. */
export function ContextMenu({ x, y, entries, onClose }: ContextMenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setPos({ x: Math.min(x, window.innerWidth - r.width - 8), y: Math.min(y, window.innerHeight - r.height - 8) });
    el.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
  }, [x, y]);

  useEffect(() => {
    const close = (e: Event) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("mousedown", close);
    window.addEventListener("blur", onClose);
    window.addEventListener("resize", onClose);
    window.addEventListener("scroll", onClose, true);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("blur", onClose);
      window.removeEventListener("resize", onClose);
      window.removeEventListener("scroll", onClose, true);
      window.removeEventListener("keydown", onKey);
    };
  }, [onClose]);

  return (
    <div ref={ref} className="context-menu" role="menu" style={{ left: pos.x, top: pos.y }} onContextMenu={(e) => e.preventDefault()}>
      <MenuList entries={entries} onClose={onClose} />
    </div>
  );
}

function MenuList({ entries, onClose }: { entries: MenuEntry[]; onClose: () => void }) {
  return (
    <>
      {entries.map((item, i) =>
        item === null ? (
          <div key={`sep-${i}`} className="menu-sep" role="separator" />
        ) : item.items ? (
          <div key={item.label} className="menu-sub">
            <button type="button" role="menuitem" aria-haspopup="menu" className="menu-item" disabled={item.disabled}>
              <span className="menu-check" />
              <span className="menu-label">{item.label}</span>
              <ChevronRight size={14} aria-hidden />
            </button>
            {!item.disabled && (
              <div className="context-menu submenu" role="menu">
                <MenuList entries={item.items} onClose={onClose} />
              </div>
            )}
          </div>
        ) : (
          <button
            key={item.label}
            type="button"
            role={item.checked === undefined ? "menuitem" : "menuitemradio"}
            aria-checked={item.checked}
            className={item.danger ? "menu-item danger" : "menu-item"}
            disabled={item.disabled}
            onClick={() => {
              onClose();
              item.onSelect?.();
            }}
          >
            <span className="menu-check">{item.checked && <Check size={14} aria-hidden />}</span>
            <span className="menu-label">{item.label}</span>
          </button>
        ),
      )}
    </>
  );
}
