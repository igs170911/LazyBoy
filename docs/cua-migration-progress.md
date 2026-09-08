# Cua-only migration verification

Verified locally on 2026-09-08 (Linux arm64, Cua Driver 0.23.2). Source changes and local acceptance are complete. Production deployment is not part of this verification.

The subsequent named-cursor feature, Chinese badge font, and its separate image
verification are documented in [Agent cursor on the shared desktop](agent-cursor.md).

## Requirements and evidence

| Requirement | Implementation | Verification |
| --- | --- | --- |
| Remove the old control implementation | Cua is the only ComputerDriver. Deleted LegacyController, direct CDP/AT-SPI Python controllers, clipboard.py, process helpers and the tmux shell. Removed xdotool/xclip packages. | Source audit; old backend names rejected; final running image contains neither binary. |
| See what Cua is doing | Actions foreground the existing browser/native window on the bot's shared display. Shell/file tools use visible named terminals; clipboard operations use a visible GTK editor. | Native/browser/noVNC smoke; terminal and clipboard integration; actual VNC demonstration. |
| All computer actions through Cua | Browser, native pointer/key/ref actions, launch/focus, shell/file tools, clipboard and saved-login use the Cua controller. Removed blanket browser-pixel blocking so canvas/unsupported controls can use fresh screenshot coordinates through Cua. | Adapter, terminal, clipboard, login and dual-display tests; API tool-path audit found no hidden shell execution in agent computer/file tools. |
| Time on every conversation record | Every persisted message renders MessageTime, including attachment/chip messages. Date/time includes seconds, ISO datetime and full local-time tooltip. | Real user and assistant error messages inspected at desktop and 500px width; frontend tests/typecheck/build. |
| Faster/reliable connections | Reuse browser bindings; start VNC before Cua; record the daemon PID; do not inherit startup locks; skip repeated network attachment; reject missing networks before connect and fall back to host ports; recover expired Cua sessions for reads without replaying mutations. | Warm ensure reuses one daemon with an available lock; missing-network fallback leaves no stale attachment; session-expiry observation/browser recovery and mutation refusal pass. |

Infrastructure provisioning/storage/database calls and independent connected-service MCP facilities remain. They are not alternate desktop controllers. Computer, browser, terminal and agent workspace-file interactions use Cua.

## Acceptance results

Final desktop image `lazyboy/computer:cua-work`:
`sha256:4ad46205023694a26ff06f9e969d7c7e0ac43b98cae97a4483d2ba3379462117`

- `cargo test --workspace`: 204 passed; the environment-dependent login integration is ignored by default and separately passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo fmt --all --check` and `git diff --check`: passed.
- Frontend: 44 tests passed; TypeScript/Vite build passed.
- `scripts/cua-smoke-test.sh --docker --repeat 1 --image lazyboy/computer:cua-work`: passed on the final image. Covers native/browser/noVNC, terminal persistence/Unicode/interrupt/reset, clipboard Unicode/multiline/copy, two-display isolation, session expiry, and browser-cookie persistence across pause/restart.
- `COMPUTER_IMAGE=lazyboy/computer:cua-work scripts/cua-login-test.sh`: passed. Uses a trusted local HTTPS fixture in a disposable container and the production field-filling function, checks both exact values and verifies no submission.
- Manual VNC demonstration switched the target desktop from Chromium to its terminal. Teaching retained 2 window events and 3 screenshots, including the final frame. The target's recording was driven by human-style VNC input, not target-side Cua action calls.

Local measurements are samples, not production benchmarks: boot request 2.358s; warm screen URL request 0.100s; missing-network fallback 0.086s with unchanged Docker network attachments. A full container replacement/restart took 10.419s, including Docker shutdown.

## Driver compatibility fixes

- A zero CLI exit code is insufficient: `effect: refused` is treated as failure along with `status: refused`.
- Email input can refuse Cua browser typing. Its fallback resolves one uniquely labelled visible native web entry through Cua, clicks the fresh observed bounds, selects all and pastes through Cua. Duplicate labels, browser chrome and zero-size fields are rejected.
- Native type_text loses Unicode in terminals. Shell commands use ASCII Bash literals encoding UTF-8 bytes; general Unicode/multiline paste uses the Cua-operated clipboard editor. Zsh confirms multiline bracketed paste with another Enter.
- The GTK helper exposes exact clipboard text through its accessibility label because the pinned driver does not return GTK entry values.
- Expired driver sessions are revived for observations. Mutations rejected at expiry are not replayed. Expired browser bindings are classified as stale and re-bound for read requests.
- Browser semantic clicks use `dom_event`; callers still inspect results. Unsupported controls can be operated with Cua coordinates from a fresh screenshot.

## Teaching and environment limits

Cua trajectories record driver invocations, not raw human VNC clicks/keys. Teaching retains window changes and visual keyframes. The start message and model prompt describe that accurately and require review of missing/ambiguous steps. Model failure no longer claims the skill was learned. The local environment has no real model key, so model-generated playbook quality was not tested; recording persistence and missing-key handling were verified.

Local API: `http://127.0.0.1:3111`; supervisor7191. The test bot `1ae00840-ceaf-4197-957d-661df677b015` is running the final image with one Cua daemon. Generated test credentials are in the local .env; no real provider credentials were used. API login sessions are in-memory, so restarting the API requires signing in again (existing behavior).

The disposable Postgres test database uses tmpfs on15434; the pre-existing compose database volume was left intact. The separate old `lazyboy-cua-verify` container is a UI-test viewer, not the final target desktop. Temporary smoke/login containers are removed by their scripts.

## Local evidence files

- `/tmp/lazyboy-workspace-tests.log`, `/tmp/lazyboy-workspace-clippy.log`
- `/tmp/lazyboy-acceptance-smoke.log`, `/tmp/lazyboy-acceptance-login.log`
- `/tmp/lazyboy-network-fallback-test.log`, `/tmp/lazyboy-session-recovery-test.log`
- `/tmp/lazyboy-chat-time.png`, `/tmp/lazyboy-chat-mobile.png`, `/tmp/teach-vnc.png`
- `/tmp/lazyboy-web-build-final.log`, `/tmp/lazyboy-final-frontend-tests.log`
