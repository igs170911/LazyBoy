# Agent cursor on the shared desktop

The computer image enables Cua's native cursor overlay. Cua draws it in X11, so
both the embedded viewer and an enlarged/direct VNC viewer receive the same
cursor and name. There is no separate frontend cursor or coordinate replay.

The API supplies the bot's current name and `avatarColor` when ensuring/restoring its screen. The
screen launcher atomically writes these settings in the display's temporary runtime
directory. The Cua client uses that name as its public session label, keeping it
consistent across observation, native/browser input, and session revival.
Display sockets provide isolation even when two agents have the same name.
Renaming starts a new public session: take a fresh browser snapshot before using
browser references again. Mutations using stale references are not replayed.
Calls that explicitly provide a session retain their explicit label; unnamed
screens retain `lazyboy-N` for compatibility.

Cua owns the cursor animation, action feedback, idle hiding, and label truncation.
Long names may be shortened in its badge. The cursor is a synthetic agent cursor,
independent of the viewer's local mouse pointer. No extra input is injected to
animate it.

## Selected agent colors

The cursor fill, glow, and name badge use the agent's existing `#RRGGBB` color.
Saving the agent's appearance also refreshes the color on an existing running
screen. This cosmetic command does not start a stopped computer, focus an app,
rename the Cua session, or replay input. Ensure/restore reapplies the persisted
setting if the running desktop could not be reached during a save.

`image/computer/cua-color.patch` adds a small override to Cua's shared color
function. The launcher sets `LAZYBOY_CURSOR_COLOR_FILE` separately for each display
daemon. `cua-color.rs` reads and caches that bounded hex value for 100 ms; missing
or invalid values retain Cua's original palette. Both cursor artwork and its
badge already consume the shared color function, so their styling stays aligned.
Names/session identities do not change when the color changes.

## Chinese names

Official Cua Driver 0.23.2 binaries embed Inter in their session badge renderer.
Inter has no Chinese glyphs, and the renderer does not consult fontconfig. Merely
installing system CJK fonts does not fix this.

The computer Dockerfile builds the pinned 0.23.2 source with the existing Huninn
2.1 UI font in that embedded asset. Both downloads have SHA-256 checks. Apart from
the cosmetic color override, driver code and its locked dependency graph remain unchanged. The build enables
`portal-input` and retains the release's Rust toolchain. Cua's MIT and the font's
OFL notices ship in `/usr/share/licenses/cua-driver/`.

The initial image build now also compiles Cua; subsequent builds reuse its own
Cargo cache. Existing containers require recreation from the new image to enable
the overlay and use the embedded Chinese font. This change does not deploy or
replace production computers automatically.

## Verification

- Workspace tests: 205 passed; the separate saved-login integration test is ignored
  by the unit-test suite.
- Workspace Clippy with warnings denied, Rust formatting, and launcher shell syntax.
- Two standalone color-module tests passed (hex validation, black/white, live
  refresh, missing-file fallback, and isolation between display files).
- Complete desktop smoke passed, including native/browser input, terminal and
  clipboard Unicode, dual-display isolation, session recovery, browser-cookie
  persistence through restart, and the named-cursor test.
- Saved-login integration passed against real HTTPS fields without submission.
- English and Chinese labels were visually verified in desktop screenshots.
  A separate read-only VNC framebuffer capture also showed the Chinese name and
  synthetic pointer in the actual VNC stream, separate from the OS pointer.
- VNC pixel assertions found the exact selected RGB values `#8B5CF6`, `#22C55E`,
  and `#E11D48` in the cursor while preserving the daemon PID and session name.
- An isolated API/supervisor/database integration test verified that persisted
  `avatarColor` reaches a newly booted screen, PATCH updates the running cursor,
  and editing a stopped agent does not start its computer. Test resources were removed.

Verified locally on 2026-09-08, Linux arm64. Image:
`lazyboy/computer:cua-cursor-color`,
`sha256:5c6e7f6befa7d9b69d6b6ff56a29692dd08854ad1273e939020ac06370ee85e6`.
Production services and existing application computers have not been replaced.
Deploy the updated API/supervisor and recreate desktops from this image together.

Color verification: `/tmp/lazyboy-cursor-color-tests.log`,
`/tmp/lazyboy-cursor-color-clippy.log`, `/tmp/lazyboy-cursor-color-smoke.log`,
`/tmp/lazyboy-cursor-color-api-test.log`, and
`/tmp/lazyboy-cursor-color-smoke/cua-cursor-green.png`.
The preceding name/font verification and saved-login run remain in
`/tmp/lazyboy-cursor-smoke.log` and `/tmp/lazyboy-cursor-login.log`.
