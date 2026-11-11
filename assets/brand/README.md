# Brand assets

Shipped brand files. Rebuild all of them with `bun assets/brand/render.mjs` (macOS, Google
Chrome, `iconutil`, `tiffutil`). The design is in [docs/DESIGN.md](../../docs/DESIGN.md) §13.11,
§13.16 and §15.

| File | Use |
| --- | --- |
| `AppIcon.icns` | App icon; `scripts/build-dmg.sh` copies it into the bundle and the DMG volume |
| `AppIcon.ico` | Windows icon (16 to 256 px, from the iconset by `ico.mjs`): the exe's resource (`apps/desktop/build.rs`), the installer and the shortcuts |
| `AppIcon.iconset/` | Every `.icns` slot, 16 to 512 pt at 1x and 2x |
| `apps/ios/Resources/Assets.xcassets/AppIcon.appiconset/icon-1024.png` | The iPhone app icon, from `src/icon-ios.svg` (full bleed; iOS rounds the corners) |
| `menubar/StatusItemTemplate{,@2x}.png` | Menu bar status item, 16 pt template image: Jojo's shell with an empty notch |
| `menubar/StatusItemWaitingTemplate{,@2x}.png` | The same with the signal dot lit in the notch: something waits on you |
| `login/logo-mark{,-dark}-{32,88}{,@2x}.png` | The bare mark: Jojo without the icon's body |
| `login/app-icon-88{,@2x}.png` | The app icon at the login size (88 pt) |
| `login/app-icon{,-dark}-72@2x.png` | The icon cropped to its body, no shadow, for light and dark surfaces (from `src/icon-bare{,-dark}.svg`) |
| `apps/website/src/img/icon{,-dark}.png` | The homepage's icons, 256 px, for the light and the dark page |
| `dmg/background.tiff` | DMG window background, 660 x 400 pt, 1x and 2x in one TIFF |

Sources in `src/`:

- The 1024 master is `docs/design/brand/icon.svg`; it draws 64 px and up.
- `icon-32.svg` and `icon-16.svg` are pixel-fitted redrawings for 32 px and 16 px: the master
  turns soft there: Jojo is larger and its fills are flat; 16 px has a larger body and no shadow.
- `status-item.svg` (2x) and `status-item-16.svg` (1x), plus the `-waiting` versions.
  Black on transparent; set `isTemplate` so macOS tints them for the menu bar.
- `logo-mark.svg` and `logo-mark-dark.svg`: the icon without its body. Use them as SVG, or the PNGs.
- The icon is Jojo, the lead character of the default bot family, calm, on the family's cream.
  Every file uses the drawing of [jojo-friends](https://github.com/joyehuang/jojo-friends)
  (`renderSvg`, character `jojo`, emotion `calm`; the numbers are in its `src/library/geometry.ts`).
  Change the body, the size or the placement here; never redraw Jojo by hand.
- `dmg-background.html`: the background. Finder draws icon labels black over a background
  picture, also in dark mode (seen on macOS 26), so it stays light.
  The icon centers (180, 186) and (480, 186) are also in `scripts/build-dmg.sh`.

`scripts/dmg-layout.py` writes the DMG's `.DS_Store` (window size, icon view, positions,
background alias) directly, so the build needs no Finder, AppleScript or GUI session.
