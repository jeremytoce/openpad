# Live verification findings (Task 5)

Date: 2026-07-20. Sources: official Claude Code docs (code.claude.com), official Codex docs (developers.openai.com/codex + github.com/openai/codex), Wispr Flow local config on this machine. Items marked **EMPIRICAL-PENDING** get confirmed by a human during the Task 10 smoke test.

## Claude Code

- **Permission prompt keys (documented):** `confirm:yes` = `Y` or `Enter`; `confirm:no` = `N` or `Escape`; `confirm:nextField` = `Tab`; `confirm:toggle` = `Space`. Digit selection (1/2/3) is not in the official keybindings reference. **EMPIRICAL-PENDING:** whether digits also select options in the multi-option permission dialog, and the single-key path for "don't ask again" (`approve_always`) — currently mapped to `2` as a hypothesis; smoke test decides between `2` and `Tab`-then-`y`.
- **Interrupt (documented):** single `Esc` stops the current response/tool call. Caveat: when a permission dialog is open, `Esc` closes the dialog (acts as reject) instead of interrupting.
- **Undo (documented):** double-`Esc` on empty input opens the rewind menu (a menu, not an instant undo). Mapped as-is; selecting within the menu is on the user.
- **Plan mode (documented):** `Shift+Tab` cycles permission modes `default → acceptEdits → plan → (custom)`. It is a cycle, not a toggle — pressing it may need multiple taps to land on plan.
- **Hooks (documented):** stdin JSON includes `hook_event_name` with values incl. `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `Stop`, `StopFailure`, `SessionEnd`. `PreToolUse` carries `tool_name` + `tool_input`. `Notification` carries `notification_type` (e.g. `"permission_prompt"`) + `message`. Whether Notification also fires on idle-waiting (and is distinguishable) is underdocumented — WAITING is keyed to Notification generally for now.
- Added mappings: `PostToolUseFailure → ERROR`, `StopFailure → ERROR`, `SessionEnd → IDLE`.

## Codex CLI

- **Not installed on this machine** (`codex` absent). All findings doc-sourced; **EMPIRICAL-PENDING** end-to-end once installed (`brew install --cask codex` or `npm i -g @openai/codex`).
- **Hooks system exists (documented)** — supersedes the spec's "notify-only, degraded fidelity" assumption. Events: `SessionStart`, `SubagentStart`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PreCompact`, `PostCompact`, `UserPromptSubmit`, `SubagentStop`, `Stop`. Payload via **stdin JSON** with `hook_event_name`, `tool_name`, `tool_input` — same shape as Claude's, so the same shim works. Configured via `hooks.json` or `[hooks]` in `~/.codex/config.toml`.
- **`PermissionRequest` fires when Codex is about to ask approval** → accurate WAITING light. Codex adapter upgraded to `fidelity = "full"`.
- **`notify` (documented):** argv-JSON, fires only on `agent-turn-complete`. Kept as a fallback shim for older Codex versions.
- **Approval keys (documented, via UI text quoted in issues):** `y` = approve; `p` = approve + standing prefix rule (the approve_always analog); `Esc` = reject. Single `Esc` interrupts a running turn; `Ctrl+C` exits the session (do not map).
- **Slash commands (documented):** `/compact`, `/new`, `/model`, `/plan`, `/review` all exist.

## Wispr Flow

- Read from `~/Library/Application Support/Wispr Flow/config.json` on this machine: `prefs.user.shortcuts` = `{"63": "ptt", "18+55": "ptt", "41+58": "ptt", ...}` — push-to-talk bound to **Fn (63)**, **Cmd+1 (18+55)**, and **Option+; (41+58)**.
- Chosen synthesis target: **Option+;** → `key code 41 using {option down}` (Fn is not synthesizable via System Events; Cmd+1 collides with tab-switching in many apps).
- **EMPIRICAL-PENDING:** hold-vs-toggle behavior when the combo is synthesized as a single key-down/up (a synthesized tap may behave as toggle rather than hold).

## KB16-01 raw HID / RGB granularity

- **Probed 2026-07-20** via `cargo run -p openpad-hid --example probe` (DOIO KB16-01, VID 0xD010 / PID 0x1601, raw HID usagePage 0xFF60 usage 0x61, attached to this machine).
- **Device open: succeeded.** `hidapi::HidApi::new()` enumerated the device; `device_list().find(...)` matched on VID/PID/usage_page 0xFF60 and `open_device()` returned an open handle with no error.
- **Writes: both succeeded**, no error returned, with the brief's exact VIA report layout — 33-byte buffer, `report_id=0x00` at index 0, `[1]=0x07` (command `custom_set_value`), `[2]=3` (channel `id_qmk_rgb_matrix`), `[3]=value_id`, data from `[4..]`. Did not need to fall back to a 32-byte (no leading 0x00) report; the 33-byte form worked on the first try.
  - `msg[3]=2` (effect), `msg[4]=1` → solid-color effect: write returned `Ok`.
  - `msg[3]=4` (color), `msg[4]=28, msg[5]=255` → hue=amber/sat=max: write returned `Ok`.
- Program exited 0 printing `sent solid amber — did the pad change color?`.
- **Visual confirmation (did the pad's LEDs actually turn amber) is EMPIRICAL-PENDING** — deferred to the Task 10 human smoke test per the brief; a successful HID write does not by itself prove the firmware applied the lighting change.
- **Per-key path: not probed / not expected.** VIA's `custom_set_value` lighting channel (`id_qmk_rgb_matrix`) is a global-effect API — there is no standard VIA value id for addressing an individual key's color. This confirms the brief's expected finding: **global-only** control. The implemented fallback (whole-pad color = bound agent's state, most-urgent aggregate when broadcast, `HidPad::send_frame` renders `frame[3]` to the whole pad) is the correct and only approach for this hardware over the raw HID/VIA protocol; no follow-up needed unless a vendor-specific per-key protocol is discovered later.

## Task 10 smoke-test checklist (human, ~5 min)

1. Claude permission prompt: press pad approve → does `y` approve? Do digits work? What selects "don't ask again"? Update `adapters/claude.toml` if needed.
2. Wispr: pad mic key → does dictation start? Tap-toggle or hold?
3. If Codex installed: hooks fire (`PermissionRequest` → amber), `y`/`p`/`Esc` behave as documented.
