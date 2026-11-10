// Builds AppIcon.ico, the Windows icon of the app, its installer and its shortcuts, from the
// PNGs of AppIcon.iconset: 16, 32, 64, 128 and 256 px, each stored as PNG (Windows 10 reads
// PNG entries at every size). render.mjs calls it after it has rendered the iconset.
//
// Run alone: bun assets/brand/ico.mjs
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const SLOTS = [
  [16, "icon_16x16.png"],
  [32, "icon_32x32.png"],
  [64, "icon_32x32@2x.png"],
  [128, "icon_128x128.png"],
  [256, "icon_256x256.png"],
];

export function writeIco(out = join(HERE, "AppIcon.ico")) {
  const images = SLOTS.map(([px, file]) => {
    const png = readFileSync(join(HERE, "AppIcon.iconset", file));
    // IHDR: width and height at bytes 16 and 20.
    if (png.readUInt32BE(16) !== px || png.readUInt32BE(20) !== px) throw new Error(`${file} is not ${px} px`);
    return { px, png };
  });
  const header = Buffer.alloc(6 + 16 * images.length);
  header.writeUInt16LE(0, 0); // reserved
  header.writeUInt16LE(1, 2); // type: icon
  header.writeUInt16LE(images.length, 4);
  let offset = header.length;
  images.forEach(({ px, png }, i) => {
    const at = 6 + 16 * i;
    header.writeUInt8(px === 256 ? 0 : px, at); // width; 0 means 256
    header.writeUInt8(px === 256 ? 0 : px, at + 1); // height
    header.writeUInt8(0, at + 2); // no palette
    header.writeUInt8(0, at + 3); // reserved
    header.writeUInt16LE(1, at + 4); // color planes
    header.writeUInt16LE(32, at + 6); // bits per pixel
    header.writeUInt32LE(png.length, at + 8);
    header.writeUInt32LE(offset, at + 12);
    offset += png.length;
  });
  writeFileSync(out, Buffer.concat([header, ...images.map((i) => i.png)]));
  return out;
}

if (import.meta.main) console.log(writeIco());
