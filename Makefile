SHELL := /bin/sh

PREFIX ?= $(HOME)/.local
BINDIR ?= $(PREFIX)/bin
CONFIG_DIR ?= $(HOME)/.config/waypie
CARGO ?= cargo
PIPX ?= pipx
PYTHON ?= python3
RUFF ?= $(PIPX) run ruff
INSTALL ?= install
CP ?= cp

RUST_MANIFEST := Cargo.toml
RUST_BINARY := target/release/waypie

.PHONY: all help build check install forceinstall install-runtime install-configurator install-config install-icons force-install-config uninstall clean

all: build

help:
	@printf '%s\n' \
		'Waypie build and installation targets:' \
		'' \
		'  make build                 Build the optimized Rust runtime' \
		'  make check                 Run all Rust and Python checks' \
		'  make install               Install or update Waypie without replacing configs' \
		'  make forceinstall          Install Waypie, back up and replace configs' \
		'  make install-runtime       Build and install only the Rust runtime' \
		'  make install-configurator  Install or update only the Python configurator' \
		'  make install-config        Install only missing configs and icons' \
		'  make uninstall             Remove the runtime and configurator' \
		'  make clean                 Remove Rust build output' \
		'' \
		'Optional variables:' \
		'  PREFIX=/path               Installation prefix (default: ~/.local)' \
		'  CONFIG_DIR=/path           Configuration directory (default: ~/.config/waypie)'

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

forceinstall:
	$(MAKE) install-configurator
	$(MAKE) install-runtime
	$(MAKE) force-install-config

install-runtime: build
	$(INSTALL) -d $(BINDIR)
	$(INSTALL) -m 755 $(RUST_BINARY) $(BINDIR)/waypie

install-configurator:
	@if $(PIPX) list --short | awk '{ print $$1 }' | grep -qx waypie; then \
		echo "Updating the Waypie configurator with pipx"; \
		$(PIPX) upgrade waypie; \
	else \
		echo "Installing the Waypie configurator with pipx"; \
		$(PIPX) install .; \
	fi

install-config:
	$(INSTALL) -d $(CONFIG_DIR)
	@if [ ! -e "$(CONFIG_DIR)/config" ]; then \
		$(INSTALL) -m 644 config.example "$(CONFIG_DIR)/config"; \
	fi
	@if [ ! -e "$(CONFIG_DIR)/style.css" ]; then \
		$(INSTALL) -m 644 style.example.css "$(CONFIG_DIR)/style.css"; \
	fi
	$(MAKE) install-icons

install-icons:
	@$(INSTALL) -d "$(CONFIG_DIR)/icons"
	@$(CP) -R --update=none icons/. "$(CONFIG_DIR)/icons/"

force-install-config:
	$(INSTALL) -d $(CONFIG_DIR)
	@if [ -e "$(CONFIG_DIR)/config" ]; then \
		$(CP) -p -f "$(CONFIG_DIR)/config" "$(CONFIG_DIR)/config.bak"; \
	fi
	@if [ -e "$(CONFIG_DIR)/style.css" ]; then \
		$(CP) -p -f "$(CONFIG_DIR)/style.css" "$(CONFIG_DIR)/style.css.bak"; \
	fi
	$(INSTALL) -m 644 config.example "$(CONFIG_DIR)/config"
	$(INSTALL) -m 644 style.example.css "$(CONFIG_DIR)/style.css"
	$(MAKE) install-icons

uninstall:
	rm -f $(BINDIR)/waypie
	-$(PIPX) uninstall waypie

clean:
	$(CARGO) clean --manifest-path $(RUST_MANIFEST)
