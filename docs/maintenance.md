# Maintenance Notes

Operational housekeeping commands for this repository. None of these are
required for normal development; they exist to keep the developer machine
healthy when Claude Code agent sessions accumulate cruft over time.

## Prune stale agent worktrees

Claude Code agent sessions create per-session worktrees under
`~/Ages/.claude/worktrees/agent-*`. After a few dozen sessions these can
consume several gigabytes of disk while their underlying branches are long
gone. To reclaim the space and drop stale `git worktree` entries:

```sh
git worktree prune && rm -rf ~/Ages/.claude/worktrees/agent-*
```

Rationale:

- `git worktree prune` removes worktree metadata whose checkout directory
  no longer exists (safe; only touches `.git/worktrees/`).
- `rm -rf .../agent-*` deletes the checkout directories themselves.
  Only the `agent-*` prefix is targeted so any hand-created worktrees in
  the same folder are left alone.

Run this **outside** an active agent session (an agent cannot delete the
worktree it is currently running in). A good time is right after merging a
batch of PRs, before starting a new round of work.
