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

## Build and run

```sh
make            # build everything
make run        # GTK app (PAGE=keymap|actuation|settings|device to open a page)
make daemon     # wootd in the foreground
make cli ARGS="rgb fill '#ff0000'"
make test       # unit tests;  make check = fmt + clippy + tests
```

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
- MCP: one tool per method, `.` → `_` (`rgb_fill`, `keymap_set_key`,
  `advanced_keys_set_mod_tap`, `links_add`, …). `claude mcp add wooting
  -- wootd mcp` or `make mcp-add`.

Method families: `device.*`, `profile.*` (list, activate, create, rename,
delete, swap, export/import, status, save, discard, undo, redo), `rgb.*`
(direct frame, stored rows, brightness, effects, image), `theme.apply`,
`settings.*` (actuation, rapid trigger, per-key `actuation.keys.*` /
`rapid_trigger.keys.*`, `settings.global.*`, `settings.device.*`),
`keymap.*` (per key, per layer, presets), `advanced_keys.*` (DKS, mod-tap,
toggle, Rappy Snappy, SOCD), `links.*`, `backup.create`, `raw.command`.

Editing model: the daemon keeps a draft per profile slot. Edits are pushed
to the keyboard's RAM immediately; `persist: true` (the CLI and MCP
default) also writes flash, while the GTK app edits with `persist: false`
and offers undo / redo / Discard / **Save to Keyboard** like Wootility.
Direct lighting is volatile and safe to animate; `rgb.save` writes flash.
Reset/bootloader commands need `confirm: true`.

## Linked profiles (per-app switching)

`wootd` watches Hyprland's event socket and activates a profile when a
matching window gets focus. Rules live in `~/.config/wooting/links.toml`
and can be edited from the CLI, the API or **My Profiles** in the app:

```sh
woot links add --class cs2 "Rapid Profile"     # class from `hyprctl clients -j`
woot links add --title Minecraft 2             # title substring, slot number
woot links default Omarchy                     # when nothing matches
woot links                                     # list / indices for `remove`
```

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
woot map preset dvorak             woot map set --layer 1 j=down --draft
woot actuation set --point 1.2 --rapid-trigger true --press 0.3
woot actuation keys w=0.8 a=0.8    woot actuation rt w=on a=on
woot akc modtap capslock --tap esc --hold lctrl   woot akc socd a d --mode 1
woot profile create "FPS"          woot profile swap 0 3    woot profile export 2 fps.json
woot profile status / save / discard / undo / redo         (daemon drafts)
woot settings --dim 5 --off 30 --debounce true --compat false
woot gamepad map w=ls_up a=ls_left s=ls_down d=ls_right space=a
woot gamepad curve aggro --deadzone 30   woot gamepad joystick --square on --angle on --degrees 60
woot settings --polling 250 --controller xbox
woot links add --class cs2 "Rapid Profile"                 woot backup   woot methods
```

## Status

- Done (Wootility v5.4 parity): RGB per key / image / theme / brightness /
  effect layers, profiles (list, activate, create, duplicate, rename,
  delete, reorder/default via swap, export/import), draft editing with
  undo / redo / Save to Keyboard, key mapping per layer with presets and
  the full key palette, global and per-key actuation, rapid trigger
  (per key, split, continuous), Tachyon, all advanced key kinds (DKS,
  mod-tap, toggle, Rappy Snappy, SOCD), device settings (6KRO, Num Lock,
  debounce, power saving, factory reset), gamepad (mode, polling rate,
  controller type, button mapping, keyboard-keys / override, analog curve
  presets and points, anti-deadzone, angle / square / snappy joystick),
  linked profiles via Hyprland, daemon, HTTP, MCP, CLI, GTK app.
- Protocol notes and the Wootility captures they came from: `docs/`.
- Not yet: firmware updates, Wooting cloud features.
