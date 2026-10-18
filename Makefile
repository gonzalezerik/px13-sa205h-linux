# SPDX-License-Identifier: AGPL-3.0-or-later
#
# PREFIX and DESTDIR follow the usual packager conventions; the install
# layout and the post-install steps are documented in docs/packaging.md.

CARGO ?= cargo

.PHONY: all build test fmt udev-rules install clean

all: build

build:
	$(CARGO) build --release --locked --workspace

test:
	$(CARGO) test --locked --workspace

fmt:
	$(CARGO) fmt --all

udev-rules:
	$(CARGO) xtask generate-udev-rules

install: build
	$(CARGO) xtask install

clean:
	$(CARGO) clean
