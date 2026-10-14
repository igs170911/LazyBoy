import { Component, type ErrorInfo, type ReactNode } from "react";
import { dismissBoot } from "./boot";

/** One broken piece of data must not leave a blank page: show what happened and a way back. */
export class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) { return { error }; }

  componentDidCatch(error: Error, info: ErrorInfo) { dismissBoot(); console.error("LazyBoy crashed", error, info.componentStack); }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <main className="crash-page" role="alert">
        <img src="/lazyboy-round.svg" width={64} height={64} alt="" />
        <h1>畫面出了點問題</h1>
        <p>Bot 的工作不受影響，重新載入就能回到剛才的地方。</p>
        <pre>{this.state.error.message}</pre>
        <div className="welcome-actions">
          <button type="button" className="btn primary" onClick={() => window.location.reload()}>重新載入</button>
          <button type="button" className="btn" onClick={() => this.setState({ error: null })}>再試一次</button>
        </div>
      </main>
    );
  }
}
