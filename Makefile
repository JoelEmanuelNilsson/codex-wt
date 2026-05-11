PREFIX ?= $(HOME)/.local
BINDIR ?= $(PREFIX)/bin

.PHONY: build test install-local

build:
	cargo build --release

test:
	cargo test

install-local: build
	mkdir -p "$(BINDIR)"
	cp target/release/codex-wt "$(BINDIR)/codex-wt"
