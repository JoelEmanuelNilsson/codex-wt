# codex-wt

`codex-wt` creates Codex-style local Git worktrees under `$CODEX_HOME/worktrees`.

It mirrors the documented Codex Desktop filesystem style, but it does not register worktrees inside Codex Desktop's internal app state.

## Install

```bash
make install-local
```

## Commands

```bash
codex-wt --json doctor
codex-wt --json create --repo /path/to/repo --base HEAD
codex-wt --json create --repo . --base main --slug ep-623 --include-dirty
codex-wt --json list --repo /path/to/repo
codex-wt --json inspect --path /path/to/worktree
```

JSON success output always includes `ok: true`. JSON errors include `ok: false` and an `error.message`.
