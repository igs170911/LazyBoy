# End-to-end interaction and memory audit

Active user objective (2026-09-08): improve the whole input → agent → Cua → shared
screen → reply flow, make the real operating cursor visible, and make agent memory
reasonable and inspectable. This is not complete until runtime and UI evidence
support all of these requirements.

## Acceptance requirements

- [ ] Measure input acknowledgement, preparation/recall, model wait, tool dispatch,
  Cua execution, screen update, and response delivery using a representative run.
- [ ] Fix unnecessary serial work and waits without replaying uncertain actions.
- [ ] Verify the named, selected-color cursor in the user's actual operating environment,
  including browser actions, typing, and reconnect/restart (not just a fixture pixel click).
- [ ] Verify the interaction feels continuous and reports what it is waiting for.
- [ ] Memory recall must not block on model download or inject unrelated memories.
- [ ] Verify useful Chinese/mixed-language recall, scope isolation, corrections,
  deletion, duplicates/conflicts, and provenance.
- [ ] Show stored memories and the specific memories used by a run, with editing,
  source/time information, and accurate enabled/indexing/error state.
- [ ] Verify the rendered memory interface and actual complete conversation/tool flows.

## Current evidence

- Worktree began clean at `17926f1`; earlier cursor/color changes are committed.
- The currently running local bot desktop still uses `lazyboy/computer:cua-work`;
  named/color cursor verification had used a separate image, not this running desktop.
  User environment clarification is pending; do not equate a local fixture with it.
- `MemoryService::embed` lazily downloads/initializes a model under a mutex and waits
  for completion before recall/save. There is no latency budget or admission limit.
- Memory recall always fills top-k, even with zero lexical/semantic relevance.
- Current model is AllMiniLML6V2; Chinese retrieval quality needs direct validation.
- Browser navigation has an unconditional 800 ms sleep plus a new snapshot. Additional
  native-window, focus, snapshot and ensure calls need timings before changing them.
- Current MemoryPane lists/edits/deletes memories but does not show retrieval use,
  indexing state, provenance or a live refresh after agent memory writes.

## Research

- Cua Linux standalone Chromium supports explicit DOM input; background trusted input
  has platform limits. Foreground input is a distinct route. Preserve route failures:
  https://cua.ai/docs/reference/cua-driver/platform-support
  https://cua.ai/docs/concepts/browser-targeting-and-background-delivery
- Current embedding model reference:
  https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2

## Verification log

2026-09-08, local disposable resources only:

- Bounded embedding inference/warmup implemented: recall/save spends at most 200 ms
  waiting for an embedding worker; a timed-out worker retains its single permit so
  subsequent requests do not queue behind a model download. Model quality, retry,
  indexing and relevance filtering remain open.
- Four API memory tests pass, including a blocked-worker latency test and a real
  PostgreSQL scope test. Migration 016 permits room members to link shared source
  conversations while rejecting other private conversations and removed members.
- MemoryPane now remounts per agent, ignores superseded list results, refreshes every
  five seconds while visible and on window focus, and shows source/revision/times.
  Message bookmarks now retain sourceMessageId. TypeScript check passes; rendered
  UI, group destination selection and run-level memory use are not yet verified.
- Pinned Cua Linux implements no browser visual-feedback hook, although browser
  engine computes the target. Added a Linux implementation sharing the cursor
  registry, guarded by live tab visibility and valid coordinates. Test image
  `lazyboy/computer:flow-audit` (7f7fe94dfe4d) built before the finite-coordinate
  follow-up. Chromium DOM click now shows the Chinese named green cursor.
- Three local browser fixture rounds: navigation 880–919 ms, click 342–586 ms,
  type 649–712 ms. These measure controller requests, not a complete model run.
  Typing cursor position is still under investigation; screenshot success alone
  is insufficient. Linux get_agent_cursor_state always returns null position in
  this upstream version, so use framebuffer evidence instead.
- A root-run diagnostic rewrote disposable screen name/color files as root-only;
  repaired ownership and reran as desktop uid 1000. This was a test harness issue,
  not evidence about the application launch path. No production resources changed.


- Isolated type-only probe, rerun as uid 1000, passes and shows the green Chinese
  cursor centered over the input (`/tmp/type-only.png`), while the physical X11
  pointer remains at (640,400). Earlier sequential screenshot discrepancy still
  needs reproduction; do not conclude a permanent geometry defect from it.

## Follow-up implementation and runtime evidence

- Lexical fallback now requires an actual full-text match, instead of returning
  recent unrelated rows. Database tests cover no-match and private agent isolation.
  Semantic filtering and Chinese/mixed-language relevance remain unverified.
- Durable context returns the exact injected memory IDs/revisions. Oversized items
  are skipped so later short items can fit; an empty selection emits no empty
  memory block. The run activity records IDs/revisions, candidate count and recall
  duration, and its UI shows the included count. Detailed used-memory inspection
  remains to be connected.
- `/memories/status` reports global enablement, embedding availability and indexed
  count. A background indexer catches up NULL embeddings in small batches, writing
  only if the item is still active at the same revision with no newer embedding.
  Revision mismatch and duplicate-index-write guards are tested.
- Isolated API on 127.0.0.1:3112, database `lazyboy_flow_audit`, was created for
  rendered verification. No model-provider credentials are configured. The initial
  missing ONNX dylib caused a worker panic and lexical fallback, correctly shown
  as unavailable. After configuring the downloaded runtime from the repository
  script and restarting only this fixture API, the pre-existing NULL embedding was
  indexed and the visible pane updated to 1/1 ready without a page reload.
- Rendered source/time panel: `/tmp/lazyboy-memory-ui-ready.png`. Fixed an existing
  unstyled danger-ghost button which appeared white-on-white in dark mode.
- Actual HTTP screenshot requests sometimes reset when a client sends `{}` and
  closes the connection. The controller observe handler now drains optional request
  bytes before returning the screenshot. The v2 image passes 20 consecutive
  non-retried screenshots of at least 1,444,848 bytes, median 60.5 ms, maximum 74 ms.
  Reproducer: `scripts/cua-observe-http-test.py`; log `/tmp/lazyboy-observe-http-v2.log`.
- `lazyboy/computer:flow-audit-v2` includes the body-drain and finite-coordinate
  fixes. Its local disposable container is `lazyboy-flow-audit-v2`. Existing app
  computers and production still use their previous images.
- A complex application-panel click displayed the named cursor near the top of
  the desktop rather than over the clicked disclosure. The target action itself
  succeeded. Browser overlay placement needs instrumentation and regression checks;
  do not claim cursor correctness from the simple fixture alone.

- Workspace tests passed before adding the explicit model-quality test (now 81
  API tests total, including the new opt-in quality test). Frontend build/typecheck
  also pass. The explicit ONNX model test FAILS: AllMiniLML6V2 selects the wrong
  topic in 3/6 Chinese/cross-language cases (coffee and transportation), despite
  successful vector generation. Full scores: `/tmp/lazyboy-memory-quality.log`.
  `semantic_memory_distinguishes_chinese_and_cross_language_topics` is ignored by
  default because it needs the runtime/model download, and must pass explicitly
  before memory quality is considered fixed. Next: multilingual model migration
  with model-version tagging, query/passage prefixes and a measured relevance gate.

- Old-image comparison now conclusively reproduces ConnectionResetError on the
  same large-image fixture (`/tmp/lazyboy-observe-http-old.log`). The fixture was
  changed to ThreadingHTTPServer: Chromium idle/preconnect sockets could otherwise
  stall single-threaded fixture cleanup after an error. The old stuck test process
  was terminated, the corrected fixture was run, and exited 1 with the reset.
- Multilingual-E5-small model author reference (384 dimensions, query/passage
  prefixes required even outside English, score ranges differ from MiniLM):
  https://huggingface.co/intfloat/multilingual-e5-small/raw/main/README.md

## Multilingual memory and inspection verification

- Compared models on the same six Chinese/cross-language topic queries. Original
  AllMiniLML6V2 chose 3/6 correctly; multilingual E5 small 5/6 and E5 base 4/6.
  The selected `ParaphraseMLMiniLML12V2` chose 6/6 correctly, with target similarities
  0.508–0.715 and other-topic scores at most 0.237. Four unrelated queries score
  at most 0.284. The model-specific 0.4 gate rejects these unrelated cases.
  Explicit quality test now passes. Logs: `/tmp/lazyboy-memory-paraphrase-quality.log`
  (selected), `/tmp/lazyboy-memory-e5-quality.log`, `/tmp/lazyboy-memory-e5-base-quality.log`.
- Migration 017 labels existing vectors as legacy. Recall never compares a legacy
  vector to a new-model query; indexer replaces legacy vectors at the same content
  revision, then stores the new model ID. Final schema stays 384-dimensional.
  The E5/768-dimensional experiment was not applied to the fixture app database.
- Actual fixture API restart migrated its existing memory and reindexed it to
  `paraphrase-multilingual-MiniLM-L12-v2:plain:v1` while preserving its ID, content
  and revision 1. No production database or application computer was changed.
- Creates use a per-agent transaction advisory lock and full content equality to
  collapse simultaneous identical saves. Different agents retain separate items;
  deletion permits a later intentional fresh save. Tests cover these cases.
- Database tests cover vector relevance rejection, legacy-model exclusion/reindex,
  revision-safe writes and per-agent scope. Embedding acquisition can wait briefly
  behind an index job, with queue and inference sharing the same total time budget;
  no extra blocking workers are queued behind a download.
- Added exact historical memory inspection to run activity. The endpoint scopes
  both the run and content to the actor/agent, reads the referenced revision, and
  suppresses content from deleted memories. SQL tests cover historical text after
  edits, cross-agent reference injection, wrong actor/run, and deletion.
- Every assistant reply with a run ID now has an execution/memory link. Completed
  runs show completed state. Popover placement follows available screen space and
  expanded height, avoiding clipping above the viewport. Rendered fixture evidence:
  `/tmp/lazyboy-memory-used-ui-fixed.png`. This uses a manually seeded completed
  run for UI verification, not a claim of a full real-provider conversation test.
- Group user-message bookmarks now ask which agent should store the memory. The
  previously hidden bookmark button is visible. A real UI click saved the fixture
  group message only to its selected non-owner agent, retaining sourceMessageId
  and sessionId; the thread-owner agent's list stayed unchanged. Screenshots:
  `/tmp/lazyboy-memory-choose-agent.png`, `/tmp/lazyboy-group-memory-panel.png`.
- Memory pane has a stable explicit group-agent selector, independent of whichever
  agent is currently busy. Switching to a private agent showed only that agent's
  memory and owner label in the live UI.
- Remaining memory concerns: richer Chinese lexical fallback while embeddings are
  unavailable, transient model-failure retry, contradictory facts/correction tool
  behavior, and long memories exceeding model/context limits. End-to-end reply
  streaming, delay measurement and browser-cursor placement remain open.
- Selected model's author documentation:
  https://huggingface.co/sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2

### Cursor motion verification (v4)

- Traced the apparent browser coordinate error to unfinished overlay animation.
  Cua supplied correct toolbar coordinates. With the old motion, the 250 ms
  deadline expired while a wide curved path was still running; ClickPulse did
  not cancel that path, so subsequent frames moved the cursor off target.
- Configure the supported per-session motion API with a 120 ms glide, 8 px
  turn radius and 40 ms click dwell. Configuration is cached by session and
  daemon socket timestamp, and reapplied after session revival. Revival now
  preserves an explicitly supplied session label.
- First browser target reveals directly from the unknown-position sentinel.
  Every browser visual ends with Cua's shared hotspot-aware SnapTo command,
  cancelling any residual path/spring after the bounded animation wait.
- Real fixture toolbar sequence (Memory, Account, Memory) now reports all three
  arrivals successful. The next operation's starting position matches the prior
  target plus Cua's documented-in-source artwork offset; the old trace drifted
  away between operations. Screenshot `/tmp/lazyboy-cursor-diagnostic-v4.png`
  visibly places the purple cursor on the Memory button. Diagnostic logs contain
  coordinates only and require `LAZYBOY_CURSOR_DIAGNOSTICS` to be set.
- Built image `lazyboy/computer:flow-audit-v4` (58969e8d01ee). Control tests: 84
  passed; control Clippy with warnings denied and frontend TypeScript passed.
- Screenshot transport on v4: 20/20 complete PNGs, minimum 1,444,743 bytes,
  median 57.5 ms, maximum 64 ms. Added the transport regression to the image and
  smoke script; this packaging addition follows the v4 build.
- Full v4 smoke completed with exit 0: browser/native actions, terminal and
  Unicode clipboard, two-display isolation, expired-session recovery without
  replaying mutations, cookies across restart, cursor rename and live color
  updates (#8B5CF6, #22C55E, #E11D48). The transport test was copied into the
  disposable smoke container for this run and passed there too (20/20).
  Evidence: `/tmp/lazyboy-flow-smoke-v4.log` and smoke artifact directory.
- Transcript refresh now starts independently of desktop discovery and applies
  the message result immediately, retaining the stale-request guard. Runtime
  delayed-desktop verification is still pending, as are input retry/outbox and
  streamed response work. These fixture results do not prove production rollout
  or a full model-provider conversation.
- Message submission now retains text/files until POST acknowledgement and shows
  a sending animation. In-memory pending nonces are scoped to session, text and
  attachment identities, so retrying an uncertain response reuses the nonce.
  New input typed during the request is preserved; a subsequent refresh failure
  cannot restore already accepted attachments/text. TypeScript passes. An
  isolated execution harness of the actual send handler verified lost-response
  retry, fresh nonce after acknowledgement, concurrent edits and refresh failure
  (`/tmp/lazyboy-send-recovery-test.mjs`). Browser fault-injection and durable
  reload recovery remain unverified; this is not yet a persistent outbox.
- Run-loop model requests now consume Rig's streaming response for all three
  configured model variants. Only public text is emitted as batched durable SSE
  deltas (150 ms timer); reasoning and partial tool arguments stay out of the UI.
  Each attempt starts a new generation; failed attempts reset their draft. Tool
  execution still receives the fully assembled choice only after a terminal
  response, and early EOF is an error. The existing halt select cancels streaming.
- Frontend renders separate per-run text drafts and handles retry generations,
  replay deduplication, parallel agents, pause, final-message and session-clear
  events. `scripts/reply-stream-test.mjs` covers these state transitions. Rust
  collector test verifies Unicode assembly and rejection of a truncated stream.
  TypeScript and API compilation pass. Real provider HTTP streaming, rendered
  browser/reconnect behavior and event retention cost still require validation;
  this does not establish full end-to-end completion.
- Actual isolated API worker -> local OpenAI-compatible HTTP fixture -> durable
  SSE -> final message run passed. The fixture received `stream:true`; first text
  arrived at 0.200 s and completion at 2.477 s. No GUI tools were requested for
  the greeting. Logs: `/tmp/lazyboy-stream-e2e.log`, provider request log, and
  `/tmp/lazyboy-stream-events.json`. This is a controlled provider fixture, not
  an external production model. Fixture API 3112 was rebuilt/restarted; only its
  `lazyboy_flow_audit` database model settings point to local fixture port 3113.
- Real Chromium via Cua displayed the partial Chinese text and later the full
  answer: `/tmp/lazyboy-stream-partial.png`, `/tmp/lazyboy-stream-final.png`.
  Inspection caught a draft/final layout shift; drafts now use the same message
  bubble classes (that styling adjustment still needs fresh rendered verification).
- Final text now remains until its exact message ID arrives in the transcript,
  avoiding an empty gap on a slow fetch. User steering messages no longer clear
  an in-progress draft. Extended reducer tests pass. Reload replay of historical
  generations and interrupted-server cleanup remain open.
- Reconnect replay now filters text events to the running run's current
  `replyGeneration`, stored in its checkpoint before each attempt. Actual HTTP
  tests passed for active fresh-load prefix restoration, Last-Event-ID suffix
  delivery without duplication, and omission of completed historical drafts:
  `/tmp/lazyboy-stream-reconnect.log`.
- A local provider deliberately closed its first HTTP stream after a partial
  Chinese chunk. The real worker emitted reset, retried with a different
  generation, and stored exactly one complete assistant reply with no failed
  prefix. Evidence: `/tmp/lazyboy-stream-retry-e2e.log` and retry event JSON.
- Cancellation previously changed DB state without a session event. Both
  session and bot cancellation now emit `run.cancelled`; the UI subscribes and
  removes the draft. Frontend reducer tests include cancellation.
- Actual HTTP stop test passed: cancellation event arrived within one second;
  after the provider would have finished, no assistant message was stored.
  Evidence: `/tmp/lazyboy-stream-stop.log`.
