import { Component, lazy, Suspense, useLayoutEffect, useMemo, useRef, useState, type ComponentType, type ReactNode } from "react";
import { useI18n } from "./i18n";
import { useDialogFocus } from "./dialog-focus";

const recoveredModules = new Map<string, Record<string, unknown>>();

function Placeholder({ onClose, onRetry, failed = false }: { onClose: () => void; onRetry?: () => void; failed?: boolean }) {
  const { t } = useI18n();
  const ref = useRef<HTMLDivElement>(null);
  useDialogFocus(ref, onClose);
  return <div className="modal-backdrop"><div ref={ref} className="dialog deferred-dialog" role="dialog" aria-modal="true" aria-label={t.featureLoading}>
    <p role={failed ? "alert" : "status"}>{failed ? t.featureLoadFailed : t.featureLoading}</p>
    <div className="dialog-actions">
      <button type="button" className="outline" onClick={onClose}>{t.close}</button>
      {failed && onRetry ? <button type="button" className="primary" onClick={onRetry}>{t.retry}</button> : null}
    </div>
  </div></div>;
}

class LoadBoundary extends Component<{ onClose: () => void; onRetry: () => void; children: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() { return { failed: true }; }
  render() { return this.state.failed ? <Placeholder onClose={this.props.onClose} onRetry={this.props.onRetry} failed /> : this.props.children; }
}

export function deferDialog<Props extends { onClose: () => void }>(load: () => Promise<{ default: ComponentType<Props> }>, exportName: string) {
  let sourceUrl: string | null = null;
  const selected = (module: Record<string, unknown>) => {
    if (typeof module[exportName] !== "function") throw new Error("Dialog export unavailable");
    return {default: module[exportName] as ComponentType<Props>};
  };
  return function DeferredDialog(props: Props) {
    const [attempt, setAttempt] = useState(0);
    // React.lazy caches a rejected load. Reset that boundary without reloading
    // the app, so conversation drafts and active computer connections survive.
    const Panel = useMemo(() => lazy(async () => {
      try {
        if (sourceUrl && recoveredModules.has(sourceUrl)) return selected(recoveredModules.get(sourceUrl)!);
        if (attempt && sourceUrl) {
          const url = new URL(sourceUrl);
          url.searchParams.set("lazyboy_retry", String(attempt));
          // Chromium also remembers a failed module fetch. Request a fresh URL
          // for this lazy module while reusing the already loaded app/React.
          const module = await import(/* @vite-ignore */ url.href);
          const component = selected(module);
          recoveredModules.set(sourceUrl, module);
          return component;
        }
        return await load();
      } catch (error) {
        const candidate = String(error instanceof Error ? error.message : error).match(/https?:\/\/[^\s"'<>]+/)?.[0];
        if (candidate) {
          const url = new URL(candidate);
          if (url.origin === location.origin && !url.username && !url.password && /^\/(?:assets\/[^/]+\.js|src\/[^/]+\.tsx)$/.test(url.pathname)) {
            url.searchParams.delete("lazyboy_retry");
            sourceUrl = url.href;
            if (recoveredModules.has(sourceUrl)) return selected(recoveredModules.get(sourceUrl)!);
          }
        }
        throw error;
      }
    }), [attempt]);
    const previous = useRef(document.activeElement as HTMLElement | null);
    useLayoutEffect(() => () => {
      queueMicrotask(() => { if (previous.current?.isConnected) previous.current.focus({ preventScroll: true }); });
    }, []);
    return <LoadBoundary key={attempt} onClose={props.onClose} onRetry={() => setAttempt(value => value + 1)}><Suspense fallback={<Placeholder onClose={props.onClose} />}><Panel {...props} /></Suspense></LoadBoundary>;
  };
}
