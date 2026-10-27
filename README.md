# wooting — native Linux control for Wooting keyboards

Rust workspace that replaces Wootility for day-to-day use on Omarchy
(Hyprland/Wayland) and lets agents drive the keyboard.

| crate | what |
|-------|------|
| `wooting-proto` | wire protocol, protobuf messages, key layouts (no I/O) |
| `wooting-hid` | device discovery and HID transport (hidapi, linux-native) |
| `wooting-core` | keyboard model, image/theme → colours, backups, shared API |
| `wootd` | daemon: Unix-socket JSON-RPC, localhost HTTP, `wootd mcp` stdio MCP server |
| `woot` | CLI (uses the daemon when running, direct HID otherwise) |
| `wooting-gtk` | GTK4/libadwaita app |

Verified on a Wooting 60HE+ (fw 2.14.1). Other 0xFF55 boards (Two HE ARM,
60HE v2, 80HE) should work for RGB and profiles; the bundled layout is the
60HE's.

## Install

```sh
./packaging/install.sh          # cargo install, systemd --user unit, Omarchy hook
woot info
woot rgb image ~/.local/state/omarchy/current/background
woot rgb release
```

Claude Code: `claude mcp add --scope user wooting -- ~/.cargo/bin/wootd mcp`
then ask it to "paint my keyboard with ~/Pictures/sunset.jpg".

## Agent API

Same methods on every surface (`woot methods` lists them):

- HTTP: `POST http://127.0.0.1:7357/v1/rgb/fill {"color":"#00ff00"}`,
  `POST /v1/rpc` (JSON-RPC), `GET /openapi.json`, `GET /v1/methods`
- socket: newline-delimited JSON-RPC on `$XDG_RUNTIME_DIR/wootd.sock`
- MCP: tools `device_info`, `device_keys`, `rgb_fill`, `rgb_set_key`,
  `rgb_set_keys`, `rgb_set_frame`, `rgb_from_image`, `rgb_save`,
  `rgb_release`, `profile_list`, `profile_activate`, `theme_apply`,
  `backup_create`, `raw_command`, …

Direct lighting is volatile and safe to animate; `rgb.save` writes flash.
Reset/bootloader commands need `confirm: true`.

## Omarchy

`packaging/omarchy/wooting-theme` is a theme-set hook: every
`omarchy-theme-set` repaints the keyboard from the theme's `colors.toml`
(`keyboard.rgb` overrides the base colour) and persists it.

## CLI cheat sheet

```sh
woot rgb fill '#7aa2f7'            woot rgb keys esc=#f7768e space=#9ece6a
woot rgb image photo.jpg           woot rgb save            woot rgb release
woot theme apply --persist         woot profile list        woot profile activate Omarchy
woot map show --layer 1            woot map set capslock=esc --persist
woot actuation set --point 1.2 --rapid-trigger true --press 0.3 --persist
woot settings --dim 5 --off 30     woot backup              woot methods
```

## Status

- Done: RGB (direct and stored), brightness, profiles (list, activate,
  rename, delete), key mapping per layer, actuation / rapid trigger /
  Tachyon, global power-saving settings, image/theme painting, daemon, HTTP,
  MCP, CLI, GTK app.
- Protocol notes and the Wootility captures they came from: `docs/`.
- Not yet: mod-tap / advanced key editing, RGB effect layers, gamepad.
