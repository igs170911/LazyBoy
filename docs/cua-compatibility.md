# Cua Driver compatibility (LazyBoy desktop)

This report answers one question, from a real `make cua-smoke` run on 2026-09-07:

> Can Cua Driver reliably control the existing LazyBoy XFCE + Xvfb desktop container?

**Yes, as an opt-in backend.** A disposable `lazyboy/computer:local` desktop on 2026-09-07 (linux/arm64, Cua Driver 0.23.2) passed raw Driver smoke 10/10, the LazyBoy adapter 10/10, dual-display isolation, and Chromium cookie persistence across `docker pause` and `docker restart`. Production still defaults to `legacy`. See [cua-review.md](cua-review.md) and [cua-benchmark.md](cua-benchmark.md).

## Environment

- Image: `lazyboy/computer:local` (`image/computer/Dockerfile`)
- Distro: Debian bookworm; Driver binary follows `TARGETARCH` (`linux-arm64` or `linux-x86_64`)
- Display: Xvfb `DISPLAY=:1` at 1280×800, XFCE (`xfwm4` compositor off, `xfce4-panel`, `xfdesktop`)
- Accessibility: AT-SPI 2 per screen (`at-spi-bus-launcher` + `at-spi2-registryd`)
- Browser: Debian `chromium` via `lazyboy-browser` (persistent profile, `--remote-debugging-port=9221+display`, `--force-renderer-accessibility`, `--lang=zh-TW`)
- Cua Driver: **0.23.2** (`cua-driver-rs-v0.23.2`; linux-arm64 SHA256 `be22768a207796a4bc1de50c52f32f9ef680b5e86e58c059e02eec2caba2e7bb`, linux-x86_64 SHA256 `01bf8339ec129cc00f4b4b2c6056ef1a7c5b52df39ff83ad17c9b16818aec500`)
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

Observation / native input used by `controld`: `get_desktop_state`, `list_windows`,
`get_window_state`, `click`, `type_text`, `press_key`, `hotkey`, `scroll`, `drag`,
`move_cursor`, `set_value`, `bring_to_front`, `get_cursor_position`, `get_screen_size`.

Browser (attach only): `browser_prepare` (`strategy.kind=existing_profile`,
`allow_launch=false`), `get_browser_state` (`semantic_v2`), `browser_navigate`
(http/https/about only), `browser_click`, `browser_type`.

Lifecycle / diagnostics: `start_session`, `end_session`, `health_report`. Skill
teaching also uses `start_recording` / `stop_recording`.

Not used: `mouse_button_down`, `mouse_button_up`, `mouse_drag` (held-button
background X11 tools). LazyBoy's action DSL has no partial-pointer state, so
`Pointer{Down}` / `Pointer{Up}` translate to `ControlError::Unsupported` instead
of a half-pressed button nobody releases. Also unused: isolated `launch_app`
browsers, Wayland helpers, and the deprecated `get_session_state` /
`escalate_session` aliases. None of these names may be exposed to the LLM.

## Driver contract rules (enforced in `crates/control/src/cua`)

Each of these was confirmed against a real 0.23.2 daemon, and each one fails
silently (exit 0) if violated:

1. **Repeat the `session` label on every call.** `CuaClient::call` injects
   `lazyboy-<display>` unless the caller already set one. Without it each CLI
   process gets an ephemeral `cli-<uuid>` session, so trajectory turns,
   snapshots, and browser binds never line up, and every call also emits a bogus
   `end_session` turn.
2. **Never send `target: {kind: "desktop", display_id: "primary"}`.** The Linux
   driver rejects it with `invalid_action_target` (exit 0). Omit `target` to use
   the global input route.
3. **Desktop `scroll` needs a point.** With `scope: "desktop"`, `x`/`y` are
   required (`missing field x`); `dispatch` aims at the pointer and falls back to
   the screen centre. `amount` is clamped to the schema range `1..=50`.
4. **Only a JSON object proves the tool ran.** A refusal or prose banner that
   arrives with exit 0 is a failure (`decode_stdout`), never an empty success.
5. **One escalation retry.** `background_unavailable` carries
   `escalation.recommended`; the client retries once with that `delivery_mode`
   and never loops.
6. **`get_window_state` can be degraded.** AT-SPI intermittently answers with
   `degraded: true` and a root-only tree. Such windows are skipped and the
   observation reports `native_observation_complete: false` rather than failing
   the whole `observe`.
7. **Nothing is validated for you.** The Linux schemas declare
   `additionalProperties: false` and numeric bounds, but 0.23.2 accepts unknown
   keys and out-of-range values anyway (a bogus key on `list_windows` and
   `scroll amount: 0` both return exit 0 with `effect: unverifiable`), so the
   bounds in `docs/cua-schemas/0.23.2/` are enforced here, by the client.
   `session` is accepted by every tool, including the ones whose own schema
   omits it (`list_windows`, `bring_to_front`, `health_report`,
   `start_recording`), which is what lets rule 1 be applied uniformly.
8. **Socket peer uid.** `/tmp/lazyboy/cua*.sock` rejects uid 0; `controld` runs as
   the desktop user (uid 1000).

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

`ComputerController` is in place. Production defaults to `legacy` pending the full acceptance suite and benchmark. Set `LAZYBOY_COMPUTER_DRIVER=cua` only for explicit testing. Set `LAZYBOY_COMPUTER_DRIVER=legacy` on the supervisor (passed into each desktop container) to roll back to CDP/AT-SPI/xdotool. Recreate desktop containers after changing the flag.

With `cua`: `POST /observe`, `POST /act`, and `POST /browser` go through Cua Driver. The Agent-facing `browser` schema is unchanged (`snapshot` / `click` / `type` / `press` / `navigate` / `wait`); Cua attaches with `existing_profile` and maps `semantic_v2` refs (`pN:M`) onto the existing element list. After human takeover ends, the run forces a fresh `computer_observe` and drops pre-handoff ids/refs. Skill teaching starts Cua `start_recording` (no video) plus the existing CDP DOM recorder so a human noVNC demo still yields semantic click/type/navigate events; Cua trajectory turns are ingested as extra evidence and password-labelled typing is masked. `use_saved_login` still fills via CDP stdin so passwords never appear on argv. `cdp.py` / AT-SPI remain for login fill, human browser recording, and the `legacy` rollback.


## Review of the current checkout (2026-09-07)

The locally tagged `lazyboy/computer:local` image now installs Cua Driver 0.23.2
for the build architecture (`cua-driver 0.23.2` on linux/arm64 in this run).
`make cua-smoke` is the acceptance entry: raw Driver smoke, adapter E2E,
isolation, and pause/restart persistence. Production defaults remain `legacy`.
See [the migration audit](cua-review.md).

One upstream caveat about that persistence claim: Chromium writes its cookie
database on a ~30 s timer and does not flush on `SIGTERM`. A profile survives
`docker pause` / `docker restart` once that write has landed; stopping a desktop
seconds after a login can still lose the cookie, and no LazyBoy code controls
the timer. `scripts/cua-smoke-test.sh` waits for the fixture cookie to reach the
profile before it restarts, so the check measures profile persistence instead of
the flush timer.

Image architectures are capped at `linux/amd64` and `linux/arm64` by upstream
binaries: the Cua Driver ships only `linux-x86_64` / `linux-arm64` and ONNX
Runtime only `linux-x64` / `linux-aarch64`. See
[development.md](development.md#映像與-cpu-架構).
