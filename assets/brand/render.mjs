// Renders every brand asset from the SVG and HTML sources with headless Chrome, then builds
// AppIcon.icns with iconutil, AppIcon.ico (ico.mjs) and the 1x + 2x DMG background TIFF with tiffutil.
//
// Run from anywhere: bun assets/brand/render.mjs   (macOS only)
//
// Sources: the 1024 master is docs/design/brand/icon.svg (DESIGN.md §15). Below 64 px the master
// turns soft, so 16 px and 32 px use the pixel-fitted drawings in src/icon-16.svg and src/icon-32.svg.
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { writeIco } from "./ico.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "../..");
const SRC = join(HERE, "src");
const MASTER = join(ROOT, "docs/design/brand/icon.svg");
const CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

// Renders `src` (an SVG or HTML file) into `out` at `w` x `h` CSS px and device scale `scale`.
function render(src, out, w, h, scale = 1) {
  const work = mkdtempSync(join(tmpdir(), "brand-render-"));
  try {
    let url = `file://${src}`;
    if (src.endsWith(".svg")) {
      const html = join(work, "page.html");
      writeFileSync(html, `<!doctype html><html><body style="margin:0;background:transparent"><img src="file://${src}" width="${w}" height="${h}" style="display:block"></body></html>`);
      url = `file://${html}`;
    }
    execFileSync(CHROME, ["--headless=new", "--hide-scrollbars", `--force-device-scale-factor=${scale}`, `--window-size=${w},${h}`,
      `--user-data-dir=${join(work, "profile")}`, "--default-background-color=00000000", "--allow-file-access-from-files",
      `--screenshot=${out}`, url], { stdio: "ignore" });
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
  const dims = execFileSync("sips", ["-g", "pixelWidth", "-g", "pixelHeight", out], { encoding: "utf8" });
  const [pw, ph] = [...dims.matchAll(/pixel(?:Width|Height): (\d+)/g)].map((m) => Number(m[1]));
  if (pw !== w * scale || ph !== h * scale) throw new Error(`${out}: ${pw}x${ph}, want ${w * scale}x${h * scale}`);
  console.log(relative(ROOT, out));
}

// 1. App icon: every .icns slot. The file name is the slot; the drawing depends on pixels.
const iconFor = (px) => (px <= 16 ? join(SRC, "icon-16.svg") : px <= 32 ? join(SRC, "icon-32.svg") : MASTER);
const ICONSET = join(HERE, "AppIcon.iconset");
rmSync(ICONSET, { recursive: true, force: true });
mkdirSync(ICONSET);
for (const pt of [16, 32, 128, 256, 512]) {
  render(iconFor(pt), join(ICONSET, `icon_${pt}x${pt}.png`), pt, pt);
  render(iconFor(pt * 2), join(ICONSET, `icon_${pt}x${pt}@2x.png`), pt * 2, pt * 2);
}
execFileSync("iconutil", ["-c", "icns", ICONSET, "-o", join(HERE, "AppIcon.icns")]);
console.log("assets/brand/AppIcon.icns");
// The Windows icon, from the same PNGs.
writeIco();
console.log("assets/brand/AppIcon.ico");

// 2. Menu bar template images (NSImage.isTemplate: the name ends in "Template").
const MENUBAR = join(HERE, "menubar");
mkdirSync(MENUBAR, { recursive: true });
for (const [name, src] of [["StatusItem", "status-item"], ["StatusItemWaiting", "status-item-waiting"]]) {
  render(join(SRC, `${src}-16.svg`), join(MENUBAR, `${name}Template.png`), 16, 16);
  render(join(SRC, `${src}.svg`), join(MENUBAR, `${name}Template@2x.png`), 16, 16, 2);
}

// 3. Login: the bare mark (light and dark ink) and the app icon at the login size (88 pt).
const LOGIN = join(HERE, "login");
mkdirSync(LOGIN, { recursive: true });
const MARK_H = (w) => Math.round((w * 448) / 656);
for (const w of [32, 88]) {
  for (const [suffix, src] of [["", "logo-mark.svg"], ["-dark", "logo-mark-dark.svg"]]) {
    render(join(SRC, src), join(LOGIN, `logo-mark${suffix}-${w}.png`), w, MARK_H(w));
    render(join(SRC, src), join(LOGIN, `logo-mark${suffix}-${w}@2x.png`), w, MARK_H(w), 2);
  }
}
render(MASTER, join(LOGIN, "app-icon-88.png"), 88, 88);
render(MASTER, join(LOGIN, "app-icon-88@2x.png"), 88, 88, 2);
// The icon cropped to its body with no baked shadow (72 pt, 2x only), for light and dark surfaces.
render(join(SRC, "icon-bare.svg"), join(LOGIN, "app-icon-72@2x.png"), 72, 72, 2);
render(join(SRC, "icon-bare-dark.svg"), join(LOGIN, "app-icon-dark-72@2x.png"), 72, 72, 2);

// 3b. The iPhone app icon (full bleed, 1024 px) and the homepage's icons (256 px, light and dark page).
render(join(SRC, "icon-ios.svg"), join(ROOT, "apps/ios/Resources/Assets.xcassets/AppIcon.appiconset/icon-1024.png"), 1024, 1024);
render(MASTER, join(ROOT, "apps/website/src/img/icon.png"), 256, 256);
render(join(ROOT, "docs/design/brand/icon-dark.svg"), join(ROOT, "apps/website/src/img/icon-dark.png"), 256, 256);

// 4. DMG window background: 660 x 400 pt, one TIFF with the 1x and 2x images.
const DMG = join(HERE, "dmg");
mkdirSync(DMG, { recursive: true });
render(join(SRC, "dmg-background.html"), join(DMG, "background.png"), 660, 400);
render(join(SRC, "dmg-background.html"), join(DMG, "background@2x.png"), 660, 400, 2);
execFileSync("tiffutil", ["-cathidpicheck", join(DMG, "background.png"), join(DMG, "background@2x.png"), "-out", join(DMG, "background.tiff")], { stdio: "ignore" });
console.log("assets/brand/dmg/background.tiff");
