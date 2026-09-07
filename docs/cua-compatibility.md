# Cua Driver compatibility (LazyBoy desktop)

This report answers one question, from a real `make cua-smoke` run on 2026-09-07:

> Can Cua Driver reliably control the existing LazyBoy XFCE + Xvfb desktop container?

**Yes.** Five core actions succeeded 10 consecutive times. Production `computer_observe` / `computer_act` / `browser` still use the legacy controld path; this only proves Cua as a driver behind those abstractions.

## Environment

- Image: `lazyboy/computer:local` (`image/computer/Dockerfile`)
- Distro: Debian bookworm, x86_64
- Display: Xvfb `DISPLAY=:1` at 1280×800, XFCE (`xfwm4` compositor off, `xfce4-panel`, `xfdesktop`)
- Accessibility: AT-SPI 2 per screen (`at-spi-bus-launcher` + `at-spi2-registryd`)
- Browser: Debian `chromium` via `lazyboy-browser` (persistent profile, `--remote-debugging-port=9221+display`, `--force-renderer-accessibility`, `--lang=zh-TW`)
- Cua Driver: **0.23.2** (`cua-driver-rs-v0.23.2` linux-x86_64-binary, SHA256 `01bf8339ec129cc00f4b4b2c6056ef1a7c5b52df39ff83ad17c9b16818aec500`)
- Install path: `/usr/local/lib/cua-driver` + `/usr/local/bin/cua-driver` (not under the persisted `/home/lazyboy` bind)
- Daemon: `cua-driver serve --grant existing-profile --socket /tmp/lazyboy/cua.sock --no-overlay` on the primary display only
- Telemetry: disabled
- How to reproduce: `make cua-smoke` (artifacts in `/tmp/lazyboy-cua-smoke-last/`)

## Doctor

`cua-driver doctor --json` exit 0, `ok: true`.

| Probe | Status | Note |
| --- | --- | --- |
| binary | ok | `cua-driver 0.23.2 (x86_64-linux)` |
| install dir | ok | `/usr/local/lib/cua-driver/cua-driver` |
| telemetry | ok | disabled |
| display server | ok | X11 `DISPLAY=:1` |
| X11 connection | ok | connected, visible top-level windows |
| AT-SPI | **warn** | CLI `doctor` (docker exec) does not always see the XFCE session bus. The **daemon** started from `lazyboy-screen` does: native `get_window_state` + AT-SPI click/type worked 10/10. |

`cua-driver status --socket /tmp/lazyboy/cua.sock`: daemon running, permission mode `standard`. Unix socket rejects uid 0; smoke and future controld calls must run as uid 1000 (`lazyboy`).

## Results (10 consecutive iterations)

Independent application state, not Cua `"ok"`:

| Check | Result |
| --- | --- |
| `cua-driver --version` | `cua-driver 0.23.2` |
| screenshot (`get_desktop_state`) | 10/10 PNG of the XFCE desktop |
| window / accessibility observation | 10/10 `list_windows` + GTK `get_window_state` |
| native click | 10/10 GTK `Smoke Click` wrote `/tmp/lazyboy/cua-smoke-clicked` |
| native type | 10/10 GTK entry + `Smoke Save` wrote `hello-cua` |
| Chromium attach (existing window/profile) | 10/10 `browser_prepare` `attached_existing_profile` |
| browser semantic click / type | 10/10 local `http://127.0.0.1:8765/cua-smoke.html`; DOM became `clicked-ok` then `typed:hello-cua` |
| noVNC `:6080` still up | 10/10 |
| leftover Cua processes | none (only `cua-driver serve`) |

Same Chromium **pid 334** / **window_id 29360131** across all ten iterations. `browser_prepare` side effects were all false: no isolated profile, no copy, no restart, no extra remote-debugging toggle (LazyBoy already exposes loopback CDP).

Element refs are snapshot-scoped (`p1:1`, `p4:1`, … `p28:1`). Reusing an old ref would be wrong; the smoke re-snapshots every action.

## Relevant tools (0.23.2 `list-tools`)

Observation / native input: `get_desktop_state`, `list_windows`, `get_accessibility_tree`, `get_window_state`, `click`, `type_text`, `press_key`, `hotkey`, `scroll`, `drag`, `get_cursor_position`, `get_screen_size`.

Browser (attach only): `browser_prepare` (`strategy.kind=existing_profile`, `allow_launch=false`), `get_browser_state` (`semantic_v2`), `browser_navigate` (http/https/about only), `browser_click`, `browser_type`.

Not used here: recording, isolated `launch_app` browsers, Wayland helpers. These names must not be exposed to the LLM.

## Integration notes for the next PR

- Call Cua as uid 1000 via `cua-driver call --socket /tmp/lazyboy/cua.sock`. Root is rejected (`reject Unix peer uid 0 for runtime owned by uid 1000`).
- `get_browser_state` on a live LazyBoy Chromium first returns `browser_consent_required` / `consumer_profile_endpoint_requires_grant`. Then `browser_prepare` with `existing_profile` attaches. Serve must keep `--grant existing-profile`. Never `allow_launch`.
- Linux Chromium trusted CDP pointer is unavailable; smoke used `browser_click` `input_route=dom_event` and verified the DOM. Production adapter should prefer that route on this platform and treat `browser_input_trust_unavailable` as classified, not a silent xdotool fallback.
- Extra Team screens are extra Xvfb `DISPLAY`s. This POC only runs a daemon on `:1`. Later: one socket per slot.
- `browser_navigate` refuses `file://`; local fixtures need `http://127.0.0.1`.
- `get_window_state` on Linux is `additionalProperties: false` — do not send macOS-only fields such as `include_accessibility_tree`.

## Known limits

- Doctor AT-SPI warn from a non-desktop D-Bus is not a daemon failure.
- Overlay warnings (`X11 channel rejected command`) appeared in the daemon log with `--no-overlay`; they did not block actions.
- Debian Chromium + zh-TW UI: existing-profile attach worked because CDP was already open, so Cua did not need the English setup-checkbox path.
- Multi-screen, pause/resume, takeover, and skill recording were **not** in this POC; later PRs added controller routing, browser attach, takeover re-observe, and dual-source skill recording.

## Conclusion

Cua Driver 0.23.2 **can** control the existing LazyBoy XFCE + Xvfb container: screenshot, window/AT-SPI observation, native click/type, and Chromium semantic click/type, 10/10, without replacing the browser profile or breaking noVNC.

`ComputerController` is in place. Production now defaults to `cua`. Set `LAZYBOY_COMPUTER_DRIVER=legacy` on the supervisor (passed into each desktop container) to roll back to CDP/AT-SPI/xdotool. Recreate desktop containers after changing the flag.

With `cua`: `POST /observe`, `POST /act`, and `POST /browser` go through Cua Driver. The Agent-facing `browser` schema is unchanged (`snapshot` / `click` / `type` / `press` / `navigate` / `wait`); Cua attaches with `existing_profile` and maps `semantic_v2` refs (`pN:M`) onto the existing element list. After human takeover ends, the run forces a fresh `computer_observe` and drops pre-handoff ids/refs. Skill teaching starts Cua `start_recording` (no video) plus the existing CDP DOM recorder so a human noVNC demo still yields semantic click/type/navigate events; Cua trajectory turns are ingested as extra evidence and password-labelled typing is masked. `use_saved_login` still fills via CDP stdin so passwords never appear on argv. `cdp.py` / AT-SPI remain for login fill, human browser recording, and the `legacy` rollback.
