# Wooting configuration protocol (as used by this project)

Everything below was verified on a **Wooting 60HE+** (USB `31e3:1322`,
firmware 2.14.1) unless marked *unverified*. Sources: Wooting's MPL-licensed
[wooting-rgb-sdk](https://github.com/WootingKb/wooting-rgb-sdk),
[meeuw/libwootility](https://github.com/meeuw/libwootility) (protocol notes
only), [clemenscodes/wootswitch](https://github.com/clemenscodes/wootswitch),
and gists by PastaJ36 and BigBrainAFK listing the command enum.

## Interfaces

The keyboard exposes several HID interfaces. The configuration interface is
the one whose usage page is `0x1337` ("standard" firmware: One, Two, original
60HE) or `0xFF55` ("ARM"/v3 firmware: 60HE+, Two HE ARM, 60HE v2, 80HE, UwU).
On the 60HE+ this is USB interface 1.2 (`/dev/hidraw8` here). Its report
descriptor declares, for report ids 1..6, input and output reports of
32, 62, 254, 510, 1022 and 2046 bytes, plus a 7-byte feature report on id 1.

## Feature command (host → keyboard)

8 bytes sent as a feature report:

| byte | ARM | standard |
|------|-----|----------|
| 0 | report id `1` | `0` |
| 1 | `0xD1` | `0xD0` |
| 2 | `0xDA` | `0xDA` |
| 3 | command id | |
| 4..7 | parameters `p3 p2 p1 p0` (reversed, as in the SDK) | |

Profile/slot arguments go in **p3 (byte 4)** on ARM boards
(`GetProfileMetadata`, `ActivateProfile`, `ReloadProfile`,
`GetRgbProfileColorsPart1/2`, `SaveRgbProfile`, `GetRgbProfileCore`).

## Response (keyboard → host)

An input report, read with a 2047-byte buffer because dumps arrive on the
larger report ids:

```
[report_id, proto(0xD1), 0xDA, cmd_echo, status, len, 0x00, data[len]…]
```

Status `0x88` = ok, `0x66` = unsupported command (e.g. `GetNumberOfKeys`,
`GetRgbProfileCount`, `GetDigitalProfilesCount`, `RefreshRgbColors`).
`data` is a protobuf message for every "Get…Profile"/settings command.
Unrelated input reports can be interleaved; match on `cmd_echo`.

## Command ids

0 Ping · 1 GetVersion (data `maj min patch`) · 2 ResetToBootloader · 3 GetSerial ·
4 GetRgbProfileCount · 7 ReloadProfile0 · 8 SaveRgbProfile(p3=slot) · 9 GetDigitalProfilesCount ·
10 GetAnalogProfilesCount · 11 GetCurrentKeyboardProfileIndex (data[0] = active slot) ·
12 GetDigitalProfile · 13 GetAnalogProfileMainPart · 14/15 AnalogCurveChangeMap ·
16 GetNumberOfKeys · 17 GetMainMappingProfile · 18 GetFunctionMappingProfile ·
19 GetDeviceConfig (raw[10] = layout, 0 = ANSI) · 20 GetAnalogValues · 21 KeysOff · 22 KeysOn ·
23 ActivateProfile(p3=slot) · 24 GetDksProfile · 25 SoftReset · 29 RefreshRgbColors ·
30 WootDevSingleColor(p0=(row<<5)|col, p1=r, p2=g, p3=b) · 31 WootDevResetColor ·
32 WootDevResetAll · 33 WootDevInit · 35/36 GetRgbProfileColorsPart1/2(p3=slot) ·
38 ReloadProfile(p3=slot) · 39 GetKeyboardProfile · 40 GetGamepadMapping · 41 GetGamepadProfile ·
42 SaveKeyboardProfile · 43 ResetSettings · 44 SetRawScanning · 45/46 Start/StopXinputDetection ·
47 SaveDksProfile · 48 GetMappingProfile · 49 GetActuationProfile · 50 GetRgbProfileCore ·
51 GetGlobalSettings · 52 GetAkcProfile · 53 SaveAkcProfile · 54 GetRapidTriggerProfile ·
55 GetProfileMetadata(p3=slot) · 56 IsFlashChipConnected · 57 GetRgbLayer · 58 GetFlashStats ·
59 GetRgbBins. Ids 5, 6, 26, 27, 28, 34, 37 are retired.

## Direct ("WootDev") lighting

Volatile; nothing is written to flash.

1. `WootDevInit` (33) once.
2. Full frame: output report `[5, 0xD1, 0xDA, 11, u16le RGB565[6][21]…]` padded to
   2047 bytes. RGB565 = `(r&0xf8)<<8 | (g&0xfc)<<3 | (b&0xf8)>>3`.
3. Single key: command 30 as above.
4. `WootDevResetAll` (32) gives the LEDs back to the keyboard.

## Structured output reports (verified against Wootility v5.4 captures)

```
[report_id, 0xD1, 0xDA, type, flags, len_lo, len_hi, protobuf…]
```
padded to the chosen report id's size. `flags`: `0x00` = active profile in
RAM (live, lost on profile reload), `(slot << 1) | 1` = write that flash
slot (`0x07` = slot 3), `0x01` = global. The mapping report has a longer
header: `[0x14, flags, layer, 0, 0, len_lo, len_hi, protobuf]`.

The firmware encodes every scalar, including zeros (`18 00`), and rejects
messages without them, so our `.proto` declares those fields `optional`.

| type | name | protobuf (`proto/wooting.proto`) |
|------|------|----------|
| 0x07 | DeviceConfig | raw 7 bytes, no length |
| 0x0E / 0x0F | RgbProfileColorsPart1/2 | `RgbRows` rows 0..2 / 3..5, packed RGB565; always flags 0, then `SaveRgbProfile(p3=slot)` |
| 0x11 | KeyboardProfile | `KeyboardProfile`: actuation, rapid trigger, sensitivities, Tachyon. Preceded by an empty 0x1D with the same flags |
| 0x12, 0x15, 0x1D | ? | sent empty around profile writes |
| 0x13 | AnalogCurve | 4 (x,y) points + extras; constant so far |
| 0x14 | MappingProfile | 6 rows × 21 key codes per layer (0 main, 1–2 Fn, 3 empty); empty = factory default |
| 0x16 | RgbProfileCore | brightness + opaque effect layers (field 4) |
| 0x17 | GlobalSettings | power saving timers; flags 0x01 |
| 0x18 | AdvancedKeys | mod-tap entries; then `SaveAkcProfile(p3=slot)` |
| 0x1A | ProfileMetadata | name, badge colour, icon |
| 0x1B | profile blob | 4 × 510-byte zero chunks on profile creation, flags `chunk<<5 \| 6` |
| 0x28 | ? | `12 02 08 00`, flags 0x01, after save |
| 0x0B | WootDevRawReport | raw RGB565 grid (same as the direct frame) |

Reads of 0x11/0x14/0x1A/0x35/0x36/0x50 data take the slot in p3 (byte 4);
`GetMappingProfile` takes the layer in p1 (byte 6). Reads return the flash
copy, so a RAM-only write never shows in a read-back.

Writing colours: send 0x0E and 0x0F (flags 0), then `SaveRgbProfile` with
p3 = slot. **Sending `SaveRgbProfile` without rows first copies the active
profile's colours (and brightness) into that slot** — observed by accident.

## KeyboardProfile distances

`0x4000 | round(mm / 4.0 * 16383)`: 1.0 mm = 0x5000, 1.6 mm = 0x5999,
4.0 mm = 0x7FFF. Fields: 1 actuation, 3 rapid trigger on, 4 press
sensitivity, 8 release sensitivity (only with split sensitivity), 5 Tachyon
(also sets field 9 = 1 and limits brightness to 179).

## Key codes

HID usage − 3 for ordinary keys (A = 1 … F1 = 0x37, Delete = 0x49, arrows
0x4C–0x4F), modifiers 0x63–0x6A (LCtrl … RGui), 0x77 Menu, 0xE7 Fn1,
0xEA Fn2, 0xC0–0xC5 / 0xE0–0xE8 media and specials (names TBD), 0xFF none.
See `crates/wooting-proto/src/keycode.rs`.

## Profiles (create / delete, Wootility v5.4)

Create slot N: `CreateProfile(64, p3=N)`, RGB core + rows + `SaveRgbProfile(N)`,
four zero 0x1B chunks, then 0x1D/0x11/0x13/0x12/0x14×4/0x15 with flags
`(N<<1)|1`, 0x18 + `SaveAkcProfile(N)`, 0x1A, `PostSaveProfile(72, p3=N)`.
Delete: `DeleteProfile(63, p3=N)`. Switching in Wootility means
`ActivateProfile(N)` followed by pushing the whole profile into RAM.

Full byte-level captures: `docs/captures/README.md`.

## Profiles

Count profiles by probing `GetProfileMetadata` slot 0.. until `len == 0`.
Metadata is `ProfileMetadata { name=1, color{r,g,b}=2, icon=3 }`, e.g.
`"Typing Profile"`, `(0,0,255)`, `"woot:p:black"`. Switch with
`WootDevInit` → `ActivateProfile(slot)` → 100 ms → `ReloadProfile(slot)` → 100 ms.

## Key mapping dump (48)

`MappingProfile { rows[6] { codes: bytes[21] } }`, `0xFF` = no key. Codes are
Wooting's own (letters A..Z = 1..26, digits 1..0 = 27..36, Esc = 38, …), not
HID usages. Default 60HE+ dump:

```
row1: 26 1b 1c 1d 1e 1f 20 21 22 23 24 2a 2b 27       Esc 1..0 - = Backspace
row2: 28 11 17 05 12 14 19 15 09 0f 10 2c 2d 2e       Tab Q..P [ ] \
row3: 36 01 13 04 06 07 08 0a 0b 0c 30 31 2e 25       Caps A..' (ISO) Enter
row4: 64 61 1a 18 03 16 02 0e 0d 33 34 35 ff 68       Shift (ISO) Z../ -- Shift
row5: 63 66 65 ff 29 ff 29 ff 29 ff 69 77 67 e7       Ctrl Win Alt Space×3 Alt Fn Menu Ctrl
```

The space bar occupies matrix cells (5,4), (5,6), (5,8).

## Open items

- Names of the media/special key codes (0xC0…, 0xE0…), the 0x12/0x15/0x1D
  reports and the 0x1B blob.
- Writing mod-tap / DKS / gamepad entries (0x18 format is known for mod-tap;
  no write path yet).
- RGB effect layer schema inside `RgbProfileCore.effects`.
- Whether a RAM-only (flags 0) KeyboardProfile write takes effect without a
  profile reload; Wootility relies on it, we could not observe it.
