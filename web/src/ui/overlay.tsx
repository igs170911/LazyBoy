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
