import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useDialogFocus } from "../dialog-focus";
import { X } from "./icons";

export function Modal({ label, onClose, children, className = "", dismissible = true }: {
  label: string;
  onClose: () => void;
  children: ReactNode;
  className?: string;
  dismissible?: boolean;
}) {
  const root = useRef<HTMLDivElement>(null);
  useDialogFocus(root, dismissible ? onClose : undefined);
  return createPortal(
    <div className="modal-backdrop" onMouseDown={event => { if (dismissible && event.target === event.currentTarget) onClose(); }}>
      <div ref={root} role="dialog" aria-modal="true" aria-label={label} className={`dialog ${className}`}>
        {children}
      </div>
    </div>,
    document.body,
  );
}

export function ModalHeader({ title, onClose, closeLabel = "關閉", disabled }: { title: ReactNode; onClose: () => void; closeLabel?: string; disabled?: boolean }) {
  return (
    <div className="dialog-title">
      <h2>{title}</h2>
      <button type="button" className="icon-btn" aria-label={closeLabel} disabled={disabled} onClick={onClose}><X /></button>
    </div>
  );
}

export function ConfirmDialog({ title, body, confirmLabel, danger, onConfirm, onClose }: {
  title: string;
  body: ReactNode;
  confirmLabel: string;
  danger?: boolean;
  onConfirm: () => Promise<void> | void;
  onClose: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  return (
    <Modal label={title} onClose={() => { if (!busy) onClose(); }} className="confirm-dialog">
      <ModalHeader title={title} onClose={onClose} disabled={busy} />
      <div className="dialog-lead">{body}</div>
      {error ? <p className="pane-error" role="alert">{error}</p> : null}
      <div className="dialog-actions">
        <button type="button" disabled={busy} onClick={onClose}>取消</button>
        <button type="button" className={danger ? "danger solid" : "primary"} disabled={busy} onClick={async () => {
          setBusy(true); setError("");
          try { await onConfirm(); onClose(); }
          catch (err) { setError(err instanceof Error ? err.message : String(err)); setBusy(false); }
        }}>{busy ? "處理中…" : confirmLabel}</button>
      </div>
    </Modal>
  );
}

export type MenuItem = { label: string; icon?: ReactNode; onSelect: () => void; danger?: boolean; disabled?: boolean; hint?: string } | "separator";

export function MenuButton({ label, icon, items, placement = "bottom-end", className = "icon-btn", children }: {
  label: string;
  icon?: ReactNode;
  items: MenuItem[];
  placement?: "bottom-end" | "bottom-start" | "top-start" | "top-end";
  className?: string;
  children?: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState<{ top: number; left: number } | null>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (!open || !trigger.current || !menu.current) return;
    const anchor = trigger.current.getBoundingClientRect();
    const box = menu.current.getBoundingClientRect();
    const up = placement.startsWith("top") || anchor.bottom + box.height + 8 > window.innerHeight;
    const end = placement.endsWith("end");
    let left = end ? anchor.right - box.width : anchor.left;
    left = Math.max(8, Math.min(left, window.innerWidth - box.width - 8));
    const top = up ? Math.max(8, anchor.top - box.height - 6) : anchor.bottom + 6;
    setPosition({ top, left });
    menu.current.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
  }, [open, placement]);
  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!menu.current?.contains(target) && !trigger.current?.contains(target)) setOpen(false);
    };
    const close = () => setOpen(false);
    document.addEventListener("pointerdown", outside, true);
    window.addEventListener("resize", close);
    window.addEventListener("blur", close);
    return () => {
      document.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("resize", close);
      window.removeEventListener("blur", close);
    };
  }, [open]);
  function onKeyDown(event: React.KeyboardEvent) {
    const buttons = [...(menu.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") || [])];
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); setOpen(false); trigger.current?.focus(); }
    else if (event.key === "ArrowDown") { event.preventDefault(); buttons[(index + 1) % buttons.length]?.focus(); }
    else if (event.key === "ArrowUp") { event.preventDefault(); buttons[(index - 1 + buttons.length) % buttons.length]?.focus(); }
    else if (event.key === "Tab") setOpen(false);
  }
  return (
    <>
      <button ref={trigger} type="button" className={className} aria-label={label} title={label} aria-haspopup="menu" aria-expanded={open}
        onClick={event => { event.stopPropagation(); setOpen(value => !value); }}>
        {icon}{children}
      </button>
      {open ? createPortal(
        <div ref={menu} role="menu" aria-label={label} className="menu" onKeyDown={onKeyDown}
          style={position ? { top: position.top, left: position.left } : { visibility: "hidden", top: 0, left: 0 }}>
          {items.map((item, index) => item === "separator"
            ? <div key={`s${index}`} className="menu-sep" role="separator" />
            : <button key={item.label} type="button" role="menuitem" className={item.danger ? "danger" : ""} disabled={item.disabled}
                onClick={() => { setOpen(false); trigger.current?.focus({ preventScroll: true }); item.onSelect(); }}>
                {item.icon}<span>{item.label}</span>{item.hint ? <small>{item.hint}</small> : null}
              </button>)}
        </div>,
        document.body,
      ) : null}
    </>
  );
}

const LONG_PRESS_MS = 480;

/**
 * A menu that opens where it was asked for: right-click with a mouse, a long press on a touch screen, or the
 * keyboard's menu key (Shift+F10). Spread `bind` on the row; render `menu` anywhere.
 */
export function useContextMenu(label: string, items: () => MenuItem[]) {
  const [at, setAt] = useState<{ x: number; y: number; items: MenuItem[]; anchor: Element | null } | null>(null);
  const press = useRef<{ timer: number; x: number; y: number; fired: boolean; anchor: Element | null } | null>(null);
  const open = (x: number, y: number, anchor: Element | null) => setAt({ x, y, items: items(), anchor });
  const cancel = () => { if (press.current) window.clearTimeout(press.current.timer); };
  // React events from the open menu (a portal) still bubble through the row; leave those alone.
  const fromMenu = (event: React.SyntheticEvent) => !!(event.target as Element).closest?.(".menu");
  const bind = {
    onContextMenu: (event: React.MouseEvent) => {
      event.preventDefault();
      if (fromMenu(event)) return;
      if (press.current?.fired) return;
      // The menu key reports no pointer position; open beside the row instead.
      const anchor = event.currentTarget as Element;
      if (!event.clientX && !event.clientY) { const box = anchor.getBoundingClientRect(); open(box.left + 24, box.bottom - 4, anchor); }
      else open(event.clientX, event.clientY, anchor);
    },
    onPointerDown: (event: React.PointerEvent) => {
      if (event.pointerType !== "touch" || fromMenu(event)) return;
      cancel();
      const { clientX: x, clientY: y } = event;
      const anchor = event.currentTarget as Element;
      press.current = { x, y, anchor, fired: false, timer: window.setTimeout(() => {
        if (!press.current) return;
        press.current.fired = true;
        navigator.vibrate?.(10);
        open(x, y, anchor);
      }, LONG_PRESS_MS) };
    },
    onPointerMove: (event: React.PointerEvent) => {
      const p = press.current;
      if (p && !p.fired && Math.hypot(event.clientX - p.x, event.clientY - p.y) > 8) { cancel(); press.current = null; }
    },
    onPointerUp: cancel,
    onPointerCancel: () => { cancel(); press.current = null; },
    // The tap that ends a long press must not also open the row.
    onClickCapture: (event: React.MouseEvent) => {
      if (fromMenu(event)) return;
      if (press.current?.fired) { event.preventDefault(); event.stopPropagation(); }
      press.current = null;
    },
  };
  const menu = at ? <PointMenu label={label} x={at.x} y={at.y} items={at.items} anchor={at.anchor} onClose={() => { setAt(null); press.current = null; }} /> : null;
  return { bind, menu, opened: !!at };
}

function PointMenu({ label, x, y, items, anchor, onClose }: { label: string; x: number; y: number; items: MenuItem[]; anchor: Element | null; onClose: () => void }) {
  const menu = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<{ top: number; left: number } | null>(null);
  useLayoutEffect(() => {
    if (!menu.current) return;
    const box = menu.current.getBoundingClientRect();
    setPosition({
      left: Math.max(8, Math.min(x, window.innerWidth - box.width - 8)),
      top: y + box.height + 8 > window.innerHeight ? Math.max(8, y - box.height) : y,
    });
  }, [x, y]);
  // Focus once placed: a still-hidden menu cannot take focus.
  useEffect(() => { if (position) menu.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus({ preventScroll: true }); }, [!!position]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    const outside = (event: Event) => { if (!menu.current?.contains(event.target as Node)) onClose(); };
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape") { event.preventDefault(); onClose(); } };
    // Only scrolling what holds the row moves it away from the menu; a conversation scrolling as a reply streams in
    // elsewhere on the page must not close it.
    const scrolled = (event: Event) => { const target = event.target; if (target === document || (anchor && target instanceof Node && target.contains(anchor))) onClose(); };
    document.addEventListener("keydown", escape);
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("contextmenu", outside, true);
    window.addEventListener("resize", onClose);
    window.addEventListener("blur", onClose);
    window.addEventListener("scroll", scrolled, true);
    return () => {
      document.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("contextmenu", outside, true);
      window.removeEventListener("resize", onClose);
      window.removeEventListener("blur", onClose);
      window.removeEventListener("scroll", scrolled, true);
      document.removeEventListener("keydown", escape);
    };
  }, [onClose, anchor]);
  function onKeyDown(event: React.KeyboardEvent) {
    const buttons = [...(menu.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") || [])];
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (event.key === "Escape" || event.key === "Tab") { event.preventDefault(); event.stopPropagation(); onClose(); }
    else if (event.key === "ArrowDown") { event.preventDefault(); buttons[(index + 1) % buttons.length]?.focus(); }
    else if (event.key === "ArrowUp") { event.preventDefault(); buttons[(index - 1 + buttons.length) % buttons.length]?.focus(); }
  }
  return createPortal(
    <div ref={menu} role="menu" aria-label={label} className="menu" onKeyDown={onKeyDown} onContextMenu={event => event.preventDefault()}
      style={position ? position : { visibility: "hidden", top: 0, left: 0 }}>
      {items.map((item, index) => item === "separator"
        ? <div key={`s${index}`} className="menu-sep" role="separator" />
        : <button key={item.label} type="button" role="menuitem" className={item.danger ? "danger" : ""} disabled={item.disabled}
            onClick={() => { onClose(); item.onSelect(); }}>
            {item.icon}<span>{item.label}</span>{item.hint ? <small>{item.hint}</small> : null}
          </button>)}
    </div>,
    document.body,
  );
}

type Toast = { id: number; text: string; kind: "info" | "error" | "ok"; action?: { label: string; run: () => void } };
let toasts: Toast[] = [];
let nextToast = 1;
const listeners = new Set<() => void>();
function publish(next: Toast[]) { toasts = next; listeners.forEach(listener => listener()); }

export function toast(text: string, kind: Toast["kind"] = "info", action?: Toast["action"], ms = kind === "error" ? 7000 : 4000) {
  const id = nextToast++;
  publish([...toasts.filter(item => item.text !== text), { id, text, kind, action }].slice(-3));
  window.setTimeout(() => publish(toasts.filter(item => item.id !== id)), ms);
}

export function Toaster() {
  const items = useSyncExternalStore(listener => { listeners.add(listener); return () => { listeners.delete(listener); }; }, () => toasts);
  return createPortal(
    <div className="toaster" aria-live="polite">
      {items.map(item => (
        <div key={item.id} className={`toast ${item.kind}`} role={item.kind === "error" ? "alert" : "status"}>
          <span>{item.text}</span>
          {item.action ? <button type="button" onClick={() => { item.action!.run(); publish(toasts.filter(t => t.id !== item.id)); }}>{item.action.label}</button> : null}
          <button type="button" className="icon-btn small" aria-label="關閉提示" onClick={() => publish(toasts.filter(t => t.id !== item.id))}><X /></button>
        </div>
      ))}
    </div>,
    document.body,
  );
}
