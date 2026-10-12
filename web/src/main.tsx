import { AuthGate } from "./auth-gate";
import { useEffect, type ReactNode } from "react";
import ReactDOM from "react-dom/client";
import { I18nProvider } from "./i18n";
import { applyTheme, readTheme, watchSystemTheme } from "./theme";
import { watchViewport } from "./viewport";
import { watchForUpdates } from "./update-check";
import { Workspace } from "./workspace/Workspace";
import "@fontsource/huninn";
import "blobatar/motion.css";
import "blobatar/gaze.css";
import "./styles/base.css";
import "./styles/app.css";
import "./styles/dialogs.css";

applyTheme(readTheme());

function ThemeRoot({ children }: { children: ReactNode }) {
  useEffect(() => watchSystemTheme(), []);
  useEffect(() => watchViewport(), []);
  useEffect(() => watchForUpdates(), []);
  return children;
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <ThemeRoot>
    <I18nProvider>
      <AuthGate><Workspace /></AuthGate>
    </I18nProvider>
  </ThemeRoot>
);

if (import.meta.env.PROD && "serviceWorker" in navigator) {
  navigator.serviceWorker.register("/sw.js").catch(() => undefined);
}
