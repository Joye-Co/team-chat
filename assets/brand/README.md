# Brand assets

Shipped brand files. Rebuild all of them with `bun assets/brand/render.mjs` (macOS, Google
Chrome, `iconutil`, `tiffutil`). The design is in [docs/DESIGN.md](../../docs/DESIGN.md) §13.11,
§13.16 and §15.

| File | Use |
| --- | --- |
| `AppIcon.icns` | App icon; `scripts/build-dmg.sh` copies it into the bundle and the DMG volume |
| `AppIcon.iconset/` | Every `.icns` slot, 16 to 512 pt at 1x and 2x |
| `menubar/StatusItemTemplate{,@2x}.png` | Menu bar status item, 16 pt template image |
| `menubar/StatusItemWaitingTemplate{,@2x}.png` | The same with the dot: something waits on you |
| `login/logo-mark{,-dark}-{32,88}{,@2x}.png` | The bare mark 「 ● ■ 」 for the login view, light and dark ink |
| `login/app-icon-88{,@2x}.png` | The app icon at the login size (88 pt) |
| `dmg/background.tiff` | DMG window background, 660 x 400 pt, 1x and 2x in one TIFF |

Sources in `src/`:

- The 1024 master is `docs/design/brand/icon.svg`; it draws 64 px and up.
- `icon-32.svg` and `icon-16.svg` are pixel-fitted redrawings for 32 px and 16 px: the master
  turns soft there. Strokes and shapes sit on whole pixels; 16 px has 1 px brackets and no shadow.
- `status-item.svg` (2x grid) and `status-item-16.svg` (1x grid), plus the `-waiting` versions.
  Black on transparent; set `isTemplate` so macOS tints them for the menu bar.
- `logo-mark.svg` and `logo-mark-dark.svg`: the icon without its body. Use them as SVG, or the PNGs.
- `dmg-background.html`: the background. Finder draws icon labels black over a background
  picture, also in dark mode (seen on macOS 26), so it stays light.
  The icon centers (180, 186) and (480, 186) are also in `scripts/build-dmg.sh`.

`scripts/dmg-layout.py` writes the DMG's `.DS_Store` (window size, icon view, positions,
background alias) directly, so the build needs no Finder, AppleScript or GUI session.
