#!/usr/bin/env bash
# PreToolUse hook on Bash: when the command is `git commit ...`, append
# the new lines of the active session transcript into the repo so the
# commit naturally bundles it. Lets ccusage on another machine pick up
# this session's usage after a pull.
#
# Each line is redacted to ccusage-essential fields only — usage
# counters, model name, timestamps, ids — and conversation content
# (message text, tool inputs, tool outputs, cwd, gitBranch) is
# dropped. The live transcript on disk is unchanged; only the
# in-repo snapshot is redacted.
#
# Storage: .claude-history/projects/ages/<session_id>.jsonl (tracked)
# plus a sidecar .claude-history/projects/ages/<session_id>.offset
# (gitignored) recording how many *lines* of the live transcript have
# already been mirrored. Each commit only writes the new line delta —
# git log -p shows the new entries, not the whole growing file.
#
# The projects/<slug>/ layout matches what ccusage expects: point it
# at this repo via `CLAUDE_CONFIG_DIR=<repo>/.claude-history ccusage`.
#
# Wired up via .claude/settings.json. Edit there to disable.

set -euo pipefail

INPUT="$(cat)"
COMMAND="$(printf '%s' "$INPUT" | jq -r '.tool_input.command // empty')"
TRANSCRIPT="$(printf '%s' "$INPUT" | jq -r '.transcript_path // empty')"
SESSION_ID="$(printf '%s' "$INPUT" | jq -r '.session_id // empty')"

# Only act on `git commit` invocations. Match anywhere in the
# command string so bundled forms like
# `git add -A && git commit -m ...` are caught too — the previous
# `"git commit"*` matcher missed those, leaving session transcripts
# unsnapshotted whenever the agent batched add+commit+push.
case "$COMMAND" in
    *"git commit"*) ;;
    *) exit 0 ;;
esac

# Need a real transcript file and a session id to do anything.
if [[ -z "$TRANSCRIPT" || ! -f "$TRANSCRIPT" || -z "$SESSION_ID" ]]; then
    exit 0
fi

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || echo "$PWD")"
HISTORY="$ROOT/.claude-history/projects/ages"
mkdir -p "$HISTORY"

DEST="$HISTORY/$SESSION_ID.jsonl"
OFFSET_FILE="$HISTORY/$SESSION_ID.offset"

LAST_LINE=0
if [[ -f "$OFFSET_FILE" ]]; then
    LAST_LINE="$(cat "$OFFSET_FILE")"
fi

TOTAL_LINES="$(wc -l < "$TRANSCRIPT" | tr -d ' ')"

# Redact each new line to the minimal envelope ccusage consumes:
# top-level type/timestamp/sessionId/uuid/parentUuid/requestId, and
# the message envelope's id/role/model/usage. Drop content, tool
# inputs/outputs, cwd, gitBranch, attachment, etc.
REDACT='
{
  type: .type,
  timestamp: .timestamp,
  sessionId: .sessionId,
  uuid: .uuid,
  parentUuid: .parentUuid,
  requestId: (.requestId // null),
  isSidechain: (.isSidechain // null),
  message: (
    if .message != null then
      .message | {
        id: (.id // null),
        role: (.role // null),
        model: (.model // null),
        usage: (.usage // null)
      }
    else null end
  )
}'

if (( TOTAL_LINES > LAST_LINE )); then
    tail -n "+$((LAST_LINE + 1))" "$TRANSCRIPT" | jq -c "$REDACT" >> "$DEST"
    printf '%s' "$TOTAL_LINES" > "$OFFSET_FILE"
fi

# Stage for the upcoming commit. If the file is already up-to-date in
# the index this is a no-op.
git add "$DEST" >/dev/null 2>&1 || true

exit 0
