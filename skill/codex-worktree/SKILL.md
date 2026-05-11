---
name: codex-worktree
description: Create Codex-style local Git worktrees under $CODEX_HOME/worktrees using the codex-wt CLI. Use when the user asks Codex to create a new isolated Git worktree, Desktop-like Worktree-mode checkout, parallel workspace, detached worktree, or a worktree that carries current local changes safely.
---

# Codex Worktree

Use the `codex-wt` CLI. Do not hand-roll `git worktree` commands unless the CLI is missing.

## Workflow

1. Verify the command:
   `command -v codex-wt`
2. Check setup:
   `codex-wt --json doctor`
3. Create a detached worktree:
   `codex-wt --json create --repo <repo-or-.> --base <branch-or-HEAD>`
4. Carry tracked local edits only when the user asks:
   `codex-wt --json create --repo <repo-or-.> --base <branch-or-HEAD> --include-dirty`
   Prefer `--base HEAD`; `--include-dirty` fails when the chosen base does not resolve to the source checkout's current `HEAD`.
5. Copy untracked non-ignored files only when the user asks:
   `codex-wt --json create --repo <repo-or-.> --base <branch-or-HEAD> --include-untracked`
6. `cd` into the returned `path`, then run:
   `git status --short --branch`

## Safety

- Keep the worktree detached by default.
- Do not create a branch unless the user explicitly asks.
- Do not delete, prune, or remove worktrees unless the user explicitly asks.
- Do not mutate the original checkout except for normal Git metadata written by Git worktree operations.
- Treat this as a Codex-style local worktree creator, not as an official Codex Desktop thread registration tool.

## Examples

```bash
codex-wt --json create --repo /Users/joel/epsilon --base main
codex-wt --json create --repo . --base HEAD --slug ep-623 --include-dirty
codex-wt --json inspect --path /Users/joel/.codex/worktrees/ep-623/epsilon
```
