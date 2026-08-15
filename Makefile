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
CONFIG_SOURCE_DIR := config
CONFIG_FILES := $(notdir $(wildcard $(CONFIG_SOURCE_DIR)/config*))
STYLE_FILES := $(notdir $(wildcard $(CONFIG_SOURCE_DIR)/style*.css))
BUNDLED_CONFIG_FILES := $(CONFIG_FILES) $(STYLE_FILES)
ICON_SOURCE_DIR := $(CONFIG_SOURCE_DIR)/icons

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
	$(RUFF) format --check waypie_config.py waypie_common.py
	$(RUFF) check waypie_config.py waypie_common.py tests
	$(PYTHON) -m py_compile waypie_config.py waypie_common.py
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
	$(INSTALL) -d "$(BINDIR)"
	$(INSTALL) -m 755 "$(RUST_BINARY)" "$(BINDIR)/waypie"

install-configurator:
	@if $(PIPX) list --short | awk '{ print $$1 }' | grep -qx waypie; then \
		echo "Updating the Waypie configurator from this checkout with pipx"; \
		$(PIPX) runpip waypie install --force-reinstall "$(CURDIR)"; \
	else \
		echo "Installing the Waypie configurator with pipx"; \
		$(PIPX) install .; \
	fi

install-config:
	$(INSTALL) -d "$(CONFIG_DIR)"
	@for file in $(BUNDLED_CONFIG_FILES); do \
		if [ ! -e "$(CONFIG_DIR)/$$file" ]; then \
			$(INSTALL) -m 644 "$(CONFIG_SOURCE_DIR)/$$file" "$(CONFIG_DIR)/$$file"; \
		fi; \
	done
	$(MAKE) install-icons

install-icons:
	@$(INSTALL) -d "$(CONFIG_DIR)/icons"
	@$(CP) -R --update=none "$(ICON_SOURCE_DIR)/." "$(CONFIG_DIR)/icons/"

force-install-config:
	$(INSTALL) -d "$(CONFIG_DIR)"
	@for file in $(BUNDLED_CONFIG_FILES); do \
		if [ -e "$(CONFIG_DIR)/$$file" ]; then \
			$(CP) -p -f "$(CONFIG_DIR)/$$file" "$(CONFIG_DIR)/$$file.bak"; \
		fi; \
	done
	@for file in $(BUNDLED_CONFIG_FILES); do \
		$(INSTALL) -m 644 "$(CONFIG_SOURCE_DIR)/$$file" "$(CONFIG_DIR)/$$file"; \
	done
	$(MAKE) install-icons

uninstall:
	rm -f "$(BINDIR)/waypie"
	-$(PIPX) uninstall waypie

clean:
	$(CARGO) clean --manifest-path $(RUST_MANIFEST)
