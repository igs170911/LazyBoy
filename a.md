# LazyBoy → Cua Driver Migration Plan

> 2026-09-07 檢查：Phase 1 規格尚未全部勾完（生產預設仍是 legacy；takeover／錄製端到端未另開測）。opt-in Cua 已可在現有桌面容器使用，驗收見 [docs/cua-review.md](docs/cua-review.md)。本文件仍是目標規格，不能視為完成證明。

> **Purpose:** This document is an implementation specification for a coding agent.
>
> Repository: `https://code.30cm.net/daniel.w/lazyBoy`
>
> Cua: `https://github.com/trycua/cua`
>
> Cua docs: `https://cua.ai/docs`
>
> **Main principle:** Do **NOT** rewrite LazyBoy into Cua. Keep LazyBoy as the agent/workspace/product layer and replace the low-level computer-control implementation with Cua Driver incrementally.

---

## 0. Agent Instructions

You are modifying **LazyBoy**, a self-hosted AI agent workspace.

The goal is to migrate the low-level GUI/browser computer-control implementation from custom CDP / AT-SPI / X11 code to **Cua Driver**, without breaking LazyBoy's existing product architecture.

### Non-negotiable rules

1. **Do not perform a Big Bang rewrite.**
2. **Do not remove the legacy computer-control implementation in the first PR.**
3. Keep LazyBoy's current public Agent tool schema stable unless absolutely necessary.
4. Keep:
   - `computer_observe`
   - `computer_act`
   - `browser`
   - `shell`
   - file tools
   - takeover flow
   - memory
   - schedule
   - vault
   - skills/playbooks
5. Cua must initially be an **implementation detail behind LazyBoy abstractions**.
6. Do not expose all Cua MCP tools directly to the LLM.
7. Do not replace `DockerSandbox`, `Supervisor`, noVNC, or persistent bot homes during Phase 1.
8. Add a feature flag / backend selector so legacy behavior can be restored immediately.
9. Every migrated action must have observable verification.
10. Do not guess Cua API/tool names from this document.

### Cua API freshness rule

Cua changes quickly.

Before implementing anything:

```bash
cua-driver --version
cua-driver doctor
cua-driver list-tools
```

For every Cua tool you plan to call:

```bash
cua-driver describe <tool-name>
```

Use the schema reported by the installed Cua Driver version as the source of truth.

Do not hard-code assumptions from old blog posts, old examples, or this document when the installed version differs.

---

# 1. Current LazyBoy Architecture

The current architecture is approximately:

```text
User
 │
 ▼
React Web UI
 │
 ▼
crates/api
 │
 ├── Agent loop
 ├── Sessions / Runs
 ├── Memory
 ├── Schedule
 ├── Vault
 ├── Skills
 ├── MCP
 └── Tool definitions
 │
 ▼
SandboxProvider
 │
 ▼
crates/sandbox
 │
 ▼
DockerSandbox
 │
 ▼
crates/supervisor
 │
 ▼
Linux Desktop Container
 │
 ├── XFCE
 ├── Chromium
 ├── Xvfb
 ├── x11vnc
 ├── websockify
 └── controld
       │
       ├── CDP
       ├── AT-SPI
       ├── X11
       └── screenshots
```

Relevant current components:

```text
apps/web
crates/api
crates/contracts
crates/control
crates/controld
crates/harness
crates/sandbox
crates/supervisor
```

The existing `SandboxProvider` abstraction is an important migration seam and should be preserved.

Current methods include approximately:

```rust
async fn provision(...)
async fn prepare(...)
async fn capabilities(...)
async fn ensure_screen(...)
async fn reconnect(...)
async fn suspend(...)
async fn resume(...)
async fn execute(...)
async fn observe(...)
async fn act(...)
async fn connect_screen(...)
async fn list_files(...)
async fn read_file(...)
async fn write_file(...)
async fn stop(...)
async fn destroy(...)
```

Do not collapse this abstraction.

---

# 2. Target Architecture

Phase 1 target:

```text
                         LazyBoy Web
                              │
                              ▼
                         LazyBoy API
                              │
                       LazyBoy Agent Loop
                              │
             ┌────────────────┼────────────────┐
             │                │                │
             ▼                ▼                ▼
          Memory            Skills          Schedule
                              │
                              ▼
                       LazyBoy Tool Layer
                              │
                  computer / browser / shell
                              │
                              ▼
                    ComputerController
                       abstraction
                         │       │
             ┌───────────┘       └────────────┐
             ▼                                ▼
     LegacyController                  CuaController
             │                                │
      CDP / AT-SPI / X11                 Cua Driver
                                               │
                                ┌──────────────┼──────────────┐
                                ▼              ▼              ▼
                              X11           AT-SPI         Browser
                                               │
                                               ▼
                                      Existing Desktop
                                         Container
```

### Important

Cua is initially used as the **computer-control backend**.

It is **not** the Agent runtime.

It is **not** the memory system.

It is **not** the scheduler.

It is **not** the user-facing product.

It is **not** the first-phase sandbox lifecycle owner.

---

# 3. What Must Stay in LazyBoy

The following are LazyBoy product responsibilities and must remain owned by LazyBoy.

## Keep unchanged unless required

### Frontend

```text
apps/web
```

Keep:

- Chat UI
- remote desktop
- noVNC
- mobile controls
- takeover UI
- bot management
- session UI

### API / Agent layer

```text
crates/api
```

Keep:

- Agent loop
- model interaction
- run management
- session management
- tool policy
- takeover workflow
- credential flow
- memory
- schedules
- skills
- MCP integrations

### Harness

```text
crates/harness
```

Keep model-provider support.

Cua Driver must not decide which model LazyBoy uses.

### Supervisor

```text
crates/supervisor
```

Phase 1: keep current behavior.

Keep:

- Docker lifecycle
- pause/resume
- resource limits
- persisted bot homes
- screen lifecycle

### Sandbox

```text
crates/sandbox
```

Phase 1: keep `DockerSandbox`.

Do **not** replace it with Cua Sandbox yet.

---

# 4. What Cua Should Replace

The long-term goal is to reduce LazyBoy-owned OS automation.

Current files that are candidates for replacement or simplification:

```text
crates/control/src/a11y.py
crates/control/src/a11y.rs

crates/control/src/cdp.py
crates/control/src/cdp.rs

crates/control/src/x11.rs
crates/control/src/screen.rs
crates/control/src/overlay.rs
```

Do not delete them during the initial migration.

Instead place them behind a legacy backend.

The responsibilities that should gradually move to Cua:

- screenshots
- application/window enumeration
- accessibility tree
- native UI element actions
- pointer input
- keyboard input
- scrolling
- browser state
- browser semantic actions
- window-scoped control
- recording / trajectory capture

---

# 5. Do NOT Expose Raw Cua to the LLM

LazyBoy currently has a relatively compact Agent-facing tool surface.

Preserve this.

Preferred model:

```text
LLM
 │
 ▼
LazyBoy tools
 │
 ├── computer_observe
 ├── computer_act
 ├── browser
 ├── shell
 ├── file tools
 └── request_takeover
 │
 ▼
LazyBoy policy / safety / vault / state
 │
 ▼
Cua adapter
 │
 ▼
Cua Driver
```

Avoid this:

```text
LLM
 │
 ▼
50+ raw Cua tools
 │
 ▼
OS
```

Reasons:

- larger tool schemas consume context
- LazyBoy loses policy control
- LazyBoy loses stable abstraction
- Cua version changes would leak into prompts
- takeover behavior becomes harder to control
- credential handling becomes harder to constrain
- Agent behavior becomes coupled to Cua implementation details

Cua should initially behave like a device driver.

---

# 6. Introduce a ComputerController Abstraction

Create a low-level computer-control abstraction separate from sandbox lifecycle.

Suggested location:

```text
crates/control/src/controller.rs
```

Possible interface:

```rust
#[async_trait::async_trait]
pub trait ComputerController: Send + Sync {
    async fn health(&self, ctx: &ControlContext)
        -> Result<ControllerHealth, ControlError>;

    async fn observe(
        &self,
        request: ObserveRequest,
        ctx: &ControlContext,
    ) -> Result<ComputerObservation, ControlError>;

    async fn act(
        &self,
        request: ActionRequest,
        ctx: &ControlContext,
    ) -> Result<ActionResult, ControlError>;

    async fn browser(
        &self,
        request: BrowserRequest,
        ctx: &ControlContext,
    ) -> Result<BrowserResult, ControlError>;

    async fn start_recording(
        &self,
        request: RecordingRequest,
        ctx: &ControlContext,
    ) -> Result<RecordingSession, ControlError>;

    async fn stop_recording(
        &self,
        ctx: &ControlContext,
    ) -> Result<RecordingResult, ControlError>;
}
```

Names may be adjusted to match existing LazyBoy contracts.

Do not introduce unnecessary abstractions if equivalent types already exist.

---

# 7. Backend Implementations

Implement:

```text
LegacyController
CuaController
```

Suggested layout:

```text
crates/control/src/
├── controller.rs
├── legacy/
│   ├── mod.rs
│   ├── a11y.rs
│   ├── cdp.rs
│   ├── x11.rs
│   └── screen.rs
└── cua/
    ├── mod.rs
    ├── client.rs
    ├── translate.rs
    ├── observe.rs
    ├── actions.rs
    ├── browser.rs
    └── recording.rs
```

Do not spend the first PR moving all old files if it creates noisy diffs.

It is acceptable to initially keep existing file locations and only add:

```text
controller.rs
cua.rs
```

Refactor structure after behavior is stable.

---

# 8. Backend Configuration

Add a configuration value:

```bash
LAZYBOY_COMPUTER_DRIVER=legacy
```

or:

```bash
LAZYBOY_COMPUTER_DRIVER=cua
```

Default during initial rollout:

```bash
LAZYBOY_COMPUTER_DRIVER=legacy
```

After Cua passes production-equivalent validation, the default may become:

```bash
LAZYBOY_COMPUTER_DRIVER=cua
```

Do not remove the legacy option until Cua has passed the migration acceptance suite.

If configuration already has a typed settings system, add this there instead of reading environment variables throughout the code.

Suggested enum:

```rust
pub enum ComputerDriver {
    Legacy,
    Cua,
}
```

---

# 9. Recommended Cua Integration Mode

LazyBoy is an application with built-in computer-use.

Preferred order for experimentation:

## POC

Use the Cua Driver CLI / daemon boundary first if it allows fast validation.

Examples:

```bash
cua-driver call <tool> '<json>'
```

or a controlled local process interface.

This proves Cua works inside the existing Linux desktop container.

## Production integration

Prefer a typed/stable integration boundary.

Evaluate, based on the installed Cua release:

1. direct in-process SDK if suitable for LazyBoy's process model
2. private worker / daemon
3. CLI JSON calls as fallback

Do not choose MCP merely because Agent frameworks often use MCP.

LazyBoy is embedding computer use inside a product. The internal integration does not have to look like the Agent-facing integration.

### Rust note

Cua Driver's core runtime is Rust-based, but public application SDK support may differ by release.

Do not write custom unsafe bindings unless there is a strong reason.

Prefer an officially supported application integration surface.

---

# 10. Phase 0 — Discovery / Compatibility Check

Before changing LazyBoy behavior, create a short engineering report.

The coding agent must verify the actual runtime environment.

Inside a real LazyBoy desktop container:

```bash
echo "$DISPLAY"
ps aux
env | sort
```

Confirm:

- X11 display exists
- XFCE session exists
- AT-SPI bus is available
- Chromium exists
- Cua Driver can start
- Cua Driver can capture the current display
- Cua Driver can enumerate apps/windows
- Cua Driver can operate in Xvfb
- Cua Driver can access Chromium in this environment

Install Cua Driver using the current official installation method.

Then run:

```bash
cua-driver --version
cua-driver doctor
cua-driver list-tools
```

Save results in:

```text
docs/cua-compatibility.md
```

Include:

- installed Cua version
- Linux distribution
- display server
- Cua doctor output summary
- supported Cua tools relevant to LazyBoy
- missing dependencies
- known limitations

### Stop condition

If Cua cannot reliably control the existing `XFCE + Xvfb` environment, do **not** proceed with architecture replacement.

Instead document the blocker first.

---

# 11. Phase 1 — Five-Action POC

Do not begin by migrating the whole control crate.

First prove these five capabilities inside the existing LazyBoy desktop:

1. screenshot
2. accessibility/window observation
3. click native GUI element
4. type text
5. browser semantic action

Optional sixth:

6. scroll

Create a standalone POC path.

Example:

```text
scripts/cua-smoke-test.sh
```

or:

```text
crates/control/examples/cua_smoke.rs
```

### POC flow

Suggested test:

```text
1. Open a native/simple XFCE application.
2. Observe window state.
3. Click an accessible element.
4. Type text.
5. Launch/use Chromium.
6. Navigate to example.com.
7. Read browser state.
8. Trigger a semantic browser interaction if available.
9. Capture final screenshot.
```

### POC success criteria

All five core actions succeed 10 consecutive times without:

- wrong window actions
- stale element actions
- focus corruption
- unexplained timeouts
- leaving Cua processes behind
- breaking noVNC control

Do not migrate production paths until this passes.

---

# 12. Phase 2 — Migrate `computer_observe`

First production migration target:

```text
computer_observe
```

Current LazyBoy observation must remain compatible with the Agent.

Do not change the Agent prompt contract unless required.

Cua result must be translated into:

```rust
ComputerObservation
```

Preserve existing concepts where possible:

- screenshot/image
- dimensions
- cursor
- active window
- UI elements

### Translation layer

Implement:

```text
Cua state
   ↓
CuaObservationAdapter
   ↓
LazyBoy ComputerObservation
```

Do not expose Cua-native short-lived element identifiers directly as permanent LazyBoy identifiers.

Element references may only be valid for a specific observation.

Treat each observation as a snapshot.

### Observation ID

Strongly consider adding or preserving an observation/snapshot identifier.

Example:

```rust
pub struct ComputerObservation {
    pub observation_id: String,
    ...
}
```

If changing the contract is too invasive, keep this internal first.

This will later help detect stale actions.

---

# 13. Phase 3 — Migrate `computer_act`

Preserve the existing LazyBoy `ComputerAction` DSL.

Current behavior such as:

- click
- move
- pointer down/up
- hover
- drag
- type
- keyboard
- wait
- semantic references

should remain Agent-facing LazyBoy concepts.

Create a translator:

```text
LazyBoy ComputerAction
          │
          ▼
   CuaActionTranslator
          │
          ▼
       Cua Driver
```

Example conceptual mapping:

```text
LazyBoy click(element)
    → semantic Cua action when possible

LazyBoy click(x, y)
    → pixel Cua action

LazyBoy type(text)
    → Cua text input

LazyBoy key(...)
    → Cua key action

LazyBoy scroll(...)
    → Cua scroll
```

Exact tool names MUST come from:

```bash
cua-driver list-tools
cua-driver describe ...
```

### Preserve semantic-first behavior

Prefer:

```text
Accessibility / browser semantic action
```

over:

```text
screen coordinate
```

Coordinate action should be fallback, not default.

---

# 14. Preserve LazyBoy Action Safety Logic

Do not delete useful behavior from:

```text
crates/control/src/actions.rs
```

Existing logic includes concepts such as:

- action batch limits
- coordinate validation
- element lookup
- stale click prevention
- browser semantic-routing preference
- double-click expansion
- drag normalization

These should become policy/translation logic above Cua.

Desired layering:

```text
Agent request
      │
      ▼
LazyBoy validation
      │
      ▼
LazyBoy action policy
      │
      ▼
Cua translation
      │
      ▼
Cua Driver
```

Cua is not a replacement for LazyBoy product policy.

---

# 15. Stale Element Handling

This is critical.

Do not assume:

```text
element 12
```

from one Cua observation refers to the same UI element later.

The migration must treat element references as snapshot-scoped.

Preferred flow:

```text
observe
  ↓
snapshot A
  ↓
Agent selects element
  ↓
act against snapshot A
  ↓
UI changes
  ↓
observe again
  ↓
snapshot B
```

If the UI changed materially, do not retry an old semantic reference blindly.

### Retry rule

When an action fails:

1. re-observe
2. re-resolve the target
3. retry with bounded count

Never loop the same stale action indefinitely.

---

# 16. Phase 4 — Migrate Browser Control

LazyBoy currently separates browser actions from generic computer actions.

Keep that separation for the Agent.

Agent-facing:

```text
browser
```

Internal:

```text
LazyBoy BrowserRequest
        │
        ▼
Cua Browser Adapter
        │
        ▼
Cua Driver browser tools
```

Cua browser control should replace custom CDP behavior gradually.

### Important

Preserve the current policy:

```text
When semantic browser state is available,
prefer browser semantic actions over pixel clicking Chromium.
```

Do not make browser automation less reliable during migration.

### Test cases

At minimum test:

- navigate URL
- inspect page
- click semantic element
- type into input
- scroll
- multiple tabs if LazyBoy currently depends on them
- file picker transition
- page refresh
- browser restart
- authenticated persistent profile

---

# 17. Browser Profile Compatibility

LazyBoy persists browser profiles per computer/bot.

This is product-critical.

Do not let Cua silently replace LazyBoy's profile with an ephemeral managed browser unless explicitly intended.

Verify:

- existing Chromium profile path remains usable
- cookies survive container pause/resume
- login sessions survive LazyBoy restart behavior as expected
- takeover user and Agent see the same session
- Cua attaches to the correct browser/window

If Cua requires an explicit browser preparation/attach step, integrate that into LazyBoy lifecycle.

Do not auto-create a separate profile that breaks existing saved logins.

---

# 18. Phase 5 — Recording Integration

LazyBoy has an important feature:

```text
Human demonstration
        ↓
record context/actions
        ↓
model generates playbook
        ↓
future run resolves current UI
```

Preserve this architecture.

Do **not** downgrade it into raw coordinate replay.

Cua recording should be used as richer source data.

Target:

```text
Human / Agent demonstration
           │
           ▼
      Cua trajectory
           │
           ├── before state
           ├── action
           ├── after state
           ├── screenshots
           └── optional video
           │
           ▼
  LazyBoy Skill Compiler
           │
           ▼
     Semantic Playbook
```

Cua trajectory is evidence.

LazyBoy skill/playbook is the reusable automation.

### Do not do

```text
record x=312,y=441
replay x=312,y=441 forever
```

### Do

Store semantic intent when possible:

```text
Click the "Sign in" button
```

Then resolve it on the current screen during replay.

---

# 19. Takeover Must Keep Working

User takeover is a core LazyBoy feature.

Migration acceptance requires:

```text
Agent running
   ↓
request_takeover
   ↓
Agent stops issuing input
   ↓
User controls noVNC desktop
   ↓
User releases takeover
   ↓
Agent re-observes
   ↓
Agent resumes from current state
```

### Required rule

After takeover ends:

**Always perform a fresh observation before the Agent performs another UI action.**

Never reuse pre-takeover element references.

---

# 20. Credential / Vault Boundary

Cua must not get broad access to LazyBoy secrets by default.

Keep credential policy in LazyBoy.

Preferred flow:

```text
Agent wants login
     │
     ▼
LazyBoy checks:
- allowed domain
- saved credential exists
- HTTPS / policy
     │
     ▼
LazyBoy authorizes injection
     │
     ▼
Cua performs allowed typing/action
```

Do not make the Cua integration read the entire Vault.

Secrets should not appear:

- in command-line arguments
- in logs
- in Cua debug output
- in trajectory metadata
- in screenshots longer than unavoidable
- in error messages

Audit recording behavior around passwords.

If recording is active, ensure sensitive typing can be masked or recording paused.

---

# 21. `controld` Migration Strategy

Do not delete `crates/controld` during Phase 1.

It is a useful compatibility boundary.

Current conceptual API:

```text
POST /observe
POST /act
```

Recommended first migration:

```text
POST /observe
    ↓
selected ComputerController
    ↓
CuaController or LegacyController
```

```text
POST /act
    ↓
LazyBoy validation
    ↓
selected ComputerController
    ↓
CuaController or LegacyController
```

This keeps:

```text
API
SandboxProvider
Supervisor
```

largely unchanged.

Later, if Cua integration makes `controld` unnecessary, remove it in a dedicated architectural PR.

Do not mix that cleanup into the initial migration.

---

# 22. Docker Image Changes

Add Cua dependencies to the LazyBoy desktop image.

The coding agent must locate the actual Dockerfile(s) used for desktop computers.

Do not assume the path.

Changes may include:

- Cua Driver install
- required X11 packages
- AT-SPI dependencies
- ffmpeg if recording/video is enabled
- runtime directories
- permissions
- PATH configuration

Run:

```bash
cua-driver doctor
```

inside the built desktop container as part of the smoke test.

### Image versioning

Pin a known-working Cua version for reproducible builds.

Do not install uncontrolled nightly builds in the default production image.

Optionally allow:

```bash
LAZYBOY_CUA_CHANNEL=stable
LAZYBOY_CUA_VERSION=<version>
```

or equivalent build args.

---

# 23. Cua Process Lifecycle

Do not start one uncontrolled global Cua process for every LazyBoy computer unless architecture requires it.

Determine which process should own:

- display connection
- accessibility session
- browser connection
- recordings
- Cua lifecycle

For LazyBoy's existing architecture, the safest initial design is usually:

```text
one desktop container
      │
      ├── XFCE/Xvfb
      ├── Chromium
      ├── noVNC
      ├── LazyBoy controld
      └── Cua runtime/driver
```

The driver should only see/control that computer's desktop session.

### Isolation

Bot A must never control Bot B's display.

Tests must explicitly verify isolation.

---

# 24. Health Checks

Add a Cua health check.

Possible information:

```rust
pub struct ControllerHealth {
    pub backend: String,
    pub version: Option<String>,
    pub healthy: bool,
    pub degraded: bool,
    pub details: Vec<String>,
}
```

Surface useful failures:

- driver not installed
- X11 unavailable
- AT-SPI unavailable
- screenshot unavailable
- browser unavailable
- incompatible Cua version

Do not return generic:

```text
computer failed
```

when a meaningful diagnosis is available.

---

# 25. Fallback Behavior

During migration:

```text
LAZYBOY_COMPUTER_DRIVER=legacy
```

must work.

For `cua` mode, avoid silent fallback for individual actions unless explicitly designed.

Bad:

```text
Cua click failed
→ silently xdotool click
```

This makes failures impossible to debug.

Preferred:

```text
Cua action failed
→ return classified error
→ Agent re-observes / retries / requests takeover
```

A feature-flag-level fallback is acceptable.

An invisible per-action fallback is not.

---

# 26. Logging / Observability

Add structured logs around Cua calls.

Include:

- operation ID
- run ID
- bot ID
- screen ID
- backend
- Cua tool
- duration
- success/failure
- action route if Cua reports it
- observation ID if available

Never log secret text.

For typing actions:

```text
text="<redacted>"
length=12
```

not:

```text
text="actual-password"
```

---

# 27. Metrics

If LazyBoy has metrics infrastructure, add:

```text
computer_action_total
computer_action_failed_total
computer_observe_duration_ms
computer_action_duration_ms
computer_browser_action_duration_ms
computer_stale_reference_total
computer_takeover_total
cua_driver_restart_total
```

Useful labels:

```text
backend
action_type
route
result
```

Avoid high-cardinality IDs such as `run_id` as metric labels.

---

# 28. Testing Strategy

## Unit tests

Test translation only.

Examples:

```text
LazyBoy click semantic ref → expected Cua request

LazyBoy coordinate click → expected Cua request

LazyBoy type → expected Cua request

Cua observation → LazyBoy ComputerObservation

unsupported Cua response → classified error
```

Use fake/mocked Cua responses.

---

## Contract tests

The Agent-facing tool results should remain equivalent between:

```text
LegacyController
CuaController
```

for common scenarios.

Test:

- observation shape
- action result shape
- error behavior
- browser results
- takeover interaction

---

## Integration tests

Run against a real desktop container.

Test native GUI:

```text
open app
observe
click
type
verify application state
```

Do not validate only that Cua returned `"ok"`.

Validate independent application state.

---

## Browser E2E

Use a deterministic local test page rather than an external website.

Create fixtures for:

- button
- input
- checkbox
- select
- scroll area
- delayed DOM update
- modal
- new tab
- canvas fallback if needed

Verify actual DOM/application state.

---

# 29. Migration Acceptance Suite

Cua backend is not considered ready until all of the following pass.

## Desktop

- [ ] screenshot works
- [ ] window enumeration works
- [ ] active window works
- [ ] accessibility elements work
- [ ] semantic click works
- [ ] coordinate click works
- [ ] type text works
- [ ] hotkey works
- [ ] scroll works
- [ ] drag works if supported
- [ ] desktop stays usable through noVNC

## Browser

- [ ] attach to correct Chromium
- [ ] use persistent LazyBoy profile
- [ ] navigate
- [ ] observe DOM/browser state
- [ ] semantic click
- [ ] type
- [ ] scroll
- [ ] refresh
- [ ] profile survives pause/resume
- [ ] same browser is visible to human takeover

## Lifecycle

- [ ] fresh container
- [ ] existing persisted container
- [ ] pause
- [ ] resume
- [ ] stop
- [ ] restart
- [ ] concurrent bots
- [ ] no cross-bot control

## Agent flow

- [ ] `computer_observe`
- [ ] `computer_act`
- [ ] `browser`
- [ ] `shell`
- [ ] takeover
- [ ] resume after takeover
- [ ] skill recording
- [ ] scheduled run

---

# 30. Performance Benchmark

Before defaulting to Cua, compare against legacy.

Create a benchmark report:

```text
docs/cua-benchmark.md
```

Measure:

| Scenario             | Legacy | Cua | Winner |
| -------------------- | -----: | --: | ------ |
| screenshot           |        |     |        |
| observe desktop      |        |     |        |
| native click         |        |     |        |
| type text            |        |     |        |
| browser snapshot     |        |     |        |
| browser click        |        |     |        |
| 20-step browser task |        |     |        |

Also measure:

- tool-call count
- bytes returned to model
- screenshot count
- total model-visible observation size
- end-to-end task completion time
- failure rate

The goal is not only faster pointer execution.

The real goal is:

```text
fewer Agent turns
+
less context
+
higher task success rate
```

---

# 31. Rollout Plan

## Step A

Add:

```text
ComputerController
LegacyController
```

No behavior change.

All tests must pass.

---

## Step B

Add Cua smoke test.

No production traffic.

---

## Step C

Implement:

```text
CuaController.observe
```

Feature flagged.

---

## Step D

Implement simple actions:

```text
click
type
keyboard
scroll
```

---

## Step E

Migrate browser state/actions.

---

## Step F

Add recording integration.

---

## Step G

Run acceptance suite and benchmark.

---

## Step H

Change default:

```text
legacy
→
cua
```

but keep legacy rollback.

---

## Step I

After stable operation, delete obsolete low-level code in separate PRs.

Possible deletion candidates:

```text
a11y.py
a11y.rs
cdp.py
cdp.rs
x11.rs
screen implementation portions
```

Only delete code proven unused.

---

# 32. Suggested PR Sequence

Keep PRs small.

### PR 1

```text
refactor(control): introduce pluggable ComputerController
```

- add interface
- wrap legacy
- no behavior change
- tests

### PR 2

```text
build(desktop): install and validate Cua Driver
```

- image changes
- doctor
- smoke test
- compatibility doc

### PR 3

```text
feat(control): add Cua observation backend
```

### PR 4

```text
feat(control): route computer actions through Cua
```

### PR 5

```text
feat(browser): add Cua browser adapter
```

### PR 6

```text
feat(skills): ingest Cua trajectories for demonstrations
```

### PR 7

```text
test(control): add Cua acceptance and benchmark suite
```

### PR 8

```text
chore(control): make Cua the default backend
```

### Later

```text
chore(control): remove obsolete legacy OS automation
```

---

# 33. Error Model

Map Cua failures to typed LazyBoy errors.

Suggested categories:

```rust
pub enum ControlError {
    DriverUnavailable,
    DriverUnhealthy,
    DisplayUnavailable,
    AccessibilityUnavailable,
    BrowserUnavailable,
    TargetNotFound,
    StaleReference,
    PermissionDenied,
    Timeout,
    Unsupported,
    Busy,
    InvalidAction,
    Internal(String),
}
```

Do not expose Cua raw errors directly to the Agent when a stable LazyBoy error can represent them.

Log the raw cause internally.

---

# 34. Retry Policy

Do not blindly retry UI actions.

Recommended:

### Observation failure

Retry a small bounded number if transport/runtime error appears transient.

### Semantic target not found

```text
re-observe
→ re-resolve
→ retry once
```

### Stale target

```text
re-observe
→ never retry original snapshot ref directly
```

### Pixel miss

Do not repeat the same click indefinitely.

Preserve LazyBoy's stale-click protection concept.

### Driver crash

Restart driver/runtime if safe, then require a fresh observation.

---

# 35. Screenshot Policy

Avoid sending screenshots to the model when structured UI state is sufficient.

Prefer:

```text
semantic / accessibility observation
```

Use screenshots when:

- layout matters
- semantic state is incomplete
- canvas/WebGL is involved
- visual verification is needed

This reduces:

- latency
- model context
- vision token cost
- accidental secret exposure

---

# 36. Cua Permission Policy

If the chosen Cua deployment mode supports permission policies, use allow-list behavior.

LazyBoy only needs a subset of Cua capabilities.

Allow only required operations.

Example conceptual set:

```text
health/doctor-like observation
window/app listing
screenshot
window state
click
type
key
scroll
browser state
browser click
browser type
browser navigation
recording
```

Do not allow unrelated capabilities automatically.

Use actual current Cua tool names from the installed version.

---

# 37. Security Requirements

- Cua must not access the host Docker socket from Agent desktops.
- Each bot must remain isolated.
- Keep LazyBoy supervisor on internal network.
- Keep `controld` localhost/internal where applicable.
- Do not expose Cua daemon ports publicly.
- Do not log passwords/tokens.
- Verify trajectory storage permissions.
- Verify screenshots are covered by LazyBoy retention policy.
- Do not allow an Agent to change Cua policies.
- Do not let the Agent select another bot's display/session.
- Cua executable/version should be controlled by LazyBoy image/build process.

---

# 38. Multi-Screen Considerations

LazyBoy already has screen concepts:

```text
screen_lease_id
screen_id
screen_slot
display
```

Do not discard these.

Cua must be bound to the LazyBoy-selected display/screen.

Verify multi-screen behavior before enabling Cua for multi-screen bots.

If Cua does not safely support LazyBoy's current multi-screen semantics:

- support one screen first
- return explicit capability information
- do not fake support

---

# 39. Cua Capability Mapping

LazyBoy should expose backend capabilities.

Suggested internal structure:

```rust
pub struct ComputerCapabilities {
    pub multi_screen: bool,
    pub semantic_desktop: bool,
    pub semantic_browser: bool,
    pub pixel_actions: bool,
    pub recording: bool,
    pub background_input: bool,
}
```

Only add fields if needed and compatible with existing contract evolution.

Use capability checks instead of OS-name conditionals where possible.

Bad:

```rust
if linux {
    ...
}
```

Better:

```rust
if capabilities.semantic_browser {
    ...
}
```

---

# 40. Future Phase — Cua Sandbox

Do **not** implement this during the Driver migration.

Later, LazyBoy may support:

```text
SandboxProvider
     │
     ├── LazyBoyDocker
     └── CuaSandbox
```

Possible future computer types:

```text
Lightweight Linux
Linux VM
Windows VM
macOS VM
Cloud Computer
```

This should be a separate project after Cua Driver migration is stable.

The current `SandboxProvider` abstraction should make this possible.

---

# 41. Future Phase — Windows / macOS

One strategic reason for Cua is avoiding custom implementations for:

```text
Linux AT-SPI/X11
Windows UIA/input/capture
macOS AX/input/capture
```

Do not add Windows/macOS support during the initial Linux migration.

First ensure the LazyBoy control contract is OS-neutral.

Then add backend capabilities and sandbox providers later.

---

# 42. Definition of Done — Phase 1

Phase 1 is complete when:

1. LazyBoy can run with:

```bash
LAZYBOY_COMPUTER_DRIVER=legacy
```

and:

```bash
LAZYBOY_COMPUTER_DRIVER=cua
```

2. Existing Agent tool schemas remain compatible.

3. Cua works in the existing LazyBoy Linux desktop container.

4. `computer_observe` works through Cua.

5. `computer_act` core actions work through Cua.

6. Browser semantic operations work through Cua.

7. noVNC human takeover still works.

8. Persistent Chromium sessions still work.

9. Docker pause/resume still works.

10. Multi-bot isolation is verified.

11. Legacy backend remains available for rollback.

12. No secret is exposed in logs or recording metadata.

13. Acceptance tests pass.

14. Benchmark results are documented.

---

# 43. First Task for the Coding Agent

Do this first and nothing larger:

## Task

Create a branch:

```text
feat/cua-driver-poc
```

Then:

1. inspect current LazyBoy computer-control flow
2. identify actual desktop Dockerfile/image entrypoint
3. install a pinned stable Cua Driver in that image
4. make no Agent-facing schema changes
5. create a Cua smoke test that runs inside a LazyBoy desktop
6. verify:
   - `cua-driver --version`
   - `cua-driver doctor`
   - screenshot
   - window/accessibility observation
   - native click
   - typing
   - Chromium/browser state
7. write results to:

```text
docs/cua-compatibility.md
```

8. stop after the POC
9. do not delete any legacy control code
10. report blockers before starting the controller refactor

### Expected output

The first PR should answer only:

> "Can Cua Driver reliably control the existing LazyBoy XFCE + Xvfb desktop container?"

If the answer is yes, proceed to the next PR.

---

# 44. Second Task After POC Passes

Create:

```text
ComputerController
```

with:

```text
LegacyController
CuaController
```

Initially make all production requests use:

```text
LegacyController
```

Then route only test/flagged traffic to:

```text
CuaController
```

Do not migrate browser and recording in the same PR.

---

# 45. Architecture Decision

The intended ownership after migration is:

| Concern                             | Owner   |
| ----------------------------------- | ------- |
| Product UI                          | LazyBoy |
| Agent loop                          | LazyBoy |
| Model provider                      | LazyBoy |
| Memory                              | LazyBoy |
| Schedules                           | LazyBoy |
| Vault                               | LazyBoy |
| Takeover                            | LazyBoy |
| Skills/playbooks                    | LazyBoy |
| Sandbox lifecycle (Phase 1)         | LazyBoy |
| Docker resources                    | LazyBoy |
| Remote desktop/noVNC                | LazyBoy |
| Computer observation                | Cua     |
| Native UI control                   | Cua     |
| Pointer/keyboard                    | Cua     |
| Browser semantic control            | Cua     |
| OS-specific accessibility           | Cua     |
| Trajectory capture                  | Cua     |
| Workflow abstraction from recording | LazyBoy |

This is the central design decision.

Do not invert it.

---

# 46. Final Principle

LazyBoy's competitive/product value is:

```text
Agent workspace
Multi-agent
Persistent computers
Human takeover
Memory
Schedules
Vault
Skills
Demo → reusable workflow
Web/mobile UX
Model choice
```

The goal of using Cua is to stop spending LazyBoy engineering effort on:

```text
X11 automation
AT-SPI edge cases
CDP plumbing
screen capture
OS-specific input
window enumeration
cross-platform UI automation
```

Build LazyBoy **on top of** Cua.

Do not turn LazyBoy **into** Cua.

---

# References

LazyBoy:

- https://code.30cm.net/daniel.w/lazyBoy
- https://code.30cm.net/daniel.w/lazyBoy/src/branch/main/docs/architecture.md
- https://code.30cm.net/daniel.w/lazyBoy/src/branch/main/crates/control
- https://code.30cm.net/daniel.w/lazyBoy/src/branch/main/crates/control/src/sandbox.rs
- https://code.30cm.net/daniel.w/lazyBoy/src/branch/main/crates/control/src/actions.rs
- https://code.30cm.net/daniel.w/lazyBoy/src/branch/main/crates/sandbox

Cua:

- https://github.com/trycua/cua
- https://cua.ai/docs
- https://cua.ai/docs/how-to-guides/driver/install
- https://cua.ai/docs/concepts/choose-a-cua-driver-integration
- https://cua.ai/docs/how-to-guides/driver/connect-your-agent
- https://cua.ai/docs/reference/cua-driver/cli-reference
- https://cua.ai/docs/reference/cua-driver/mcp-tools
- https://cua.ai/docs/how-to-guides/driver/record-and-render-a-trajectory
- https://cua.ai/docs/how-to-guides/driver/restrict-tool-access
