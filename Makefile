PREFIX ?= $(HOME)/.local
BINDIR ?= $(PREFIX)/bin
SKILLDIR ?= $(HOME)/.codex/skills/codex-worktree

.PHONY: build test install-local install-skill install-all

build:
	cargo build --release

test:
	cargo test

install-local: build
	mkdir -p "$(BINDIR)"
	cp target/release/codex-wt "$(BINDIR)/codex-wt"

install-skill:
	mkdir -p "$(SKILLDIR)"
	cp skill/codex-worktree/SKILL.md "$(SKILLDIR)/SKILL.md"

install-all: install-local install-skill
