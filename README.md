# codex-wt

Create clean, Codex-style Git worktrees from any repo.

`codex-wt` is a small CLI for the workflow Codex agents need all the time:
make a fresh isolated checkout, put it under `$CODEX_HOME/worktrees`, keep it
detached by default, and return stable JSON so an agent can safely move into it.

It uses normal `git worktree` under the hood. The value is the safe, repeatable
Codex-shaped wrapper around it.

## Why Use This?

Raw `git worktree` is powerful, but it is easy to vary the path, accidentally
check out a branch, forget what dirty changes moved over, or leave an agent with
human-oriented output to parse.

`codex-wt` gives you:

- A predictable Codex-style location: `$CODEX_HOME/worktrees/<id>/<repo-name>`
- Detached HEAD by default, so branches do not get occupied accidentally
- Clean worktrees by default, with dirty changes copied only when you ask
- JSON output built for agents and scripts
- Guardrails around common footguns, including mismatched dirty patches,
  untracked overwrite hazards, nested Git repos, and failed-create cleanup
- A companion Codex skill that tells future agents to use the same recipe every
  time

Use plain `git worktree` when you want full manual Git control. Use `codex-wt`
when you want the safest default for agent work, parallel experiments, or
Desktop-like local worktrees.

## What This Is Not

This creates **Codex-style local Git worktrees**. It does not register a thread
inside Codex Desktop, and it does not provide Desktop-only features like Handoff,
automatic snapshots, restore, or app-managed cleanup.

For official Desktop-managed worktrees, use the Codex Desktop app. For local
agent-friendly worktrees, use this CLI.

See the Codex docs on [app worktrees](https://developers.openai.com/codex/app/worktrees)
for the Desktop behavior this mirrors at the filesystem/Git level.

## Install

Requirements:

- Git
- Rust and Cargo

Clone and install:

```bash
git clone https://github.com/JoelEmanuelNilsson/codex-wt.git
cd codex-wt
make install-local
```

If you already have the repo locally:

```bash
make install-local
```

That installs the binary to:

```text
~/.local/bin/codex-wt
```

Make sure `~/.local/bin` is on your `PATH`.

## Quick Start

Check that the local setup looks good:

```bash
codex-wt --json doctor
```

Create a clean detached worktree from the current repo:

```bash
codex-wt --json create --repo . --base HEAD
```

The JSON response includes the new `path`. Move into it:

```bash
cd /path/from/json
git status --short --branch
```

Or launch Codex directly in that path:

```bash
codex --cd /path/from/json
```

## Carrying Local Changes

By default, local edits are not copied. This keeps new worktrees clean and
boring in the best way.

Copy tracked dirty changes:

```bash
codex-wt --json create --repo . --base HEAD --include-dirty
```

Copy tracked dirty changes and untracked non-ignored files:

```bash
codex-wt --json create --repo . --base HEAD --include-dirty --include-untracked
```

Important: `--include-dirty` only works when `--base` resolves to the source
checkout's current `HEAD`. In normal use, that means `--base HEAD`. This avoids
applying a patch made from one commit onto a different commit.

## Useful Commands

```bash
codex-wt --json doctor
codex-wt --json create --repo /path/to/repo --base HEAD
codex-wt --json create --repo . --base HEAD --slug ep-623 --include-dirty
codex-wt --json list --repo /path/to/repo
codex-wt --json inspect --path /path/to/worktree
```

## JSON Contract

All successful JSON responses include:

```json
{
  "ok": true
}
```

Errors include:

```json
{
  "ok": false,
  "error": {
    "message": "what went wrong"
  }
}
```

`create` returns fields like:

```json
{
  "ok": true,
  "path": "/Users/you/.codex/worktrees/example/repo",
  "repo": "/Users/you/code/repo",
  "base_ref": "HEAD",
  "head": "abc123...",
  "detached": true,
  "dirty_applied": false,
  "untracked_applied": false,
  "untracked_count": 0
}
```

## For Codex Agents

There is a companion skill at:

```text
~/.codex/skills/codex-worktree/SKILL.md
```

The intended agent flow is:

1. Run `command -v codex-wt`
2. Run `codex-wt --json doctor`
3. Create with `codex-wt --json create --repo <repo> --base <ref>`
4. Add `--include-dirty` or `--include-untracked` only when the user asks
5. `cd` into the returned `path`
6. Run `git status --short --branch`

Agents should not create branches, delete worktrees, prune, or mutate the
original checkout unless the user explicitly asks.

## Development

```bash
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```
