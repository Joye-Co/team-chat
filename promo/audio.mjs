// Mixes the soundtrack into a 48 kHz stereo WAV: one licensed music track and a few recorded effects.
//   bun audio.mjs <cues.json> <out.wav>          (SFX_DIR points at the effects; ffmpeg decodes everything)
// Nothing is synthesized. cues.json comes from `render.mjs --cues`: the film names a sound only where
// something on screen causes one (a message arriving, a click, text being typed, a step being ticked).
// The film is cut to the music: cues.json names the track and which parts of it play (see timing/*.json).
import { readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { resolve } from 'node:path';

const { duration, cues, music } = JSON.parse(readFileSync(process.argv[2] || 'cues.json', 'utf8'));
const SFX = process.env.SFX_DIR || 'ext/hyperframes/skills/media-use/audio/assets/sfx';
const SR = 48000, N = Math.round(duration * SR), out = [new Float32Array(N), new Float32Array(N)];
const decode = file => {
  const r = spawnSync('ffmpeg', ['-v', 'error', '-i', file, '-ac', '2', '-ar', String(SR), '-f', 'f32le', '-'], { maxBuffer: 1 << 30 });
  if (r.status !== 0) throw new Error('cannot decode ' + file + ': ' + r.stderr);
  const a = new Float32Array(r.stdout.buffer, r.stdout.byteOffset, r.stdout.length / 4), n = a.length / 2, L = new Float32Array(n), R = new Float32Array(n);
  for (let i = 0; i < n; i++) { L[i] = a[2 * i]; R[i] = a[2 * i + 1]; }
  return [L, R];
};
const cache = {};
function sample(name) {                          // decoded, with the time of its first audible moment
  if (cache[name]) return cache[name];
  const [L, R] = decode(resolve(SFX, name + '.mp3')); let pk = 0; for (let i = 0; i < L.length; i++) pk = Math.max(pk, Math.abs(L[i]), Math.abs(R[i]));
  let on = 0; while (on < L.length && Math.max(Math.abs(L[on]), Math.abs(R[on])) < pk * .06) on++;
  return cache[name] = { L, R, onset: on / SR, peak: pk };
}
/* a recording placed so its first audible moment lands one frame after the picture: sound never leads */
function place(name, t, gain, o = {}) {
  const s = sample(name), g = gain / s.peak, rate = o.rate || 1, i0 = Math.round((t + 1 / 60 - s.onset / rate) * SR);
  const len = Math.min(Math.floor(s.L.length / rate), Math.round((o.len ?? 99) * SR + s.onset / rate * SR)), fade = Math.round((o.fade ?? .08) * SR);
  for (let i = 0; i < len; i++) { const j = i0 + i; if (j < 0 || j >= N) continue; const x = i * rate, k = Math.floor(x), f = x - k, e = Math.min(1, (len - i) / fade);
    out[0][j] += (s.L[k] * (1 - f) + (s.L[k + 1] || 0) * f) * g * e; out[1][j] += (s.R[k] * (1 - f) + (s.R[k + 1] || 0) * f) * g * e; }
}
const turn = {}; const vary = (k, list) => list[(turn[k] = (turn[k] ?? -1) + 1) % list.length];   // repeats take another speed
for (const { type, t, v } of cues) {
  if (type === 'click') place('click', t, .3, { rate: vary('click', [1, 1.05, .96]) });
  else if (type === 'pop') place('pop', t, .16, { rate: vary('pop', [1, 1.12, .92, 1.06]), len: .5 });
  else if (type === 'key') place('key-press', t, .22);
  else if (type === 'tick') place('click-soft', t, .12, { rate: vary('tick', [1.25, 1.4, 1.15]) });
  else if (type === 'typing') for (let x = 0; x < v - .1; x += 1.0) place('typing', t + x, .16, { len: Math.min(1.0, v - x), rate: vary('typing', [1, 1.05, .96]), fade: .05 });
}
// the music: the parts of the track the timing file names, joined on bar lines. Each join blends over 20 ms,
// just before the bar line, into what precedes the next part, so the downbeat itself is untouched.
if (music) {
  const [L, R] = decode(music.file), G = +(process.env.MUSIC_GAIN || music.gain || .6), X = .02, PRE = .004;
  let at = 0; const segs = music.segs.map(([a, b]) => { const s = { a, b: Math.min(b, L.length / SR), at }; at += s.b - a; return s; });
  const smp = (ch, t) => { const k = Math.round(t * SR); return k >= 0 && k < L.length ? (ch ? R : L)[k] : 0; };
  for (let j = 0; j < N; j++) {
    const t = j / SR, i = segs.findIndex(s => t >= s.at && t < s.at + (s.b - s.a)); if (i < 0) break;
    const s = segs[i], nx = segs[i + 1], left = s.at + (s.b - s.a) - t;                     // time left in this part
    const x = !nx || left > X + PRE ? 0 : left <= PRE ? 1 : 1 - (left - PRE) / X;
    const g = Math.min(1, t / (music.fadeIn ?? .05)) * Math.min(1, Math.max(0, (duration - .05 - t) / (music.fadeOut ?? .5))) * G;
    for (let c = 0; c < 2; c++) out[c][j] += (smp(c, s.a + (t - s.at)) * (1 - x) + (x ? smp(c, nx.a - left) * x : 0)) * g;
  }
}
let peak = 0; for (let c = 0; c < 2; c++) for (let i = 0; i < N; i++) peak = Math.max(peak, Math.abs(out[c][i]));
const gain = Math.min(1, .89 / peak), pcm = Buffer.alloc(44 + N * 4);
pcm.write('RIFF', 0); pcm.writeUInt32LE(36 + N * 4, 4); pcm.write('WAVEfmt ', 8); pcm.writeUInt32LE(16, 16); pcm.writeUInt16LE(1, 20); pcm.writeUInt16LE(2, 22);
pcm.writeUInt32LE(SR, 24); pcm.writeUInt32LE(SR * 4, 28); pcm.writeUInt16LE(4, 32); pcm.writeUInt16LE(16, 34); pcm.write('data', 36); pcm.writeUInt32LE(N * 4, 40);
for (let i = 0; i < N; i++) for (let c = 0; c < 2; c++) pcm.writeInt16LE(Math.round(Math.max(-1, Math.min(1, out[c][i] * gain)) * 32767), 44 + (i * 2 + c) * 2);
writeFileSync(process.argv[3] || 'promo.wav', pcm);
console.log(cues.length, 'cues,', duration, 's, peak', peak.toFixed(3), 'gain', gain.toFixed(2));
