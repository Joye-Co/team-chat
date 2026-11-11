/* The film. See SPEC.md for the shots and STORYLINES.md for why it is built this way.
   The music is the clock. The film is written in beats; a timing file (timing/*.json, injected at build)
   ties the beats that matter (a click, the cut to black, the logo) to bar lines of the track in use, and
   K(j) is the time of beat j, read between those ties. Without a timing file the beats are even. */
const TIMING = /*TIMING*/null || { anchors: [[0, .35], [114.6, 84.2]], end: 84.2, music: null };
const ANC = TIMING.anchors;
const K = j => { let i = 0; while (i < ANC.length - 2 && j >= ANC[i + 1][0]) i++; const [j0, t0] = ANC[i], [j1, t1] = ANC[i + 1]; return +(t0 + (j - j0) * (t1 - t0) / (j1 - j0)).toFixed(4); };
const P = (ANC[ANC.length - 1][1] - ANC[0][1]) / (ANC[ANC.length - 1][0] - ANC[0][0]);     // an average beat
const DURATION = TIMING.end;
const MUSIC = TIMING.music;

const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const clamp = (v, a = 0, b = 1) => Math.min(b, Math.max(a, v));
const lerp = (a, b, p) => a + (b - a) * p;
const spring = z => p => { const w = 11, wd = w * Math.sqrt(1 - z * z); return p >= 1 ? 1 : 1 - Math.exp(-z * w * p) * (Math.cos(wd * p) + z / Math.sqrt(1 - z * z) * Math.sin(wd * p)); };
const E = {
  lin: p => p, out2: p => 1 - (1 - p) * (1 - p), in2: p => p * p, in3: p => p * p * p,
  io: p => p < .5 ? 2 * p * p : 1 - Math.pow(-2 * p + 2, 2) / 2,            // camera moves
  expo: p => p >= 1 ? 1 : 1 - Math.pow(2, -10 * p), p4: p => 1 - Math.pow(1 - p, 4),   // arrivals: a long tail
  ui: spring(.8), toy: spring(.55),
};

/* ---- timeline: every style is a pure function of t ---- */
const DEF = { o: 1, x: 0, y: 0, s: 1, sx: 1, sy: 1, r: 0, blur: 0 };
const els = new Map(), hooks = [], late = [], CUES = [];
let NOW = 0;
const reg = el => { if (!els.has(el)) els.set(el, { base: {}, anims: [] }); return els.get(el); };
const set = (el, props) => { Object.assign(reg(el).base, props); };
const to = (el, t, d, props, ease) => { reg(el).anims.push({ t, d, props, ease: ease || E.expo }); };
const hook = (t0, t1, fn) => hooks.push({ t0, t1, fn });          // fn(p clamped, raw)
const after = fn => late.push(fn);                                 // runs once the camera is placed
/* a sound, only where something on screen causes one */
const cue = (type, t, v = 1) => CUES.push({ type, t: +t.toFixed(4), v });

function applyEl(el, rec, t) {
  const st = Object.assign({}, DEF, rec.base);
  for (const a of rec.anims) {
    if (t < a.t) continue;
    const e = a.ease(a.d ? clamp((t - a.t) / a.d) : 1);
    for (const k in a.props) st[k] = lerp(st[k], a.props[k], e);
  }
  const s = el.style;
  s.opacity = st.o; s.visibility = st.o <= .002 ? 'hidden' : '';
  let tr = '';
  if (st.x || st.y) tr += `translate3d(${st.x}px,${st.y}px,0) `;
  if (st.r) tr += `rotate(${st.r}deg) `;
  if (st.s !== 1 || st.sx !== 1 || st.sy !== 1) tr += `scale(${st.s * st.sx},${st.s * st.sy})`;
  s.transform = tr || 'none';
  s.filter = st.blur > .05 ? `blur(${st.blur}px)` : '';
  if (st.h !== undefined) s.height = st.h + 'px';
}
const show = (el, t) => { set(el, { o: 0 }); to(el, t, .001, { o: 1 }, E.lin); };
const swap = (a, b, t, d = .18) => { set(b, { o: 0 }); to(a, t, d, { o: 0 }, E.out2); to(b, t, d, { o: 1 }, E.out2); };

/* ---- the stage ---- */
const g = id => document.getElementById(id);
const stage = g('stage'), camEl = g('cam'), camwrap = g('camwrap'), cursor = g('cursor'), titles = g('titles'), matte = g('matte');
g('apphost').innerHTML = buildApp();
const app = g('app'), appwrap = g('appwrap');
/* where an element of the rebuilt screen is, in the window's own points, whatever the camera is doing */
function box(el) { const a = app.getBoundingClientRect(), r = el.getBoundingClientRect(), z = a.width / 1440; return { x: (r.left - a.left) / z, y: (r.top - a.top) / z, w: r.width / z, h: r.height / z }; }
/* where an element outside the camera is, on the 1920 x 1080 frame */
function onStage(el, ax = .5, ay = .5) { const s = stage.getBoundingClientRect(), r = el.getBoundingClientRect(), k = s.width / 1920; return [(r.left - s.left + r.width * ax) / k, (r.top - s.top + r.height * ay) / k]; }

/* ---- pages: the rebuilt topic screen, and real screens of the product in the same coordinates ---- */
const pages = [{ t: 0, name: 'app' }], plates = {};
function plate(name, file, w = 1440, h = 900) { const d = document.createElement('div'); d.className = 'plate'; d.style.width = w + 'px'; d.style.height = h + 'px'; d.innerHTML = `<img src="shots/${GEO.dir}${file}.webp">`; camEl.appendChild(d); plates[name] = d; }
const page = (name, t) => pages.push({ t, name });
/* the ground never cuts: the outgoing page blurs out, the next one sharpens in */
function blurSwap(name, t) { to(camwrap, t - .16, .16, { o: 0, blur: 10 }, E.in2); page(name, t); to(camwrap, t, .22, { o: 1, blur: 0 }, E.out2); }
/* a patch over part of a real screen, so a row can arrive or a field can be typed into */
function cover(name, x, y, w, h, color) { const c = document.createElement('div'); c.className = 'cover'; c.style.cssText = `left:${x}px;top:${y}px;width:${w}px;height:${h}px;background:${color}`; plates[name].appendChild(c); return c; }

/* ---- the camera: a key says which point of the window sits where on screen, and how large ---- */
const camKeys = []; let CAM = { tx: 0, ty: 0, z: 1 };
function cam(t, target, o = {}) { camKeys.push({ t, d: o.d ?? .7, target, z: o.z ?? 2.4, sx: o.sx ?? 960, sy: o.sy ?? 540, ax: o.ax ?? .5, ay: o.ay ?? .5, e: o.e || E.io }); }
function resolve(k) {
  let tg = typeof k.target === 'function' ? k.target() : k.target, x, y;
  if (Array.isArray(tg)) [x, y] = tg; else { const b = box(tg); x = b.x + b.w * k.ax; y = b.y + b.h * k.ay; }
  return { x, y, z: k.z, sx: k.sx, sy: k.sy };
}
function camAt(t) {
  let i = 0; for (let j = 0; j < camKeys.length; j++) if (camKeys[j].t - camKeys[j].d <= t) i = j;
  const b = camKeys[i], Bv = resolve(b); let v = Bv;
  if (i > 0 && b.d > 0 && t < b.t) { const A = resolve(camKeys[i - 1]), p = b.e(clamp((t - (b.t - b.d)) / b.d)); v = { x: lerp(A.x, Bv.x, p), y: lerp(A.y, Bv.y, p), z: A.z * Math.pow(Bv.z / A.z, p), sx: lerp(A.sx, Bv.sx, p), sy: lerp(A.sy, Bv.sy, p) }; }
  return { tx: v.sx - v.x * v.z, ty: v.sy - v.y * v.z, z: v.z };
}
const toScreen = (el, ax = .5, ay = .5) => { const b = box(el); return [CAM.tx + (b.x + b.w * ax) * CAM.z, CAM.ty + (b.y + b.h * ay) * CAM.z]; };

/* ---- the cursor: it enters from off screen, and its click causes the next thing ----
   a target is a point on the frame [x, y], a point of the window {pt: [x, y]}, an element of the
   rebuilt screen, or an element outside the camera {stage: el} */
const curKeys = [{ t: 0, d: 0, target: [2100, 1250], e: E.io }];
function curTo(t, d, target, ax = .5, ay = .5, e = E.io) { curKeys.push({ t, d, target, ax, ay, e }); }
function click(t, el) { to(cursor, t - .07, .07, { s: .86 }, E.out2); to(cursor, t + .02, .22, { s: 1 }, E.ui); if (el) { to(el, t - .02, .06, { s: .965 }, E.out2); to(el, t + .06, .25, { s: 1 }, E.ui); } cue('click', t); }
const curPos = k => Array.isArray(k.target) ? k.target : k.target.pt ? [CAM.tx + k.target.pt[0] * CAM.z, CAM.ty + k.target.pt[1] * CAM.z] : k.target.stage ? onStage(k.target.stage, k.ax, k.ay) : toScreen(k.target, k.ax, k.ay);
function placeCursor(t) {
  let i = 0; for (let j = 0; j < curKeys.length; j++) if (curKeys[j].t - curKeys[j].d <= t) i = j;
  const b = curKeys[i]; let [x, y] = curPos(b);
  if (i > 0 && b.d > 0 && t < b.t) { const [ax, ay] = curPos(curKeys[i - 1]), p = b.e(clamp((t - (b.t - b.d)) / b.d)); x = lerp(ax, x, p); y = lerp(ay, y, p); }
  const st = cursor.style.transform.match(/scale\(([^,)]+)/); const sc = st ? +st[1] : 1;
  cursor.style.transform = `translate(${x - 9.7}px,${y - 4.4}px) scale(${sc})`;
}

/* ---- text ---- */
function chars(el) {
  const out = [];
  const walk = node => { for (const n of [...node.childNodes]) {
    if (n.nodeType === 3) { const f = document.createDocumentFragment();
      // each letter is its own box; in English a word's letters sit in one unbreakable run, so lines wrap between words
      for (const word of LANG === 'en' ? n.textContent.split(/(\s+)/).filter(Boolean) : [n.textContent]) {
        const host = LANG === 'en' && !/^\s+$/.test(word) ? f.appendChild(Object.assign(document.createElement('span'), { className: 'ch-w' })) : f;
        for (const c of word) { const s = document.createElement('span'); s.className = 'ch'; s.textContent = c; host.appendChild(s); out.push(s); } }
      n.replaceWith(f); }
    else if (n.tagName !== 'I' && !n.classList.contains('av')) walk(n); } };
  walk(el); return out;
}
/* letters go from nothing to ink, in place */
function inkIn(el, t, o = {}) { const cs = chars(el), gap = (o.gap ?? .035) * T.gapScale; cs.forEach((c, i) => { set(c, { o: o.from ?? 0 }); to(c, t + i * gap, o.d ?? .22, { o: 1 }, E.out2); }); }
/* a line sharpens out of a blur, in place */
function blurIn(el, t, o = {}) { const cs = chars(el), gap = (o.gap ?? .03) * T.gapScale; cs.forEach((c, i) => { set(c, { o: 0, blur: 9 }); to(c, t + i * gap, o.d ?? .2, { o: 1, blur: 0 }, E.out2); }); }
function title(lines, x, y, size, t0, t1, o = {}) {
  const el = document.createElement('div'); el.className = 'ft'; el.style.cssText = `left:${x}px;top:${y}px;font-size:${size}px;${o.css || ''}`;
  el.innerHTML = lines.map(l => `<div>${l}</div>`).join(''); titles.appendChild(el);
  [...el.children].forEach((l, i) => (o.blur ? blurIn : inkIn)(l, t0 + i * (o.gap ?? P), { gap: o.cgap ?? .04, d: .26 }));
  if (o.cut) to(el, t1, .001, { o: 0 }, E.lin); else to(el, t1, .2, { o: 0, blur: 10 }, E.in2);
  return el;
}
/* the one line a shot says, bottom left, over a white fade */
const cap = (text, j0, j1) => title([text], 96, 884, 84, K(j0), K(j1) - .22);

/* ---- AI members' faces: stacked poses; a change of mood goes through a take ---- */
const POSES = ['calm:idle', 'happy:idle', 'focus:working', 'curious:needs-input', 'happy:success', 'celebrate:success'];
function faces() {
  for (const el of $$('.av[data-bot]')) {
    const id = el.dataset.bot, have = POSES.filter(p => CHARS[`${id}:${p}`]); if (!have.length) continue;
    el.classList.add('chr'); el.textContent = '';
    const f = document.createElement('span'); f.className = 'face'; el.appendChild(f); el._poses = {};
    have.forEach((p, i) => { const im = new Image(); im.src = CHARS[`${id}:${p}`]; im.style.cssText = 'position:absolute;inset:0'; f.appendChild(im); el._poses[p] = im; set(im, { o: i ? 0 : 1 }); });
    el._face = f; f.style.transformOrigin = '50% 85%';
  }
}
function pose(el, t, p) {
  if (!el._poses || !el._poses[p]) return;
  to(el._face, t - .12, .12, { sy: .84, sx: 1.1 }, E.out2);                      // anticipation: a squash
  for (const k in el._poses) to(el._poses[k], t, .001, { o: k === p ? 1 : 0 }, E.lin);
  to(el._face, t, .5, { sy: 1, sx: 1 }, E.toy);                                   // the new face, then a settle
}

/* ---- rows of the thread arrive at the bottom; what is above moves up ---- */
function arrive(row, t, o = {}) {
  const H = row.offsetHeight, inner = row.firstElementChild;
  set(row, { h: 0, o: 0 }); to(row, t, o.d ?? .5, { h: H }, E.expo); to(row, t + .05, .25, { o: 1 }, E.out2);
  set(inner, { y: 16 }); to(inner, t, .55, { y: 0 }, E.p4);
  if (o.cue !== false) cue('pop', t);
}
const depart = (row, t) => { to(row, t, .3, { h: 0 }, E.io); to(row, t, .18, { o: 0 }, E.out2); };

/* ---- a world that opens out of a thing on screen, and closes in a blur ---- */
function opensFrom(world, el, t, d = .6) {
  set(world, { o: 0 }); to(world, t, .001, { o: 1 }, E.lin);
  after(() => {
    if (NOW < t || NOW > t + d + .02) { if (NOW >= t) world.style.clipPath = 'none'; return; }
    const p = E.io(clamp((NOW - t) / d)), b = box(el), x = CAM.tx + b.x * CAM.z, y = CAM.ty + b.y * CAM.z, w = b.w * CAM.z, h = b.h * CAM.z;
    world.style.clipPath = `inset(${lerp(y, 0, p)}px ${lerp(1920 - x - w, 0, p)}px ${lerp(1080 - y - h, 0, p)}px ${lerp(x, 0, p)}px round ${lerp(14 * CAM.z, 0, p)}px)`;
  });
}
const closes = (world, t) => to(world, t, .22, { o: 0, blur: 10 }, E.in2);

/* ---- a real card, as an object on the stage ---- */
function card(file, w, h) {
  const s = Math.min(1180 / w, 690 / h), W = w * s, H = h * s, el = document.createElement('div');
  el.className = 'cardimg'; el.style.cssText = `left:${960 - W / 2}px;top:${420 - H / 2}px;width:${W}px;height:${H}px`;
  el.innerHTML = `<img src="shots/${GEO.dir}card-${file}-light.webp"><div class="press"></div>`; g('cards').appendChild(el); set(el, { o: 0 });
  const pr = $('.press', el); set(pr, { o: 0 });
  return { el, at: (x, y) => [960 - W / 2 + x * s, 420 - H / 2 + y * s],
    /* the control under the cursor answers the click */
    press(t, x, y, w2, h2, onDark) { pr.style.cssText = `left:${x * s}px;top:${y * s}px;width:${w2 * s}px;height:${h2 * s}px;border-radius:${16 * s}px;background:${onDark ? 'rgba(255,255,255,.28)' : 'rgba(0,0,0,.1)'}`; to(pr, t - .02, .05, { o: 1 }, E.lin); to(pr, t + .1, .25, { o: 0 }, E.out2); } };
}

/* where things are on the real screens: the English screens are laid out a little differently
   (the right panel starts at x 1040, buttons are wider), so each language has its own measures */
const GEO = LANG === 'en' ? {
  dir: 'en/', desk: 'desk-answered-light', artOld: 'artifact-v2-old-light',
  deskCam: [646, 548], deskZ: 2.48, stateCam: [360, 150], stateZ: 2.8, memX: 1012, workFit: true, handled: [290, 594, 520, 55], view: [820.5, 404],
  artX: 652, artZ: 2.48, backToV3: [952, 122], table: [660, 362], handBack: [1355.5, 107],
  // the query is revealed glyph by glyph (the right edge of each, measured on the screenshot)
  ask: { x0: 460, x1: 760, stops: [469.5, 478, 487, 496.5, 505.5, 514, 528.5, 537, 540.5, 544.5, 561, 569.5, 575.5, 584.5, 594, 603, 609, 621, 624, 633.5, 642.5, 651.5, 661.5, 669.5, 683, 691.5, 704, 713, 721.5, 730.5, 739.5, 748.5, 752], caretAt: 461 },
  cards: { approve: [1294, 495, [40, 402, 316, 56, 1]], draft: [1294, 550, [40, 457, 274, 56, 1]], compare: [1294, 1105, [65, 810, 544, 56, 1]], time: [1294, 855, [39, 762, 238, 56, 1]], poll: [1294, 607, [40, 300, 1214, 62, 0]], chart: [1294, 813, null] }, chartAt: [720, 600],
} : {
  dir: '', desk: 'x-desk-answered-light', artOld: 'x-artifact-v2-old-light',
  deskCam: [690, 552], deskZ: 2.3, stateCam: [336, 150], stateZ: 3, memX: 1047, workFit: false, handled: [290, 594, 820, 55], view: [956, 404],
  artX: 692, artZ: 2.3, backToV3: [1040, 122], table: [700, 405], handBack: [1377, 108],
  ask: { x0: 459, x1: 729, stops: Array.from({ length: 17 }, (_, i) => 459 + 263.5 * (i + 1) / 17), caretAt: null },
  cards: { approve: [1294, 498, [40, 402, 210, 56, 1]], draft: [1294, 552, [40, 457, 182, 56, 1]], compare: [1294, 992, [65, 694, 544, 55, 1]], time: [1294, 814, [39, 717, 188, 56, 1]], poll: [1294, 610, [40, 300, 1214, 62, 0]], chart: [1294, 816, null] }, chartAt: [700, 430],
};

/* =================================================================== */
function build() {
  faces();
  plate('desk', GEO.desk); plate('artOld', GEO.artOld); plate('art', 'artifact-v2-light');
  plate('live', 'live-light'); plate('take', 'live-takeover-light'); plate('ask', 'ask-light');
  plate('mem', 'members-v2-light'); plate('memo', 'members-memory-light'); plate('mach', 'settings-machines-light', 640, 450);
  const tflow = g('tflow'), rail = g('rail'), sb = g('sb'), tb = g('tb');
  const F = id => $('.av', g(id));                     // the member's avatar in a row

  /* ----- the recording: the same group chat, on the day it broke ----- */
  const msg = (name, letter, role, time, text) => `<div class="row"><div class="msg">${APPAV(name, letter)}<div><div class="hd"><span class="b6">${name}</span><span class="sm m">${role}</span><span class="sm m">${time}</span></div><div class="tx">${text}</div></div></div></div>`;
  const S0 = 1920 / 470;
  function recording(host, fixed) {
    host.innerHTML = `<div class="pane"><div class="app" style="position:relative;width:470px;height:264.4px">
      <div class="bnw"><div class="banner"><span style="display:inline-grid"><span class="a" style="grid-area:1/1;display:flex;align-items:center;gap:7px">${APPSPIN()}${T.reconnecting}</span><span class="b" style="grid-area:1/1;display:flex;align-items:center;gap:7px"><span class="okc"></span>${T.reconnected}</span></span></div></div>
      <div class="rows">${msg(...T.recMsgs[0])}${msg(...T.recMsgs[1])}${fixed ? '' : msg(...T.recMsgs[1])}</div>
      <div class="scrub" style="left:20px;right:20px;bottom:11px;height:2px"><i></i></div></div></div>`;
    const pane = $('.pane', host); pane.style.transform = `scale(${S0})`;
    return { a: $('.banner .a', host), b: $('.banner .b', host), rows: $$('.row', host), bar: $('.scrub i', host), bn: $('.banner', host), pane };
  }

  /* ================= A · one thing, from start to finish ================= */
  /* 1 · the bug (0 → K(3.2)) */
  const A = g('clipA'), ra = recording(A, false), tDup = K(1);
  set(ra.bn, { o: 0, y: -30 }); to(ra.bn, .12, .4, { o: 1, y: 0 });
  swap(ra.a, ra.b, tDup, .12);
  { const dup = ra.rows[2]; set(dup, { h: 0, o: 0 }); to(dup, tDup, .3, { h: 56 }, E.expo); to(dup, tDup, .001, { o: 1 }, E.lin);
    const inner = dup.firstElementChild; set(inner, { s: .9 }); to(inner, tDup, .5, { s: 1 }, E.ui); cue('pop', tDup); }
  to(ra.bn, K(1.6), .25, { o: 0 }, E.out2);
  hook(K(1.6), K(2.7), p => { const e = E.io(p); ra.pane.style.transform = `translate(${lerp(0, -40, e)}px,${lerp(0, -660, e)}px) scale(${S0 * lerp(1, 1.5, e)})`; });
  hook(0, K(3.2), p => { ra.bar.style.width = lerp(22, 80, p) + '%'; });
  /* …it is a recording: it shrinks into the file in 小王's message */
  const tile = g('file1pl'), tileIc = $('.ic', tile), tCarry = K(3.2), dCarry = .7;
  set(tileIc, { o: 0 }); to(tileIc, tCarry + dCarry - .15, .25, { o: 1 }, E.out2);
  after(() => {
    if (NOW > tCarry + dCarry) { A.style.display = 'none'; return; }
    A.style.display = '';
    const p = E.io(clamp((NOW - tCarry) / dCarry)), b = box(tile), w = b.w * CAM.z, s = lerp(1, w / 1920, p);
    const x = lerp(0, CAM.tx + b.x * CAM.z, p), y = lerp(0, CAM.ty + b.y * CAM.z - (1080 * (w / 1920) - w) / 2, p);
    A.style.transformOrigin = '0 0'; A.style.transform = `translate(${x}px,${y}px) scale(${s})`;
    A.style.borderRadius = lerp(0, 7 * CAM.z / s, p) + 'px';
    A.style.filter = p > 0 && p < 1 ? `blur(${7 * Math.sin(Math.PI * p)}px)` : '';
    A.style.opacity = p < .8 ? 1 : 1 - (p - .8) / .2;
  });

  /* 2 · it lands in the group; the main assistant hands it over (→ K(8.4)) */
  const [r1, r2, r3, r3b, r4, r5, r6, r7] = ['r1', 'r2', 'r3', 'r3b', 'r4', 'r5', 'r6', 'r7'].map(g);
  const cardKids = ['c_chip', 'c_h', 'c_tx', 'c_p1', 'c_p2', 'c_p3', 'c_l1', 'c_l2'].map(g);
  const mid = (el, x) => () => { const b = box(el); return [x, b.y + b.h / 2]; };
  const under = (el, x, dy) => () => { const b = box(el); return [x, b.y + b.h + dy]; };
  // rows arrive in story order (each call measures the row at its full height, then hides it until its time)
  arrive(r1, tCarry, { cue: false, d: .55 }); blurIn(g('r1tx'), tCarry + .4, { gap: .02 });
  arrive(r2, K(5.3), { cue: false }); inkIn($('.sysl', r2), K(5.3) + .08, { gap: .04 });
  arrive(r3, K(7));
  arrive(r3b, K(8.4)); depart(r3b, K(14.5));
  arrive(r4, K(15));
  arrive(r5, K(20) + .25, { cue: false }); inkIn($('.recl span', r5), K(20) + .3, { gap: .02 });
  arrive(r6, K(24) - .12, { d: .6 });
  arrive(r7, K(30.5));
  cam(0, mid(r1, 470), { d: 0, z: 3.2 });
  cam(K(5.3) + .5, mid(r2, 520), { z: 3, sy: 600, d: .6 });
  cam(K(7) + .55, mid(r3, 520), { z: 3, sy: 580, d: .6 });
  blurIn($('.tx', r3), K(7) + .2); pose(F('r3'), K(7) + .1, 'happy:idle');
  pose($('.av', g('work')), .2, 'focus:working');

  /* 3 · on the computer assigned to it (K(8.4) → K(15)) */
  { const b = box(g('work')), fit = GEO.workFit;             // the English row is longer: frame all of it, its button included
    cam(K(8.4) + .6, mid(r3b, fit ? b.x + b.w / 2 : 640), { z: fit ? Math.min(2.75, 1760 / b.w) : 2.75, sy: 600, d: .6 }); }
  pose(F('r3'), K(8.5), 'focus:working');
  curTo(K(9.9), 1.0, g('openlive'), .5, .6);
  click(K(10), g('openlive')); curTo(K(11.2), .8, [2080, 1220]);
  const live = g('live');
  const steps = T.steps;
  const SPK = '<path d="M8 1.800v2.400M8 11.800v2.400M1.800 8h2.400M11.800 8h2.400M3.600 3.600l1.700 1.700M10.700 10.700l1.700 1.700M3.600 12.400l1.700-1.700M10.700 5.300l1.700-1.700"/>';
  live.innerHTML = `<div class="mb"><b>${T.menu[0]}</b><span>${T.menu[1]}</span><span>${T.menu[2]}</span><span>${T.menu[3]}</span><span class="r">${T.day} <span id="lclock">09:14</span></span></div>
    <div class="tw"><div class="bar"><i style="background:#ff5f57"></i><i style="background:#febc2e"></i><i style="background:#28c840"></i><span>Mac mini — <span id="ltitle">${steps[0][0]}</span></span></div>
      <div class="body" id="lbody"></div></div>
    <div class="pc"><div class="h">${T.plan}<span id="lcount">0/5</span></div><div class="bar"><i id="lbar"></i></div>
      ${steps.map((s, i) => `<div class="ps" id="ls${i}"><span class="m"><span class="o"></span><span class="sp" style="color:#2563eb"><svg viewBox="0 0 16 16" stroke="currentColor">${SPK}</svg></span><span class="ck"><svg viewBox="0 0 16 16" stroke="#0a0a0a"><path d="M3 8.400l3.200 3.200L13 4.600"/></svg></span></span><span class="n">${s[0]}</span><span class="t">${s[1]}</span></div>`).join('')}</div>
    <div class="mach"><b>${T.machine}</b><span class="on"><i></i>${T.online}</span><span>${T.assigned}</span></div>`;
  opensFrom(live, g('work'), K(10), .55);
  title([T.onComputer], 96, 900, 92, K(10.7), K(14.6), { cgap: .04 });
  { // the terminal: commands are typed, output lands
    const body = g('lbody'), CH = T.typeCh, TL = T.term, L = [
      [K(10.8), 't', TL[0]], [K(11.6), 'o', TL[1]], [K(12.2), 't', TL[2]], [K(12.8), 'o', TL[3]], [K(13.3), 't', TL[4]], [K(14.1), 'o', TL[5]],
    ];
    hook(0, DURATION, () => {
      let html = '';
      for (let i = 0; i < L.length; i++) { const [t, kind, text] = L[i]; if (NOW < t) break;
        const older = i < L.length - 1 && NOW >= L[i + 1][0];
        const shown = kind === 't' ? text.slice(0, Math.round(clamp((NOW - t) / (text.length * CH)) * text.length)) : text;
        html += `<div class="${older && kind === 'o' ? 'd' : ''}">${shown}${!older ? (Math.floor(NOW * 2.5) % 2 ? '' : ' <span class="caret"></span>') : ''}</div>`; }
      if (body._h !== html) { body.innerHTML = html; body._h = html; }
    });
    L.filter(l => l[1] === 't').forEach(l => cue('typing', l[0], l[2].length * CH));
    // the plan: one step after another; the minutes tell how long each took
    const doneAt = [K(11.9), K(13.1), K(14.3)], startAt = [K(10.6), K(11.9), K(13.1), K(14.3)];
    steps.forEach((s, i) => {
      const row = g('ls' + i), o = $('.o', row), sp = $('.sp', row), ck = $('.ck', row), tm = $('.t', row), nm = $('.n', row);
      set(sp, { o: 0 }); set(ck, { o: 0, s: .4 }); set(tm, { o: 0 });
      if (startAt[i] !== undefined) { to(o, startAt[i], .15, { o: 0 }, E.out2); to(sp, startAt[i], .15, { o: 1 }, E.out2); }
      if (doneAt[i] !== undefined) { to(sp, doneAt[i], .1, { o: 0 }, E.out2); to(ck, doneAt[i], .001, { o: 1 }, E.lin); to(ck, doneAt[i], .4, { s: 1 }, E.ui); to(tm, doneAt[i] + .1, .3, { o: 1 }, E.out2); cue('tick', doneAt[i]); }
      hook(0, DURATION, () => { const on = startAt[i] !== undefined && NOW >= startAt[i] && !(doneAt[i] !== undefined && NOW >= doneAt[i]); row.style.background = on ? '#f5f5f5' : ''; nm.style.color = on ? '#0a0a0a' : ''; nm.style.fontWeight = on ? 500 : ''; sp.firstElementChild.style.transform = `rotate(${NOW * 200}deg)`; });
    });
    hook(0, DURATION, () => { const n = doneAt.filter(t => NOW >= t).length; g('lcount').textContent = n + '/5'; g('lbar').style.transform = `scaleX(${n / 5})`;
      const m = Math.round(lerp(14, 30, clamp((NOW - K(10.6)) / (K(14.5) - K(10.6))))); g('lclock').textContent = '09:' + String(m).padStart(2, '0');
      g('ltitle').textContent = steps[clamp(startAt.filter(t => NOW >= t).length - 1, 0, 3)][0]; });
  }
  closes(live, K(14.8));

  /* 4 · it asks; you pick (K(15) → K(23)) */
  const pinq = g('pinq'), pinr = g('pin'), pinm = g('pinm');
  [pinq, pinr, pinm].forEach(p => set(p, { o: 0, y: 26 }));
  blurIn($('.tx', r4), K(15) + .2, { gap: .018 });
  pose(F('r4'), K(15) + .1, 'curious:needs-input');
  cam(K(14.7), under(r3, 560, 40), { z: 3, sy: 520, d: 0 });           // placed while its computer still covers the frame
  to(pinq, K(16.6), .5, { o: 1, y: 0 });
  cam(K(16.6) + .8, under(r4, 692, 30), { z: 2.3, sy: 560, d: .8 });
  curTo(K(17), 0, [1500, 1250]); curTo(K(19.9), 1.3, g('optB'), .55, .62);
  click(K(20), g('optB'));
  to(pinq, K(20) + .12, .22, { o: 0, y: 14 }, E.in2);
  pose(F('r4'), K(20) + .35, 'focus:working');
  curTo(K(21.6), 1.0, [1500, 1260]);

  /* 5 · it returns with proof (K(24) → K(32)) */
  { const cd = g('card'), top = box(cd).y, H = +box(cd).h.toFixed(1), at = el => +(box(el).y + box(el).h - top + 15).toFixed(1);
    const h1 = at(g('c_h')), h2 = at(g('c_tx')), h3 = at(g('c_p1').parentNode);
    cd.style.overflow = 'hidden'; set(cd, { h: h1 });          // it grows as its parts arrive: never an empty box
    to(cd, K(24.9) - .12, .45, { h: h2 }); to(cd, K(26.2) - .12, .45, { h: h3 }); to(cd, K(27.3) - .12, .45, { h: H }); }
  cam(K(24) + .5, () => { const b = box(r6); return [630, b.y + b.h / 2 + 8]; }, { z: 2.5, sy: 510, d: .8 });
  cardKids.forEach(e => set(e, { o: 0 }));
  to(g('c_chip'), K(24) + .1, .001, { o: 1 }, E.lin); set(g('c_chip'), { s: .6 }); to(g('c_chip'), K(24) + .1, .45, { s: 1 }, E.ui);
  to(g('c_h'), K(24) + .1, .001, { o: 1 }, E.lin); blurIn(g('c_h'), K(24) + .3, { gap: .026 });
  to(g('c_tx'), K(24.9), .001, { o: 1 }, E.lin); inkIn(g('c_tx'), K(24.9), { gap: .009, d: .25 });
  ['c_p1', 'c_p2', 'c_p3'].forEach((id, i) => { const e = g(id), t = K(26.2 + i * .4); set(e, { s: .7 }); to(e, t, .001, { o: 1 }, E.lin); to(e, t, .4, { s: 1 }, E.ui); if (i === 1) cue('tick', t); });
  { const p2 = g('c_p2'), txt = [...p2.childNodes].find(n => n.nodeType === 3); hook(K(26.6), K(26.6) + .5, p => { txt.textContent = T.tests(Math.round(148 * E.out2(p))); }); }
  to(g('c_l1'), K(27.3), .3, { o: 1 }, E.out2); to(g('c_l2'), K(27.5), .3, { o: 1 }, E.out2);
  set(g('c_l1m'), { o: 0 });
  pose(F('r6'), K(24), 'happy:success');
  // the recording, again: the same scene, and this time one message stays one
  curTo(K(26), 0, [1560, 1240]); curTo(K(27.9), 1.1, g('c_p3'), .5, .7);
  click(K(28), g('c_p3')); curTo(K(29), .7, [2080, 1220]);
  const B = g('clipB'), rb = recording(B, true);
  opensFrom(B, g('c_p3'), K(28), .5);
  swap(rb.a, rb.b, K(28.9), .12); to(rb.bn, K(29.8), .25, { o: 0 }, E.out2);
  hook(K(28), K(30.5), p => { rb.bar.style.width = lerp(40, 92, p) + '%'; });
  closes(B, K(30.3));
  cam(K(30.5) + .55, under(r7, 690, 30), { z: 2.3, sy: 560, d: .65 });
  to(pinr, K(30.5) + .2, .5, { o: 1, y: 0 });
  curTo(K(30.6), 0, [1700, 1230]); curTo(K(31.9), .9, g('approve'), .5, .62);

  /* 6 · approve; the whole product is around it (K(32) → K(39.5)) */
  click(K(32), g('approve'));
  to(pinr, K(32) + .1, .14, { o: 0 }, E.out2); to(pinm, K(32) + .1, .001, { y: 0 }, E.lin); to(pinm, K(32) + .14, .2, { o: 1 }, E.out2);
  swap(g('c_l1s'), g('c_l1m'), K(32) + .2);
  pose(F('r6'), K(32) + .3, 'celebrate:success');
  curTo(K(33.3), .8, [2080, 1180]);
  const ground2 = g('ground2'), ws = g('winshadow'), comp = g('comp'), datep = $('.datep');
  const railKids = [...rail.children];
  const tPull = K(33.2), dPull = 1.0;
  cam(tPull + dPull, [720, 450], { z: 1.14, d: dPull });
  set(ground2, { o: 0 }); to(ground2, tPull + .15, .6, { o: 1 }, E.out2); to(ground2, K(36), .5, { o: 0 }, E.out2);
  set(ws, { o: 0 }); to(ws, tPull + .2, .6, { o: 1 }, E.out2);
  set(sb, { o: 0, x: -40 }); to(sb, tPull + .2, .7, { o: 1, x: 0 });
  set(tb, { o: 0, y: -18 }); to(tb, tPull + .28, .7, { o: 1, y: 0 });
  [comp, datep].forEach(e => { set(e, { o: 0 }); to(e, tPull + .4, .5, { o: 1 }, E.out2); });
  railKids.forEach((e, i) => { set(e, { o: 0, x: 30 }); to(e, tPull + .3 + i * .03, .7, { o: 1, x: 0 }); });
  hook(0, DURATION, () => { rail.style.borderLeftColor = NOW >= tPull + .3 ? '' : 'transparent'; });
  // go in on where the topic's state shows: its chip, its row in the sidebar, the count on 首页
  cam(K(36.4), GEO.stateCam, { z: GEO.stateZ, d: .9 });
  const done = K(37);
  set(g('chipB'), { o: 0 }); swap(g('chipA'), g('chipB'), done); set(g('rstB'), { o: 0 }); swap(g('rstA'), g('rstB'), done);
  { const cb = g('chipB'); to(cb, done, .001, { s: .7 }, E.lin); to(cb, done + .01, .45, { s: 1 }, E.ui); }
  to($('.nm', g('navme')), done, .25, { o: 0 }, E.out2); cue('tick', done);
  { const b4 = g('hb4'), b3 = g('hb3'), bd = g('homebd'), t = K(38); set(b3, { o: 0, y: 8 }); to(b4, t, .16, { o: 0, y: -8 }, E.in2); to(b3, t + .08, .3, { o: 1, y: 0 }); to(bd, t, .1, { s: 1.25 }, E.out2); to(bd, t + .1, .4, { s: 1 }, E.ui); cue('tick', t); }
  // …and the cursor goes to 首页
  curTo(K(38.2), 0, [1500, 1240]); curTo(K(39.4), 1.0, { pt: [72, 114.5] });
  click(K(39.5));

  /* ================= B · and it is one of many things ================= */
  to(matte, 0, .001, { o: 0 }, E.lin); to(matte, K(40), .4, { o: 1 }, E.out2); to(matte, K(90) - .2, .2, { o: 0 }, E.out2);

  /* 7 · 首页: what waits on you, in one place (K(39.5) → K(45)) */
  page('desk', K(39.5));
  cam(K(39.5) + 1.0, GEO.deskCam, { z: GEO.deskZ, d: 1.0 });
  { const c = cover('desk', ...GEO.handled, '#fff'); to(c, K(41.6), .35, { o: 0 }, E.out2); cue('tick', K(41.6)); }   // what you just approved lands under 刚处理
  cap(T.capHome, 40.3, 44.9);
  curTo(K(44.9), 1.1, { pt: GEO.view });
  click(K(45));

  /* 8 · what a member sends you is an interface (K(45) → K(60)) */
  to(camwrap, K(45) + .05, .3, { o: 0, blur: 10 }, E.out2); to(matte, K(45), .3, { o: 0 }, E.out2); to(matte, K(59.6), .3, { o: 1 }, E.out2);
  const KINDS = ['approve', 'draft', 'compare', 'time', 'poll', 'chart'].map((f, i) => { const [w, h, btn] = GEO.cards[f]; return [f, w, h, T.kinds[i], 45 + i * 2.4, btn]; });
  const kindsEl = g('kinds'); kindsEl.innerHTML = KINDS.map(k => `<span><svg viewBox="0 0 16 16"><path d="M3 8.400l3.200 3.200L13 4.600"/></svg>${k[3]}</span>`).join('');
  set(kindsEl, { o: 0 }); to(kindsEl, K(45.6), .4, { o: 1 }, E.out2); to(kindsEl, K(59.4), .2, { o: 0 }, E.in2);
  hook(0, DURATION, () => { [...kindsEl.children].forEach((s, i) => { const k = KINDS[i], on = NOW >= K(k[4]) && (i === KINDS.length - 1 || NOW < K(KINDS[i + 1][4])), c = (on ? 'on ' : '') + (k[5] && NOW >= K(k[4] + 1.55) ? 'done' : ''); if (s._c !== c) { s._c = c; s.className = c; } }); });
  title([T.titleCards], 96, 838, 76, K(45.5), K(59.4));
  let chart;
  KINDS.forEach(([file, w, h, , j, btn], i) => {
    const c = card(file, w, h), t = K(j), el = c.el;
    if (i === 0) {            // the first one opens out of the 看看 it was asked with
      set(el, { x: 612, y: -220, s: .1 }); to(el, t, .001, { o: 1 }, E.lin); to(el, t, .5, { x: 0, y: 0, s: 1 }, E.io);
    } else { set(el, { x: 820, blur: 12 }); to(el, t, .001, { o: 1 }, E.lin); to(el, t, .4, { x: 0, blur: 0 }, E.p4); }
    if (btn) {
      const tc = K(j + 1.55), [bx, by, bw, bh, dark] = btn;
      curTo(tc - .03, .55, c.at(bx + bw * (dark ? .55 : .25), by + bh * .6));
      click(tc); c.press(tc, bx, by, bw, bh, dark);
      to(el, K(j + 2.05), .24, { x: -900, blur: 12 }, E.in2); to(el, K(j + 2.05) + .22, .03, { o: 0 }, E.lin);
    } else chart = c;
  });
  // the last one is a report: clicking it opens the page itself
  curTo(K(59.4) - .03, .6, chart.at(...GEO.chartAt)); click(K(59.4));
  to(chart.el, K(59.45), .42, { s: 2.6, o: 0, blur: 14 }, E.in2);

  /* 9 · 产出: a page you can open, with every version kept (K(60) → K(67)) */
  page('artOld', K(59.5)); cam(K(59.5), [GEO.artX, 240], { z: GEO.artZ, d: 0 });
  to(camwrap, K(59.62), .34, { o: 1, blur: 0 }, E.out2);
  cap(T.capArt, 60.4, 66.9);
  curTo(K(61.9), 1.2, { pt: GEO.backToV3 });
  [[335, 64], [399, 37.5], [436.5, 37], [473.5, 37], [510.5, 37], [547.5, 34.5]].forEach(([y, h], i) => { const c = cover('art', GEO.table[0], y, GEO.table[1], h, '#fff'); to(c, K(62) + .25 + i * .07, .28, { o: 0 }, E.out2); });
  click(K(62)); page('art', K(62) + .03);                     // 回到 v3: the newest version, with what it added
  cam(K(62) + .75, [GEO.artX, 330], { z: GEO.artZ, d: .7 });
  curTo(K(63.6), 1.0, [2080, 1100]);
  cam(K(65.4), [GEO.artX, 760], { z: GEO.artZ, d: .9 });                // down to the question the page itself asks

  /* 10 · 实时查看: step in, hand back (K(67) → K(72)) */
  blurSwap('live', K(67)); cam(K(67), [1041, 762], { z: 2.6, sx: 800, d: 0 });
  cover('live', 0, 0, 1040.5, 900, '#fff'); cover('take', 0, 0, 1040.5, 900, '#fff');     // only the panel: the thread beside it was the first half
  to(matte, K(67) - .16, .16, { o: 0 }, E.in2); to(matte, K(72), .22, { o: 1 }, E.out2);
  title(T.takeover, 96, 405, T.takeoverSize, K(67.4), K(72) - .2, { gap: P * 3.2 });
  curTo(K(67.2), 0, [1500, 1240]); curTo(K(68.9), 1.1, { pt: [1334, 868] });
  click(K(69)); page('take', K(69) + .03);
  cam(K(69) + .8, [1041, 222], { z: 2.6, sx: 800, d: .75 });
  curTo(K(71.3), 1.0, { pt: GEO.handBack });
  click(K(71.4)); page('live', K(71.4) + .03);                // …and hand it back: the panel is Forge's again

  /* 11 · ⌘K: one sentence, to the right member (K(72) → K(77)) */
  blurSwap('ask', K(72)); cam(K(72), [720, 285], { z: 2.5, d: 0 });
  curTo(K(72), .5, [2100, 900]);
  { const A = GEO.ask, N = A.stops.length, t0 = K(72.5), t1 = K(75);
    if (A.caretAt) cover('ask', A.caretAt - .1, 130, 1.4, 30, '#fff');     // the screenshot's own caret, at the start of the line
    const cv = cover('ask', A.x0, 128, A.x1 - A.x0, 35, '#fff'), caret = cover('ask', A.x0, 133, 1.5, 25, '#0a0a0a');
    hook(t0, t1, p => { const n = Math.round(p * N), x = n ? A.stops[n - 1] : A.x0; cv.style.left = x + 'px'; cv.style.width = A.x1 - x + 'px'; caret.style.left = x + 1 + 'px'; caret.style.opacity = p >= 1 ? 0 : 1; });
    cue('typing', t0, t1 - t0);
    [[171, 100], [271, 66], [337, 113]].forEach(([y, h], i) => { const c = cover('ask', 422, y, 596, h, '#fff'); to(c, K(75) + .04 + i * .1, .25, { o: 0 }, E.out2); });   // results answer the finished sentence
    const row = cover('ask', 425, 206, 590, 32, 'rgba(0,0,0,.07)'); row.style.borderRadius = '8px'; set(row, { o: 0 }); to(row, K(76.4), .05, { o: 1 }, E.lin); to(row, K(76.4) + .12, .3, { o: 0 }, E.out2); cue('key', K(76.4)); }
  cap(T.capAsk, 72.4, 76.9);

  /* 12 · a member has a page of its own (K(77) → K(80)) */
  blurSwap('mem', K(77)); cam(K(77), [GEO.memX, 322], { z: 2.3, d: 0 });
  [[633, 328, 138.5, 77.4], [1045, 423, 152, 77.9]].forEach(([x, y, w, j]) => { const c = cover('mem', x, y, w, 4, '#cecece'); c.style.borderRadius = '2px';   // its bars fill as the page opens
    hook(K(j), K(j) + .8, p => { const e = E.p4(p); c.style.left = x + w * e + 'px'; c.style.width = w * (1 - e) + 'px'; }); });
  cap(T.capMember, 77.3, 79.95);

  /* 13 · …and remembers how the team works (K(80) → K(85)) */
  cam(K(80) + .12, [GEO.memX, 470], { z: 2.3, d: .26, e: E.in2 }); page('memo', K(80) + .12);       // a scroll, cut at its fastest frame
  cam(K(80) + .121, [1012, 150], { z: 2.3, d: 0 }); cam(K(80) + .6, [1012, 312], { z: 2.3, d: .47, e: E.p4 });
  [[218, 59], [278, 64], [343, 64]].forEach(([y, h], i) => { const c = cover('memo', 610, y, 800, h, '#fff'), t = K(81 + i); to(c, t, .3, { o: 0 }, E.out2); cue('tick', t); });
  cap(T.capMemory, 80.7, 84.9);

  /* 14 · every member gets a computer (K(85) → K(90)) */
  blurSwap('mach', K(85)); cam(K(85), [320, 196], { z: 2.75, d: 0 });
  [[89, 58], [153, 58], [217, 52]].forEach(([y, h], i) => { const c = cover('mach', 28, y, 584, h, '#f6f6f6'), t = K(86 + i * .7); to(c, t, .3, { o: 0 }, E.out2); });
  cap(T.capMachines, 85.4, 89.9);

  /* 15 · and you put a member together your way (K(90) → K(96)) */
  to(camwrap, K(90) - .18, .18, { o: 0, blur: 10 }, E.in2);
  { const mk = g('mk'), face = k => CHARS[k] ? `<img src="${CHARS[k]}">` : '<i></i>';
    const ROWS = T.mkRows.map(([label, chips], i) => [label, chips, ...[[[0], [2]], [[0], [2]], [[0, 4], [2, 4]]][i]]);
    mk.innerHTML = `<div class="face" id="mkf"><span id="mkf1">${face('o08:happy:idle')}</span><span id="mkf2">${face('h02:happy:idle')}</span></div>
      <div class="nm"><span id="mkn1">Forge<em>${T.mkRoles[0]}</em></span><span id="mkn2">Ledger<em>${T.mkRoles[1]}</em></span></div>
      ${ROWS.map((r, i) => `<div class="rl mono" style="top:${196 + i * 170}px">${r[0]}</div><div class="chips" style="top:${238 + i * 170}px">${r[1].map((c, k) => `<span class="c2" id="mk${i}_${k}">${c}<u>${c}</u></span>`).join('')}</div>`).join('')}
      <div class="os" id="mkos">${T.openSource}</div>`;
    set(mk, { o: 0 }); to(mk, K(90), .3, { o: 1 }, E.out2); to(mk, K(96) - .2, .2, { o: 0, blur: 10 }, E.in2);
    const f = g('mkf'); set(f, { s: .6 }); to(f, K(90), .6, { s: 1 }, E.toy);
    set(g('mkf2'), { o: 0 }); set(g('mkn2'), { o: 0 });
    $$('.rl, .chips', mk).forEach((e, i) => { set(e, { o: 0, x: 24 }); to(e, K(90) + .1 + i * .05, .5, { o: 1, x: 0 }); });
    $$('.c2 u', mk).forEach(u => set(u, { o: 0 }));
    const pick = (i, k, t, on, snd) => { const c = g(`mk${i}_${k}`); to($('u', c), t, .16, { o: on ? 1 : 0 }, E.out2); if (on) { to(c, t, .07, { s: 1.07 }, E.out2); to(c, t + .07, .3, { s: 1 }, E.ui); if (snd) cue('tick', t); } };
    ROWS.forEach((r, i) => { const tA = K(91 + i * .55), tB = K(93.6 + i * .3);
      r[2].forEach((k, n) => pick(i, k, tA, true, n === 0));
      r[2].filter(k => !r[3].includes(k)).forEach(k => pick(i, k, tB, false)); r[3].filter(k => !r[2].includes(k)).forEach(k => pick(i, k, tB, true)); });
    // another member, another set of parts
    const tS = K(93.6); to(f, tS - .12, .12, { sy: .86, sx: 1.08 }, E.out2); to(f, tS, .5, { sy: 1, sx: 1 }, E.toy);
    to(g('mkf1'), tS, .001, { o: 0 }, E.lin); to(g('mkf2'), tS, .001, { o: 1 }, E.lin); to(g('mkn1'), tS, .12, { o: 0 }, E.out2); to(g('mkn2'), tS, .2, { o: 1 }, E.out2);
    hook(K(90), K(96), () => { const a = Math.sin((NOW - K(90)) * 2.4); for (const id of ['mkf1', 'mkf2']) g(id).style.transform = `translateY(${(a * 7).toFixed(2)}px) rotate(${(a * 1.2).toFixed(2)}deg)`; });
    const os = g('mkos'); set(os, { o: 0, s: .7 }); to(os, K(94.9), .001, { o: 1 }, E.lin); to(os, K(94.9), .45, { s: 1 }, E.ui); }
  cap(T.capBuild, 90.3, 95.9);

  /* ================= C · the claim, on the hit; then the mark ================= */
  const [L1, L2] = T.claim, [y1, y2] = T.claimTop;
  title([L1], 0, y1, T.claimSize, K(96.3), K(99), { css: 'left:0;right:0;text-align:center', blur: true, cgap: .04, cut: true });
  const end = g('end');
  end.innerHTML = `<div class="claim" id="ec1" style="top:${y1}px;font-size:${T.claimSize}px">${L1}</div><div class="claim" id="ec2" style="top:${y2}px;font-size:${T.claimSize}px">${L2}</div>
    <div class="lock"><img id="eicon" src="icons/appicon.png"><div id="ename" class="name" style="overflow:hidden;white-space:nowrap">${PRODUCT}</div></div>
    <div class="tag" id="etag">${T.tagline}</div>
    <div class="ctaw"><div class="cta" id="ecta">${T.cta} <span style="font-weight:400">→</span></div></div><div class="sub" id="esub">${T.ctaSub}</div>`;
  const ec1 = g('ec1'), ec2 = g('ec2'), eicon = g('eicon'), ename = g('ename'), etag = g('etag'), ecta = g('ecta'), esub = g('esub'), lock = $('.lock', end);
  chars(ec1);                                                  // set like the line it replaces, so nothing shifts on the cut
  show(end, K(99));                                           // the ground changes on the hit: no ease
  [ec1, ec2].forEach(e => to(e, K(103.3), .22, { o: 0, blur: 10 }, E.in2));
  show(eicon, K(104)); to(eicon, K(104), .001, { s: 1.6 }, E.lin); to(eicon, K(104) + .01, .7, { s: 1 }, E.ui);
  { const W = ename.scrollWidth + 8; hook(K(105.2), K(105.2) + .6, p => { const e = E.p4(p); ename.style.width = W * e + 'px'; lock.style.gap = 44 * e + 'px'; }); }
  set(etag, { o: 0 }); to(etag, K(106.5), .001, { o: 1 }, E.lin); blurIn(etag, K(106.5), { gap: .03 });
  set(ecta, { o: 0, s: .7 }); to(ecta, K(108), .001, { o: 1 }, E.lin); to(ecta, K(108), .5, { s: 1 }, E.ui);
  set(esub, { o: 0 }); to(esub, K(108.8), .5, { o: 1 }, E.out2);
  curTo(K(108.4), 0, [1500, 1240]); curTo(K(109.9), 1.0, { stage: ecta }, .6, .62);
  click(K(110), ecta);

  cue('music', 0, 0);
}

let seeks = 0;
function seek(t) {
  t = clamp(t, 0, DURATION); NOW = t;
  stage.style.setProperty('--t', t);
  let cur = 'app'; for (const p of pages) if (p.t <= t) cur = p.name;
  appwrap.style.display = cur === 'app' ? '' : 'none'; for (const n in plates) plates[n].style.display = n === cur ? '' : 'none';
  for (const [el, rec] of els) applyEl(el, rec, t);
  for (const h of hooks) { const raw = (t - h.t0) / (h.t1 - h.t0); h.fn(clamp(raw), raw); }
  CAM = camAt(t); camEl.style.transform = `translate(${CAM.tx}px,${CAM.ty}px) scale(${CAM.z})`;
  // a fast camera move carries its own blur, on top of the sub-frame samples
  { const c0 = camAt(t - 1 / 60), d = Math.hypot(CAM.tx - c0.tx, CAM.ty - c0.ty) + Math.abs(CAM.z - c0.z) * 500; camEl.style.filter = d > 40 ? `blur(${Math.min(14, d * .1).toFixed(1)}px)` : ''; }
  for (const f of late) f();
  placeCursor(t);
  // headless capture waits for a new frame: one corner pixel changes on every seek
  g('px').style.background = (seeks++ % 2) ? '#000' : '#020202';
}
window.DURATION = DURATION; window.seek = seek; window.CUES = CUES; window.MUSIC = MUSIC;
const faces0 = LANG === 'en' ? ['400 20px "Film SF"', '500 20px "Film SF"', '600 20px "Film SF"', '700 20px "Film SF"', '400 20px "Film SF Mono"'] : [];
window.ready = Promise.all(faces0.map(f => document.fonts.load(f))).then(() => document.fonts.ready).then(async () => {
  build();
  for (const rec of els.values()) rec.anims.sort((a, b) => a.t - b.t);
  pages.sort((a, b) => a.t - b.t); camKeys.sort((a, b) => a.t - b.t); curKeys.sort((a, b) => a.t - b.t); CUES.sort((a, b) => a.t - b.t);
  await Promise.all($$('img').map(i => i.decode().catch(() => {})));
  seek(0);
  if (location.hash !== '#render') {   // preview: fit to the window and play; ←/→ scrub, space pauses
    const fit = () => { stage.style.transform = `scale(${Math.min(innerWidth / 1920, innerHeight / 1080)})`; };
    fit(); addEventListener('resize', fit);
    let t = +(new URLSearchParams(location.search).get('t') || 0), play = true, last = performance.now();
    addEventListener('keydown', e => { if (e.key === ' ') play = !play; if (e.key === 'ArrowRight') t += 1; if (e.key === 'ArrowLeft') t -= 1; });
    const loop = now => { if (play) t = (t + (now - last) / 1000) % DURATION; last = now; seek(t); requestAnimationFrame(loop); };
    requestAnimationFrame(loop);
  }
  return true;
});
