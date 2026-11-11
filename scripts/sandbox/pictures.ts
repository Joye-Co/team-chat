// Sample pictures for the sandbox: small PNG mock-ups (a gradient, a disc and a few text
// bars, like a poster or a phone screenshot) made here, so the seed needs no image files.
import { deflateSync } from "node:zlib";

const CRC = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

function crc32(bytes: Uint8Array): number {
  let c = 0xffffffff;
  for (const b of bytes) c = CRC[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type: string, data: Uint8Array): Uint8Array {
  const out = new Uint8Array(12 + data.length);
  const view = new DataView(out.buffer);
  view.setUint32(0, data.length);
  out.set(new TextEncoder().encode(type), 4);
  out.set(data, 8);
  view.setUint32(8 + data.length, crc32(out.subarray(4, 8 + data.length)));
  return out;
}

type Rgb = [number, number, number];

const hex = (h: string): Rgb => [parseInt(h.slice(1, 3), 16), parseInt(h.slice(3, 5), 16), parseInt(h.slice(5, 7), 16)];

/** Palettes: top colour, bottom colour, accent. */
const PALETTES: [string, string, string][] = [
  ["#ff9a8b", "#ff6a88", "#fff3b0"],
  ["#4facfe", "#00f2fe", "#ffffff"],
  ["#43e97b", "#38f9d7", "#0b3d2e"],
  ["#fa709a", "#fee140", "#ffffff"],
  ["#30cfd0", "#330867", "#ffd166"],
  ["#a18cd1", "#fbc2eb", "#3a0ca3"],
  ["#f6d365", "#fda085", "#7a1f00"],
  ["#5ee7df", "#b490ca", "#ffffff"],
  ["#232526", "#414345", "#00e0a4"],
  ["#ffecd2", "#fcb69f", "#9d0208"],
  ["#89f7fe", "#66a6ff", "#001d3d"],
  ["#fddb92", "#d1fdff", "#6a4c93"],
];

/** A `w` x `h` PNG mock-up in palette `n`; `phone` draws a status bar and list rows. */
export function picture(w: number, h: number, n: number, phone = false): Uint8Array {
  const [top, bottom, accent] = PALETTES[n % PALETTES.length].map(hex);
  const raw = new Uint8Array((w * 3 + 1) * h);
  const cx = w * (0.3 + 0.4 * ((n * 37) % 10) / 10);
  const cy = h * (phone ? 0.3 : 0.42);
  const r = Math.min(w, h) * (phone ? 0.18 : 0.26);
  const bars = phone
    ? [0.55, 0.63, 0.71, 0.79, 0.87].map((y, i) => [0.08, y, i % 2 ? 0.7 : 0.84, 0.05] as const)
    : [
        [0.1, 0.74, 0.62, 0.05],
        [0.1, 0.82, 0.44, 0.035],
        [0.1, 0.875, 0.5, 0.035],
      ] as const;
  for (let y = 0; y < h; y++) {
    const row = y * (w * 3 + 1);
    raw[row] = 0;
    const t = y / (h - 1);
    for (let x = 0; x < w; x++) {
      let c: Rgb = [0, 1, 2].map((i) => top[i] + (bottom[i] - top[i]) * t) as Rgb;
      const dx = x - cx;
      const dy = y - cy;
      if (dx * dx + dy * dy < r * r) c = c.map((v, i) => v * 0.35 + accent[i] * 0.65) as Rgb;
      for (const [bx, by, bw, bh] of bars) {
        if (x > bx * w && x < (bx + bw) * w && y > by * h && y < (by + bh) * h) c = c.map((v) => v * 0.25 + 255 * 0.75) as Rgb;
      }
      if (phone && y < h * 0.04) c = c.map((v) => v * 0.85) as Rgb;
      const o = row + 1 + x * 3;
      raw[o] = c[0];
      raw[o + 1] = c[1];
      raw[o + 2] = c[2];
    }
  }
  const ihdr = new Uint8Array(13);
  const v = new DataView(ihdr.buffer);
  v.setUint32(0, w);
  v.setUint32(4, h);
  ihdr.set([8, 2, 0, 0, 0], 8);
  const sig = new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]);
  const parts = [sig, chunk("IHDR", ihdr), chunk("IDAT", deflateSync(raw, { level: 6 })), chunk("IEND", new Uint8Array())];
  const out = new Uint8Array(parts.reduce((s, p) => s + p.length, 0));
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}
