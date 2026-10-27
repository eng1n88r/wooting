# Wootility WebHID captures (60HE+, fw 2.14.1, Wootility v5.4.2)

Captured with `tools/webhid-capture/hook.js`. Bytes are the HID report
payload as WebHID sees it (report id separate). Trailing zeros stripped.

## Polling (filtered out of captures)
Wootility polls feature cmd 0x49 (73) and 0x4a (74) continuously; 0x4a
answers on report id 3 with 168 bytes (analog values). Not needed by us.

## Switching the Wootility-selected profile (header dropdown)
Wootility does NOT just activate a slot; it pushes its own copy of the
profile into the keyboard's active RAM profile:

```
feature  d1 da 17 <slot>                 ActivateProfile(slot)   (slot in byte 4 = p3)
output3  d1 da 0e 00 <len16> <RgbRows rows 0-2>
output3  d1 da 0f 00 <len16> <RgbRows rows 3-5>
output2  d1 da 13 00 <len16> <AnalogCurve>    0a 1c {4 x (x,y) points} 12 08 {08 00 10 00 18 01 20 32}
output3  d1 da 18 00 <len16> <AdvancedKeys>   mod-tap entries, see below
output2  d1 da 1a 00 <len16> <ProfileMetadata>
output1  d1 da 1d 00 <len16> <?>           0a 0a 08 42 08 61 08 62 08 63 18 01 (Typing) / empty (Omarchy)
output1  d1 da 16 00 <len16> <RgbProfileCore>
output1  d1 da 12 00                       empty
output1  d1 da 11 00 <len16> <KeyboardProfile>
```

## Quick Settings changes (live, RAM only)
Every change re-sends `1d` then `11` (KeyboardProfile):

| change | KeyboardProfile payload |
|---|---|
| actuation 1.0 | `08 80 a0 01 10 01 18 00 20 e6 8c 01 28 00 30 02 38 01 48 00` |
| actuation 2.0 | `08 80 c0 01 …` |
| actuation 4.0 | `08 ff ff 01 …` |
| actuation 1.6 | `08 99 b3 01 …` |
| RT on | `… 18 01 …` |
| RT sensitivity 1.0 | `… 20 80 a0 01 …` |
| RT split on, release 0.55 | `… 40 cd 91 01 …` (field 8 appears) |
| Tachyon on | `… 28 01 … 48 01`, plus `16` with brightness 0xb3 |

Decoding: field1 = 0x4000 | round(mm / 4.0 * 16383); field3 = rapid
trigger on; field4 = 0x4000 | press sensitivity; field8 = 0x4000 | release
sensitivity (only with split sensitivity); field5 = tachyon; field9 = 1 with
tachyon; fields 2,6,7 constant (1,2,1).

## Creating an onboard profile (slot 3) and "Save to Keyboard"
```
feature  d1 da 40 03                     cmd 64: create/init slot 3
output1  d1 da 16 00 …                   RgbProfileCore -> RAM
output3  d1 da 0e 00 … / 0f 00 …         RgbRows -> RAM
feature  d1 da 08 03                     SaveRgbProfile(3)
output4  d1 da 1b 06|26|46|66 00…        4 x 510-byte zero chunks (flags = chunk<<5 | 6)
output1  d1 da 1d 07 …                   flags 07 = write straight to slot 3
output1  d1 da 11 07 <len16> KeyboardProfile
output2  d1 da 13 07 <len16> AnalogCurve
output1  d1 da 12 07
output3  d1 da 14 07 <layer> 00 00 <len16> MappingProfile   layers 0..3 (3 empty)
output1  d1 da 15 07
output1  d1 da 18 00 …                   AdvancedKeys -> RAM
feature  d1 da 35 03                     SaveAkcProfile(3)
output2  d1 da 1a 07 <len16> ProfileMetadata
feature  d1 da 48 03                     cmd 72 (response 88 04 00 13 …)
output2  d1 da 1a 00 <len16> ProfileMetadata -> RAM
output1  d1 da 07 10 00 00 00 00 00 0c   DeviceConfig (raw, no length)
output1  d1 da 28 01 04 00 12 02 08 00   cmd/report 0x28: {2:{1:0}} (default profile index?)
output1  d1 da 17 01 08 00 0a 04 08 05 10 0a 10 00   GlobalSettings, flags 01
```
Flags byte (byte after the report type): `00` = active profile in RAM,
`(slot << 1) | 1` = persist into that slot (07 = slot 3), `01` = global.

Mapping layer 1 (Fn) default: `32 37 38 39 3a 3b 3c 3d 3e 3f 40 41 42 49` on
the number row (F-keys …), `43 ff 48 ff 5d 5e 5f 60 ff ff e2 e3 e4` on QWERTY
row, `ea 47 4b 4a 5a 5b 5c c1 c2 c0 e1 e0` on home row. Layer 2 default spells
"EPIC GAMER" (codes 05 10 09 03 / 07 01 0d 05 12).

## RGB effects (RGB Settings page)
Selecting "Wave" sends RgbProfileCore with field 4:
`22 0a 0a 08 0a 06 08 00 10 10 18 00` → layers{ layer{ effect{ kind=0, speed=16, dir=0 } } }.
Global brightness 60 % → field1 = 0x9a (154).

## Deleting a profile
```
(push Typing profile to RAM as above)
feature  d1 da 3f 03                     cmd 63: delete slot 3
(push Omarchy profile to RAM)
```

## Advanced keys (0x18) — the user's mod-tap keys
`0a 0b 12 04 08 4f 10 68 40 8d 01 48 00` ×5: entry{ 2: modtap{ 1: tap_code 0x4f, 2: hold_code 0x68 }, 8: 141, 9: 0 }.
Pairs seen: (4f,68) (4d,77) (4e,67) (4c,e7) (36,e7). Trailer `10 00 18 05` = count 5.

# Session 2 (2026-10-27, temp profile in slot 3) — feature parity captures

## Per-key actuation (report 0x15)
Setting key A to 2.5 mm: `d1 da 15 00 08 00 | 22 06 08 e1 80 fc bf 06` →
`PerKey { repeated Entry actuation = 4 }`, `Entry { uint32 packed = 1 }` with
`packed = (actuation_code << 16) | matrix_id` (0x67FF << 16 | 0x61).

## Per-key rapid trigger (report 0x1D)
Enabling RT on key S: `d1 da 1d 00 06 00 | 0a 04 08 62 18 01` →
`RtKeys { Keys keys = 1 { repeated uint32 key = 1 }; uint32 enabled = 3 }`.
The Typing profile ships `0a 0a 08 42 08 61 08 62 08 63 18 01` (W A S D).
Sensitivities stay in KeyboardProfile (global). Every KeyboardProfile write
is preceded by this report with the same flags.

## Continuous rapid trigger
KeyboardProfile field 7: 1 = RT ends at the actuation point, 0 = continuous.

## Key codes recovered (drop tile → save → diff layer 0)
mouse_left d8, mouse_right d9, mouse_middle da, mouse_back db, mouse_forward dc;
media_player c6, mail c7, calculator c8, file_explorer c9, search ca, www cb,
browser_back cc, browser_forward cd, stop ce, refresh cf, bookmark d0, power d1,
sleep d2, (d3 unused?), screen_switch d4, app_grid d5, display_brightness_down d6,
display_brightness_up d7; F13–F24 = 6b–76; profile_1 e5, key_lock e8, fn3 eb,
profile_cycle ec, ime_on ed, ime_off ee, lang3 ef, lang4 f0, lang5 f1, ro f2,
kana f3, yen f4, henkan (Conv) f5, muhenkan (Non-conv) f6. Fn Lock is disabled
in Wootility for this board. A fourth mapping layer (Fn Layer 3) appears once
an `fn3` key exists.

## Advanced keys (0x18) entry kinds
```
Rappy Snappy  0a 0a 22 04 08 63 10 00 40 61 48 00      Entry{4: {1: other_key=D, 2: 0}, 8: key=A}
SOCD          0a 0d 2a 07 08 a2 01 10 00 18 01 40 40 48 00   Entry{5: {1: other=L-Alt, 2: 0, 3: mode=1}, 8: Tab}
DKS           0a 12 0a 0c 0a 04 17 64 ff ff 12 04 a9 a4 a8 aa 40 42 48 00
                 Entry{1: {1: codes[4]=[W, LShift, -, -], 2: masks[4]=[a9 a4 a8 aa]}, 8: W}
Mod-tap       0a 0b 12 04 08 4f 10 68 40 8d 01 48 00   Entry{2: {1: tap, 2: hold}, 8: key}
Toggle        0a 09 1a 03 08 ea 01 40 60 48 00          Entry{3: {1: code=fn2}, 8: Caps}
```
Trailer `10 00 18 <count>`; saved with `SaveAkcProfile(p3 = slot)` after a
flags-0 write.

## RGB effects (RgbProfileCore field 4 = Effects { repeated Layer layers = 1 })
Layer oneof by field number: 1 wave `{look, speed(×4 s), direction}`,
2 scan `{width, speed, base_brightness, direction}`, 3 breath `{1: 15, 3: 0}`,
4 cycle `{look, speed}`, 5 ripple `{1:0, 2:20, 3:30, 4:30}`, 6 trail
`{1:30, 2:4, 3:0, 4:0xFFFF}`, 7 touch `{1:30, 2:0, 3:0, 4:0, 5:1}`, 8 jelly
`{1:30, 2:0}`, 9 aoe `{1:30, 2:0}`, 10 mixing `{1:0}`, 11 rt_display `{1:140}`,
14 noise `{colour1 RGB565, colour2 RGB565, 3:50, 4:25}`, 15 spiral
`{colour1, colour2, 3:100, 4:139}`. The first layer is the base effect
(Static = no layers); extra "RGB Fx layers" append layers. Presets write
colour rows (0x0E/0x0F) only.

## Global / device settings
- Num Lock inverted: GlobalSettings field 2 = 1.
- Debounce filter: GlobalSettings field 3 = 1.
- Compatibility mode (6KRO): DeviceConfig (0x07, raw) byte 0 = 0x10 on / 0x00 off,
  followed by `SoftReset` (25); Wootility then re-reads everything.
- Gamepad enable: DeviceConfig byte 6 = 0x08 (0x0C = off), then SoftReset; the
  keyboard re-enumerates as PID 0x1320 (gamepad composite) and WebHID needs a
  new permission. Disable by writing 0x0C and resetting.

## Profiles
- "Set as Default" and drag-reorder are **swaps**: Wootility rewrites both
  slots completely (all reports with `(slot<<1)|1` flags, RGB rows + `SaveRgbProfile`,
  0x1B blobs, AKC + `SaveAkcProfile`, metadata) and activates slot 0. The
  default profile is always slot 0.
- Full profile read sequence used by Wootility after a reset (all with p3 = slot):
  `GetVersion`, `GetSerial`(0/1/2/3), 59, 62, 72(slot), 81, 11, 83 (switch table),
  82 (`12 02 08 00 18 01`), 51, 50, 35, 36, 57(slot,1), 39, 54, 61, 41 (analog
  curve = report 0x13), 40, 48 (layers 0..3), 49, 52, 55.

## 0x1B blobs
Four 510-byte chunks, flags `chunk<<5 | slot-ish`; carry RGB565-looking data
for extra RGB layers. Kept opaque.
