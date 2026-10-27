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

## Structured output reports

```
[report_id, 0xD1, 0xDA, report_type, len_hi, len_lo, (prefix…), protobuf…]
```
padded to the chosen report id's size. `len` counts the protobuf only.

| type | name | prefix | protobuf (`proto/wooting.proto`) |
|------|------|--------|----------|
| 0x0E | RgbProfileColorsPart1 | slot byte | `RgbRows` rows 0..2, packed RGB565 per column |
| 0x0F | RgbProfileColorsPart2 | slot byte | `RgbRows` rows 3..5 |
| 0x16 | RgbProfileCore | none? | `RgbProfileCore` (brightness) — *write path unverified: a write followed by Save did not change the read-back* |
| 0x17 | GlobalSettings | none | `GlobalSettings` — *unverified write* |
| 0x14 | MappingProfile | ? | *unverified* |
| 0x0B | WootDevRawReport | – | raw RGB565 grid (same as the direct frame) |

Writing colours: send 0x0E and 0x0F with the slot prefix, then
`SaveRgbProfile` with p3 = slot. **Sending `SaveRgbProfile` without rows
first copies the active profile's colours (and brightness) into that slot** —
observed by accident on slot 0.

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

## Open items (Phase 2)

Write formats for key mappings, actuation, rapid trigger, DKS, Mod-Tap, AKC,
gamepad and global settings, and the exact targeting of `RgbProfileCore`
writes, are to be recovered by capturing Wootility's WebHID traffic
(`tools/webhid-capture/hook.js`).
