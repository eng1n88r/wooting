# Wooting control suite — common tasks.
#   make            build everything (debug)
#   make run        run the GTK app          make daemon   run wootd in the foreground
#   make cli ARGS="rgb fill '#ff0000'"       run woot with arguments
#   make test       unit tests               make check    fmt + clippy + tests
#   make release    optimised build          make install  cargo install + systemd + hooks
#   make mcp-add    register the MCP server with Claude Code

CARGO ?= cargo
ARGS  ?=
PAGE  ?=

.PHONY: all build release run daemon cli test check fmt clippy clean install uninstall mcp-add logs

all: build

build:
	$(CARGO) build --workspace

release:
	$(CARGO) build --workspace --release

# PAGE=keymap|actuation|settings|device opens the app on that page.
run: build
	WOOTING_GTK_PAGE=$(PAGE) $(CARGO) run -p wooting-gtk

daemon: build
	RUST_LOG=$${RUST_LOG:-info} $(CARGO) run -p wootd -- serve

cli: build
	$(CARGO) run -q -p woot -- $(ARGS)

test:
	$(CARGO) test --workspace

fmt:
	$(CARGO) fmt --all

clippy:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

check: fmt clippy test

clean:
	$(CARGO) clean

install:
	./packaging/install.sh

uninstall:
	-systemctl --user disable --now wootd.service
	-rm -f ~/.config/systemd/user/wootd.service ~/.config/omarchy/hooks/theme-set.d/wooting-theme \
	       ~/.local/share/applications/wooting-gtk.desktop
	-$(CARGO) uninstall wootd woot wooting-gtk

mcp-add:
	claude mcp add --scope user wooting -- $$HOME/.cargo/bin/wootd mcp

logs:
	journalctl --user -u wootd -f
