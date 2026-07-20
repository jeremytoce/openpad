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

- Not yet probed — Task 8 Step 1 spike (needs pad attached).

## Task 10 smoke-test checklist (human, ~5 min)

1. Claude permission prompt: press pad approve → does `y` approve? Do digits work? What selects "don't ask again"? Update `adapters/claude.toml` if needed.
2. Wispr: pad mic key → does dictation start? Tap-toggle or hold?
3. If Codex installed: hooks fire (`PermissionRequest` → amber), `y`/`p`/`Esc` behave as documented.
