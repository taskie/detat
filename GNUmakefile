.PHONY: build

CLI := target/release/detat

build:
	cargo build --release

PREFIX := $(HOME)/.local

.PHONY: install

install: build
	cp $(CLI) $(PREFIX)/bin/detat

.PHONY: fmt

fmt:
	cargo fmt

.PHONY: fix

fix:
	cargo fix --allow-staged
