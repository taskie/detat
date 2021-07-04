CLI := target/release/detat
PREFIX := $(HOME)/.local

.PHONY: build

build:
	cargo build --release

.PHONY: install

install: build
	cp $(CLI) $(PREFIX)/bin/detat

.PHONY: dev

dev:
	cargo build

.PHONY: test

test:
	cargo test --all

.PHONY: fmt

fmt:
	cargo +nightly fmt

.PHONY: fix

fix:
	cargo +nightly fix --allow-staged

.PHONY: lint

lint:
	cargo +nightly clippy --all-features

.PHONY: doc

doc:
	cargo doc --open

.PHONY: pre-commit

pre-commit:
	$(MAKE) fix
	$(MAKE) fmt
	git diff --exit-code
	$(MAKE) lint
	$(MAKE) test

.PHONY: setup-dev

setup-dev:
	echo 'exec make pre-commit 1>&2' >.git/hooks/pre-commit
	chmod +x .git/hooks/pre-commit
