#!/usr/bin/env bash
# SessionStart hook: print the resumption packet for this repo.
# Output is a JSON object whose `hookSpecificOutput.additionalContext`
# is injected into the model's context at session start, so a new
# session can pick up where the last one stopped without manually
# reading PLANNING.md.
#
# Wired up via .claude/settings.json. Edit there to disable.

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || echo "$PWD")"
cd "$ROOT"

CURRENT_STATE="$(awk '/^## Current state$/{flag=1; print; next} /^## /{flag=0} flag' PLANNING.md 2>/dev/null || echo '(PLANNING.md missing)')"
RECENT_COMMITS="$(git log -5 --oneline 2>/dev/null || echo '(git log unavailable)')"

PACKET="$(printf '=== PLANNING.md: Current state + Next action ===\n\n%s\n\n=== Recent commits ===\n%s\n' "$CURRENT_STATE" "$RECENT_COMMITS")"

printf '%s' "$PACKET" | jq -Rs '{hookSpecificOutput: {hookEventName: "SessionStart", additionalContext: .}}'
