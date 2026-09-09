// Linux adapter for Cua's existing browser visual-feedback contract.
// Cosmetic only: never inject input or activate an application/tab.
use cua_driver_core::browser::platform::{BrowserVisualAction, BrowserVisualActionKind};
use cursor_overlay::{CursorRegistry, OverlayCommand};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct BrowserCursorTracker {
    bindings: HashMap<String, (u64, String)>,
}

impl BrowserCursorTracker {
    fn update(&mut self, action: &BrowserVisualAction) -> Vec<(String, bool)> {
        self.bindings
            .retain(|session, _| !cua_driver_core::session::is_session_ended(session));
        self.bindings.insert(
            action.session.clone(),
            (action.window_id, action.cdp_target_id.clone()),
        );
        if !action.tab_is_active
            || !action.screen_x.is_some_and(f64::is_finite)
            || !action.screen_y.is_some_and(f64::is_finite) {
            return vec![(action.session.clone(), false)];
        }
        self.bindings
            .iter()
            .filter(|(_, (window, _))| *window == action.window_id)
            .map(|(session, (_, tab))| {
                (
                    session.clone(),
                    session == &action.session && tab == &action.cdp_target_id,
                )
            })
            .collect()
    }
}

impl Default for LinuxBrowserPlatform {
    fn default() -> Self {
        Self::new(Arc::new(CursorRegistry::new()))
    }
}

impl LinuxBrowserPlatform {
    pub fn new(cursor_registry: Arc<CursorRegistry>) -> Self {
        Self {
            cursor_registry,
            browser_cursors: Mutex::new(BrowserCursorTracker::default()),
        }
    }

    async fn show_browser_cursor(&self, action: BrowserVisualAction) {
        if action.session.is_empty()
            || action.cdp_target_id.is_empty()
            || cua_driver_core::session::is_session_ended(&action.session)
        {
            return;
        }
        let enabled = self
            .cursor_registry
            .get_or_create(&action.session)
            .config
            .enabled;
        let updates = match self.browser_cursors.lock() {
            Ok(mut tracker) => tracker.update(&action),
            Err(_) => return,
        };
        for (session, visible) in updates {
            let allowed = self
                .cursor_registry
                .get(&session)
                .is_some_and(|state| state.config.enabled);
            crate::overlay::send_command_for(
                session,
                OverlayCommand::SetEnabled(visible && allowed),
            );
        }
        if !action.tab_is_active || !enabled {
            return;
        }
        let (Some(x), Some(y)) = (action.screen_x, action.screen_y) else {
            return;
        };
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        crate::overlay::send_command_for(
            action.session.clone(),
            OverlayCommand::PinAbove(action.window_id),
        );
        let diagnostics = std::env::var_os("LAZYBOY_CURSOR_DIAGNOSTICS").is_some();
        let before = crate::overlay::current_position_for(&action.session);
        // Renderer failure must never hold up the real browser operation.
        let arrived = if before.0 < -50.0 && before.1 < -50.0 {
            true // No known starting position: reveal directly at the first target.
        } else {
            tokio::time::timeout(
                Duration::from_millis(250),
                crate::overlay::animate_cursor_to_for(action.session.clone(), x, y),
            ).await.is_ok()
        };
        if diagnostics {
            eprintln!("lazyboy-browser-cursor window={} kind={:?} target=({x},{y}) before={before:?} after={:?} arrived={}", action.window_id, action.kind, crate::overlay::current_position_for(&action.session), arrived);
        }
        self.cursor_registry.update_position(&action.session, x, y);
        if matches!(
            action.kind,
            BrowserVisualActionKind::Click
                | BrowserVisualActionKind::Type
                | BrowserVisualActionKind::RightClick
                | BrowserVisualActionKind::DoubleClick
                | BrowserVisualActionKind::Drag
        ) {
            crate::overlay::send_command_for(action.session.clone(), OverlayCommand::ClickPulse { x, y });
        }
        // ClickPulse does not cancel an in-flight path or settling spring. A
        // timed-out animation would otherwise keep overriding the target after
        // input has happened. Cua's shared transform preserves the arrow hotspot.
        crate::overlay::send_command_for(action.session, cursor_overlay::track_pointer_command(x, y));
    }
}
