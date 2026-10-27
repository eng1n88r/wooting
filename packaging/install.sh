#!/bin/bash
# Build and install wootd, woot and wooting-gtk for the current user, enable
# the daemon, and hook Omarchy theme changes.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo install --path crates/wootd --locked
cargo install --path crates/woot --locked
cargo install --path crates/wooting-gtk --locked || echo "wooting-gtk not installed (GTK build failed?)"

mkdir -p ~/.config/systemd/user
cp packaging/systemd/wootd.service ~/.config/systemd/user/wootd.service
systemctl --user daemon-reload
systemctl --user enable --now wootd.service

mkdir -p ~/.config/omarchy/hooks/theme-set.d
install -m 755 packaging/omarchy/wooting-theme ~/.config/omarchy/hooks/theme-set.d/wooting-theme

mkdir -p ~/.local/share/applications
cp packaging/wooting-gtk.desktop ~/.local/share/applications/ 2>/dev/null || true

echo
echo "Installed. Try:   woot info      woot rgb image ~/.local/state/omarchy/current/background"
echo "Claude Code MCP:  claude mcp add --scope user wooting -- $HOME/.cargo/bin/wootd mcp"
