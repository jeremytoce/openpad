# Openpad UI (Plan 2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax. UI tasks MUST load the frontend-design skill before writing markup.

**Goal:** A Tauri v2 app (`openpad-ui`) that replaces VIA for this pad: visual keymap editor with live daemon reload, one-click firmware programming over raw HID, config export/import, an always-on-top HUD showing live agent state and pending tool calls, and a menu-bar tray.

**Architecture:** Daemon (existing launchd service) stays the single HID owner and grows a loopback HTTP API (extending the existing ingest server). The UI is a separate Tauri process, dependency-free vanilla HTML/CSS/JS frontend, that only ever talks to `http://127.0.0.1:7676`. Keymap moves from code to config data with hot-reload.

**Tech Stack:** Rust, Tauri v2 (`tauri = "2"`, tray-icon feature), tiny_http (existing), serde_json, plain HTML/CSS/JS (no bundler, no node deps).

## Global Constraints

- All UI↔daemon traffic on `127.0.0.1:7676` only; the UI never opens the HID device or touches config files directly — the daemon owns both.
- Motion rule holds in the HUD: only WAITING may animate.
- No em dashes in any user-facing copy (project rule).
- Config stays hand-editable TOML at `~/.config/openpad/config.toml`; the UI is an editor of that file via the daemon, not a replacement for it.
- `cargo test --workspace` green after every task; UI logic that can be tested headless (serialization, API handlers) must be.
- Frontend: system font stack, dark-mode aware, zero external resources (CSP-safe, fully offline).
- Commit messages end with: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`

## File Structure

```
crates/openpad-core/src/keymap.rs      # + ActionSpec string codec (to/from "agent:approve")
crates/openpad-daemon/src/config.rs    # + [keymap] tables, terminal_apps etc. serialization back to TOML
crates/openpad-daemon/src/api.rs       # NEW: /state /config /pad/program /export /import handlers
crates/openpad-daemon/src/ingest.rs    # route non-/event paths to api.rs
crates/openpad-daemon/src/runloop.rs   # retain per-agent detail; expose snapshot; hot-reload keymap
crates/openpad-hid/src/lib.rs          # + dynamic keymap read/write + program_layout()
app/openpad-ui/Cargo.toml              # NEW: Tauri app crate
app/openpad-ui/tauri.conf.json
app/openpad-ui/src/main.rs             # tray, editor window, HUD window management
app/openpad-ui/ui/editor.html|css|js   # visual keymap editor + settings tabs
app/openpad-ui/ui/hud.html|css|js      # overlay
```

---

### Task 1: Engine state snapshot with pending detail

**Files:** Modify `crates/openpad-daemon/src/runloop.rs`, `crates/openpad-core/src/state.rs` (only if needed — prefer engine-level storage)

**Interfaces:**
- Produces: `pub struct AgentSnapshot { pub name: String, pub state: String, pub detail: Option<String>, pub pane: Option<String> }`; `impl Engine { pub fn ui_snapshot(&self) -> UiSnapshot }` where `pub struct UiSnapshot { pub agents: Vec<AgentSnapshot>, pub layer_lock: bool }` (both `serde::Serialize`).
- `detail` = the `IngestEvent.detail` of the most recent event that set that agent's state (cleared when state returns to Idle/Done); state string = "IDLE"/"THINKING"/"RUNNING"/"WAITING"/"DONE"/"ERROR".

- [ ] TDD: engine test `snapshot_carries_waiting_detail` — ingest PreToolUse w/ detail "Bash {...}" then Notification; `ui_snapshot()` shows state "WAITING" and the detail; after Stop, detail is None.
- [ ] Implement: engine keeps `details: Vec<Option<String>>` parallel to agents, updated in `on_ingest`.
- [ ] `cargo test --workspace` green; commit `feat(daemon): UI snapshot with pending tool detail`.

### Task 2: Keymap as data

**Files:** Modify `crates/openpad-core/src/keymap.rs`, `crates/openpad-daemon/src/config.rs`

**Interfaces:**
- `ActionSpec` string codec on `Action`: `to_spec(&self) -> String` / `from_spec(&str) -> Result<Action, String>` with grammar: `agent:<verb>`, `text:<literal>`, `prompt:<n>`, `mic`, `goto-waiting`. Round-trip tested for every variant.
- Config gains optional `[keymap.steer]` / `[keymap.launch]` tables: keys "1".."16" → spec strings. `Config { pub keymap: Keymap }` built from tables merged over `Keymap::default_map()` (table entries override; missing = default). `config::to_toml(&Config) -> String` serializes the FULL effective config (agents, prompts, keymap, allowlist, wispr, ingest_addr) back to TOML for PUT /config and export.
- `Engine::new` uses `cfg.keymap` instead of `Keymap::default_map()`; add `pub fn reload(&mut self, cfg: Config)` swapping cfg+keymap (state machine untouched).

- [ ] TDD: codec round-trip test; keymap-table override test (`[keymap.steer] "1" = "prompt:3"` → key 0 is Prompt(3), key 1 still default); `to_toml` → `parse` round-trip test.
- [ ] Implement; `cargo test --workspace`; commit `feat: keymap as config data with spec-string codec`.

### Task 3: Daemon HTTP API

**Files:** Create `crates/openpad-daemon/src/api.rs`; modify `ingest.rs` (route), `main.rs` (wire), `runloop.rs` (mpsc for reload)

**Interfaces (the UI contract — exact):**
- `GET /state` → 200 JSON `{"agents":[{"name","state","detail","pane"}],"layer_lock":bool}`
- `GET /config` → 200 `text/plain` TOML (current effective config)
- `PUT /config` body=TOML → validate via `config::parse`; on Ok: atomic-write `~/.config/openpad/config.toml` (tmp+rename), signal engine reload, 204. On Err: 422 with the parse error as body.
- `GET /export` → same as GET /config plus `Content-Disposition: attachment; filename=openpad-config.toml`
- `POST /import` body=TOML → identical semantics to PUT /config.
- `POST /pad/program` → runs `openpad_hid::program_layout` (Task 4); 200 JSON `{"written":N,"verified":bool}` or 500 with error body.
- Existing `POST /event` unchanged. All other paths 404. Handlers are pure functions taking `&ApiState` (channel senders + config path) so they are unit-testable with a real server on a test port.

- [ ] TDD: integration tests (test ports 178xx): state round-trip after posting an event; PUT /config with bad TOML → 422 and file untouched; PUT valid → file rewritten + engine reload observed (engine picks up a keymap override).
- [ ] Implement; run loop handles a `Reload` message between ticks; commit `feat(daemon): loopback HTTP API for UI (state, config, program)`.

### Task 4: VIA dynamic-keymap programming (hardware)

**Files:** Modify `crates/openpad-hid/src/lib.rs`; create `crates/openpad-hid/examples/probe-keymap.rs` (exists), extend

**Interfaces:**
- `pub fn read_keycode(dev, layer, row, col) -> Result<u16, String>` (VIA cmd 0x04), `pub fn write_keycode(dev, layer, row, col, code) -> Result<(), String>` (cmd 0x05, verify by readback), `pub const CANONICAL_LAYOUT: &[(u8, u8, u8, u16)]` — the full F-key matrix from layouts/README (QMK keycodes: KC_F13=0x0068 .. KC_F20=0x006F, LSFT/LCTL/LALT modifier masks 0x0200/0x0100/0x0400, MO(1)=0x5221 per QMK v0.22 encoding — VERIFY on-device, this is the task's empirical core), `pub fn program_layout(pad: &HidPad) -> Result<usize, String>` writing + verifying every entry, plus encoder map via VIA encoder commands if supported (probe; else document keys-only and keep VIA export for encoders).
- HidPad grows `pub fn device(&self) -> &hidapi::HidDevice` or the fns take `&HidPad`.

- [ ] **Requires the pad plugged in and `openpad service stop`.** Probe read (cmd 0x04) against known current layout; record findings in docs/verification.md. If dynamic keymap is unsupported on this firmware, STOP and report BLOCKED (the UI ships with "Program pad" disabled and VIA imports stay documented).
- [ ] Implement read/write + program_layout with full readback verification; hardware test behind `#[ignore]`.
- [ ] Commit `feat(hid): direct firmware programming via VIA dynamic keymap`.

### Task 5: Tauri scaffold (tray + windows)

**Files:** Create `app/openpad-ui/` (Cargo.toml, tauri.conf.json, src/main.rs, ui/ placeholder pages); modify root Cargo.toml (workspace member, but excluded from default `cargo test` heavy deps? include — tests are cheap)

**Setup:** `cargo install tauri-cli --version '^2'` (document in plan; ~5 min compile). `tauri.conf.json`: app id `dev.openpad.ui`, no bundler frontend (`"frontendDist": "../ui"`), windows defined in code not config.

**Interfaces:**
- Tray menu: "Open Editor", "Show HUD" (toggle, checked state), "Pause pad" (calls nothing yet — disabled, Plan 2b), separator, "Daemon: running/stopped" (status line, refreshed on menu open via GET /state reachability), "Quit".
- Editor window: 900x640, resizable, hidden on close (not quit). HUD window: 360x220, frameless, always-on-top, transparent background, positioned top-right, hidden by default, never steals focus (`focused: false`, `accept_first_mouse`).
- `main.rs` exposes Tauri commands `hud_show/hud_hide` callable from JS.

- [ ] Scaffold compiles; `cargo tauri dev` shows tray with both windows openable. Placeholder pages render. Commit `feat(ui): tauri scaffold with tray, editor and HUD windows`.

### Task 6: Editor frontend

**Files:** Create `app/openpad-ui/ui/editor.html`, `editor.css`, `editor.js`

**MUST load frontend-design skill first.** Design intent: calm, instrument-like, dark-first; the pad rendering is the hero. No frameworks.

**Behavior contract:**
- Renders the KB16: 4x4 key grid + 3 knobs, layer tabs (Steer / Launch). Key caps show short labels derived from ActionSpec (`agent:approve` → "Approve"; `prompt:3` → "P3"; `text:...` → first word).
- Click key → side panel: action type picker (Verb dropdown listing union of adapter verbs; Prompt n; Literal text; Mic; Goto waiting; None) with current value selected. Apply = PUT /config with the updated TOML (fetch current via GET /config, patch the keymap table client-side using a minimal TOML section rewriter — keep a `[keymap.steer]`/`[keymap.launch]` table the daemon serializes, so the JS only replaces those sections wholesale).
- Tabs: Keymap | Prompts (1-7 textareas) | Settings (terminal allowlist chips, Wispr hotkey string, ingest addr read-only) | Pad (Program pad button → POST /pad/program with result toast; Export → GET /export download; Import → file picker → POST /import, errors shown verbatim).
- Daemon-unreachable banner when any fetch fails.

- [ ] Implement per contract; manual verification checklist (click key, change to prompt:2, confirm `~/.config/openpad/config.toml` updated and daemon reloaded via GET /state... keymap change observable by pressing the physical key). Commit `feat(ui): visual keymap editor, prompts, settings, pad programming`.

### Task 7: HUD frontend

**Files:** Create `app/openpad-ui/ui/hud.html`, `hud.css`, `hud.js`

**MUST load frontend-design skill first.** Poll GET /state every 250ms.

**Behavior contract:**
- Compact card: one row per configured agent — name, state chip (colors matching the LED legend: idle #999, thinking #4a6, running #48f, waiting #fa0, done #2c8, error #d33), and when WAITING, the `detail` string (tool + input, truncated to 2 lines, full on hover). Only the WAITING chip pulses (CSS animation); nothing else moves.
- Footer: current layer (STEER / LAUNCH from layer_lock; firmware-held layer is not knowable — label it "locked" only when layer_lock).
- Auto-show: when any agent enters WAITING, call `hud_show`; auto-hide 5s after the last agent leaves WAITING (unless the user opened it manually from the tray).
- Unreachable daemon → dim "daemon offline" state, keep polling.

- [ ] Implement; manual verification with synthetic events (`curl -X POST '127.0.0.1:7676/event?agent=codex' -d '{"hook_event_name":"PermissionRequest","tool_name":"shell","tool_input":{"command":"rm -rf build"}}'` → HUD appears showing the command; Stop event → hides after 5s). Commit `feat(ui): always-on-top HUD with waiting detail and auto-show`.

### Task 8: Docs + ship

- [ ] README: new "The app" section (screenshots deferred), updated quickstart (Program pad replaces VIA import when Task 4 landed unblocked), config share section. layouts/README: mark VIA path as fallback/manual alternative.
- [ ] docs/verification.md: record dynamic-keymap findings + UI smoke checklist results.
- [ ] `cargo test --workspace` green; commit `docs: Plan 2 UI documentation`.

## Self-Review Notes

- Spec coverage: editor ✓ (T6), direct programming ✓ (T4, honest BLOCKED path), config share ✓ (T3/T6), HUD ✓ (T1/T7), tray ✓ (T5), keymap-as-data + hot reload ✓ (T2/T3), local-only ✓ (loopback constraint), dependency-free frontend ✓.
- Type consistency: UiSnapshot (T1) is the GET /state payload (T3) consumed by hud.js (T7); ActionSpec codec (T2) is the editor's label/value format (T6); program_layout (T4) is POST /pad/program's implementation (T3).
- Open risk, stated: QMK keycode numeric encodings for MO(1) and modifier masks vary by firmware version; T4 verifies empirically before T3's endpoint goes live, and the UI degrades gracefully if unsupported.
