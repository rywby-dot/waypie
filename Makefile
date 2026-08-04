SHELL := /bin/sh

PREFIX ?= $(HOME)/.local
BINDIR ?= $(PREFIX)/bin
CONFIG_DIR ?= $(HOME)/.config/waypie
CARGO ?= cargo
PIPX ?= pipx
PYTHON ?= python3
RUFF ?= $(PIPX) run ruff
INSTALL ?= install

RUST_MANIFEST := Cargo.toml
RUST_BINARY := target/release/waypie

.PHONY: all build check install install-runtime install-configurator install-config uninstall clean

all: build

build:
	$(CARGO) build --release --locked --manifest-path $(RUST_MANIFEST)

check:
	$(CARGO) fmt --manifest-path $(RUST_MANIFEST) -- --check
	$(CARGO) clippy --locked --all-targets --manifest-path $(RUST_MANIFEST) -- -D warnings
	$(CARGO) test --locked --manifest-path $(RUST_MANIFEST)
	$(RUFF) format --check waypie_animation.py waypie_config.py waypie_common.py
	$(RUFF) check waypie_animation.py waypie_config.py waypie_common.py tests
	$(PYTHON) -m py_compile waypie_animation.py waypie_config.py waypie_common.py
	$(PYTHON) -m unittest discover -s tests

install:
	$(MAKE) install-configurator
	$(MAKE) install-runtime
	$(MAKE) install-config

install-runtime: build
	$(INSTALL) -d $(BINDIR)
	$(INSTALL) -m 755 $(RUST_BINARY) $(BINDIR)/waypie

install-configurator:
	$(PIPX) install --force .

install-config:
	$(INSTALL) -d $(CONFIG_DIR)
	@if [ ! -e "$(CONFIG_DIR)/config" ]; then \
		$(INSTALL) -m 644 config.example "$(CONFIG_DIR)/config"; \
	fi
	@if [ ! -e "$(CONFIG_DIR)/style.css" ]; then \
		$(INSTALL) -m 644 style.example.css "$(CONFIG_DIR)/style.css"; \
	fi
	@$(INSTALL) -d "$(CONFIG_DIR)/icons"
	@cp -R -n icons/. "$(CONFIG_DIR)/icons/"

uninstall:
	rm -f $(BINDIR)/waypie
	-$(PIPX) uninstall waypie

clean:
	$(CARGO) clean --manifest-path $(RUST_MANIFEST)
