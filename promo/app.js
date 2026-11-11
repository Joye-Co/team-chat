/* Builds the product's topic screen as live DOM (see app.css). Labels and data are the product's own
   sample team (copy.js): the topic 断线重连后消息重复 (Reconnect duplicates) in the group 开发. */
const IC = {
  side: '<rect x="1.5" y="2.5" width="13" height="11" rx="2.5"/><path d="M6 2.5v11"/>',
  filter: '<path d="M2.5 4.5h11M4.5 8h7M6.5 11.5h3"/>',
  compose: '<path d="M13.5 8.5v3.2a1.8 1.8 0 0 1-1.8 1.8H4.3a1.8 1.8 0 0 1-1.8-1.8V4.3a1.8 1.8 0 0 1 1.8-1.8h3.2"/><path d="M12.3 2.3l1.4 1.4-5 5-1.9.5.5-1.9z"/>',
  search: '<circle cx="7" cy="7" r="4.5"/><path d="M10.4 10.4l3.1 3.1"/>',
  home: '<path d="M2.2 8.5 4 4.2a1.2 1.2 0 0 1 1.1-.7h5.8a1.2 1.2 0 0 1 1.1.7l1.8 4.3v2.8a1.2 1.2 0 0 1-1.2 1.2H3.4a1.2 1.2 0 0 1-1.2-1.2z"/><path d="M2.4 8.5h3.2l.7 1.5h3.4l.7-1.5h3.2"/>',
  box: '<rect x="3.2" y="5" width="9.6" height="8.5" rx="1.8"/><path d="M5 5V3.6h6V5M5.6 2.2h4.8"/>',
  people: '<circle cx="5.6" cy="6" r="2"/><circle cx="11" cy="6.4" r="1.6"/><path d="M1.8 12.4c.4-2 1.9-3 3.8-3s3.4 1 3.8 3M10 9.6c1.9-.4 3.6.6 4.1 2.6"/>',
  clock: '<circle cx="8" cy="8" r="5"/><path d="M8 5.2V8H5.6"/>',
  panel: '<rect x="1.5" y="2.8" width="13" height="10.4" rx="2.4"/><path d="M10 2.8v10.4"/>',
  chat: '<path d="M2 4.3A1.8 1.8 0 0 1 3.8 2.5h6A1.8 1.8 0 0 1 11.6 4.3v3.2a1.8 1.8 0 0 1-1.8 1.8H6l-2.4 2V9.3A1.8 1.8 0 0 1 2 7.5z"/><path d="M13 6.2c.8.2 1.4.9 1.4 1.8v2.6c0 .9-.6 1.6-1.4 1.8v1.5l-2-1.5H8.4"/>',
  merge: '<path d="M5 13.5V4M5 4 2.8 6.2M5 4l2.2 2.2M5 8.5c0 2.5 2 3 4 3"/>',
  play: '<path d="M5 3.4v9.2l7.4-4.6z"/>',
  check: '<path d="M3 8.4l3.2 3.2L13 4.6"/>',
  branch: '<circle cx="4" cy="3.6" r="1.4"/><circle cx="12" cy="3.6" r="1.4"/><path d="M4 5v1.2c0 1.6 4 1.4 4 3.4V13M12 5v1.2c0 1.6-4 1.4-4 3.4"/>',
  doc: '<path d="M4 2.5h5l3 3v7.2a.8.8 0 0 1-.8.8H4a.8.8 0 0 1-.8-.8V3.3A.8.8 0 0 1 4 2.5z"/><path d="M9 2.5v3h3M5.6 8.4h4.8M5.6 10.8h4.8"/>',
  out: '<path d="M4.5 11.5l7-7M6 4.5h5.5V10"/>',
  down: '<path d="M4 6l4 4 4-4"/>',
  updown: '<path d="M5 6.2 8 3.4l3 2.8M5 9.8l3 2.8 3-2.8"/>',
  x: '<path d="M3.5 3.5l9 9M12.5 3.5l-9 9"/>',
  plus: '<path d="M8 3v10M3 8h10"/>',
  at: '<circle cx="8" cy="8" r="2.4"/><path d="M10.4 5.6v3.2c0 1.900 3 1.900 3-.8a5.4 5.4 0 1 0-2 4.200"/>',
  up: '<path d="M8 12.500V3.800M4.400 7.200 8 3.600l3.600 3.600"/>',
  spin: '<path d="M8 1.800v2.400M8 11.800v2.400M1.800 8h2.400M11.800 8h2.400M3.600 3.600l1.700 1.700M10.700 10.700l1.700 1.700M3.600 12.400l1.700-1.700M10.700 5.300l1.700-1.700"/>',
};
const ic = n => `<i class="ic"><svg viewBox="0 0 16 16">${IC[n]}</svg></i>`;
const spin = () => `<span class="spin"><svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round">${IC.spin}</svg></span>`;
/* an avatar: people are monogram circles; AI members are their character when the pack is present */
const BOTS = { Jojo: 'jojo', Forge: 'o08', Scout: 'o07', Tata: 'o03', Watch: 'o11', Ledger: 'h02', Taro: 'h03', Pocket: 'o05' };
function av(name, letter, cls = '') {
  const id = BOTS[name];
  return `<span class="av ${id ? 'bot' : ''} ${cls}" ${id ? `data-bot="${id}"` : ''}>${letter}</span>`;
}

function buildApp() {
  const [S, Q] = [T.sec, T.topics], R = T.role, [W, WL] = T.wang, [C, CL] = T.chen;
  const navs = [
    ['sec', S[0], 206], ['t', 'spin', Q[0], 226.5], ['t', 'warn', Q[1], 258.5, 'sel'], ['t', 'warn', Q[2], 290.5], ['t', 'clock', Q[3], 322.5],
    ['sec', S[1], 366], ['t', 'warn', Q[4], 386.5], ['t', '', Q[5], 418.5],
    ['sec', S[2], 462], ['t', 'warn', Q[6], 482.5], ['t', 'spin', Q[7], 514.5], ['t', '', Q[8], 546.5],
    ['sec', S[3], 590], ['t', 'spin', Q[9], 610.5],
    ['sec', S[4], 654],
  ];
  const mark = k => k === 'spin' ? spin() : k === 'warn' ? '<span class="dotw" style="margin:0 3px"></span>' : k === 'clock' ? `<span class="m" style="display:flex">${ic('clock')}</span>` : '<span style="width:16px;flex:none"></span>';
  const sb = `<div class="sb grp-chrome" id="sb">
    <div class="tl3"><i></i><i></i><i></i></div><span class="t1 m">${ic('side')}</span><span class="t2">${ic('filter')}</span><span class="t3">${ic('compose')}</span>
    <div class="search">${ic('search')}${T.search}</div>
    <div class="nav" style="top:98.5px">${ic('home')}${T.home}<span class="bd" id="homebd"><span id="hb4">3</span><span id="hb3" style="position:absolute">2</span></span></div>
    <div class="nav" style="top:130.5px"><span class="m" style="display:flex">${ic('box')}</span>${T.artifacts}</div>
    <div class="nav" style="top:162.5px"><span class="m" style="display:flex">${ic('people')}</span>${T.team}</div>
    ${navs.map(n => n[0] === 'sec' ? `<div class="sec" style="top:${n[2]}px">${n[1]}</div>` : `<div class="nav ${n[4] || ''}" style="top:${n[3]}px" ${n[4] ? 'id="navme"' : ''}><span class="nm" style="display:flex;width:16px;justify-content:center">${n[4] ? spin() : mark(n[1])}</span>${n[2]}</div>`).join('')}
    <div class="nav" style="top:674.5px">${av('Jojo', 'J', 's16')}Jojo</div>
    <div class="nav" style="top:706.5px">${av(W, WL, 's16')}${W}</div>
    <div class="me">${av('Joye', 'J')}<div><div class="b6">Joye</div><div class="sm m">${R.owner}</div></div><span class="up">${ic('updown')}</span></div>
  </div>`;

  const tb = `<div class="tb grp-chrome" id="tb"><span class="m">${S[0]}</span><span class="m" style="display:flex;transform:rotate(-90deg)">${ic('down')}</span>
    <span class="ttl">${Q[1]}</span><span id="tchip" style="display:inline-grid"><span class="chip" id="chipA" style="grid-area:1/1;justify-self:start">${spin()}${T.merging}</span><span class="chip" id="chipB" style="grid-area:1/1;justify-self:start"><span class="okc"></span>${T.done}</span></span>
    <div class="acts"><span class="tbb on">${ic('panel')}${T.tabs[0]}</span><span class="tbb">${ic('box')}${T.tabs[1]}</span><span class="tbb">${ic('chat')}${T.tabs[2]}</span><span class="tbb">${ic('merge')}${T.tabs[3]}</span></div></div>`;

  const thread = `<div class="thread" id="thread"><div class="datep grp-chrome">${T.date}</div>
   <div class="tflow" id="tflow">
    <div class="row" id="r1"><div class="msg">${av(W, WL)}<div><div class="hd"><span class="b6">${W}</span><span class="sm m">${R.fe}</span><span class="sm m">09:12</span></div>
      <div class="tx" id="r1tx">${T.r1}</div>
      <div class="file" id="file1"><span class="pl" id="file1pl">${ic('play')}</span><div><div class="b5">${T.file}</div><div class="sm m">${T.fileMeta}</div></div></div></div></div></div>
    <div class="row" id="r2"><div class="sysl">${T.r2}&nbsp; 09:13</div></div>
    <div class="row" id="r3"><div class="msg">${av('Forge', 'F')}<div><div class="hd"><span class="b6">Forge</span><span class="sm m">${R.dev}</span><span class="sm m">09:14</span></div><div class="tx">${T.r3}</div></div></div></div>
    <div class="row" id="r3b"><div class="work" id="work">${av('Forge', 'F', 's24')}<span class="b5">${T.working}<span id="workstep">${T.railSteps[0][0]}</span></span><span class="r"><span class="chip" id="workchip">${spin()}${T.workChip}&nbsp;<span><span id="workn">1</span>/5</span></span><span class="sm m" id="openlive">${T.openLive}</span></span></div></div>
    <div class="row" id="r4"><div class="msg">${av('Forge', 'F')}<div><div class="hd"><span class="b6">Forge</span><span class="sm m">${R.dev}</span><span class="sm m">09:30</span></div><div class="tx">${T.r4}</div></div></div></div>
    <div class="row" id="r5"><div class="recl">${ic('check')}<span>${T.r5} 09:31</span></div></div>
    <div class="row" id="r6"><div class="msg">${av('Forge', 'F')}<div><div class="hd"><span class="b6">Forge</span><span class="sm m">${R.dev}</span><span class="sm m">09:55</span></div>
      <div class="card" id="card"><span class="chip" id="c_chip" style="height:20px;padding:0 8px 0 6px"><span class="okc"></span>${T.delivered}</span>
        <h4 id="c_h">${T.cardH}</h4>
        <div class="tx" id="c_tx">${T.cardTx}</div>
        <div class="pf"><span id="c_p1">${ic('branch')}#61 +214 −63</span><span id="c_p2"><span class="okc"></span>${T.tests(148)}</span><span id="c_p3">${ic('play')}${T.rec}</span></div>
        <div class="ln first" id="c_l1"><span class="m" style="display:flex">${ic('merge')}</span><span class="b5">${T.pr}</span><span class="m">${T.prTx}</span><span class="r"><span style="display:inline-grid"><span id="c_l1s" style="grid-area:1/1;justify-self:end">${T.prOpen}</span><span id="c_l1m" style="grid-area:1/1;justify-self:end">${T.prMerging}</span></span>${ic('out')}</span></div>
        <div class="ln" id="c_l2"><span class="m" style="display:flex">${ic('doc')}</span><span class="b5">${T.art}</span><span class="sm m">${T.artMeta}</span><span class="r">${ic('out')}</span></div>
        <div class="foot"></div></div></div></div></div>
    <div class="row" id="r7"><div class="msg" style="padding-top:8.5px;padding-bottom:0">${av(W, WL)}<div><div class="hd"><span class="b6">${W}</span><span class="sm m">${R.fe}</span><span class="sm m">10:20</span></div><div class="tx">${T.r7}</div></div></div></div>
   </div>
   <div class="pin" id="pinq"><span class="dotw"></span><div><div class="b6">${T.askH}</div><div class="m" style="margin-top:2.5px">${T.askSub}</div></div>
     <div class="acts"><span class="btn" id="optA">${T.optA}</span><span class="btn pri" id="optB">${T.optB}</span></div></div>
   <div class="pin" id="pinm"><span style="display:flex;color:var(--info);align-self:flex-start;margin-top:15.5px">${ic('merge')}</span><div><div class="b6">${T.mergedH}</div><div class="m" style="margin-top:2.5px">${T.justNow}</div></div>
     <div class="acts"><span class="btn">${T.undo}</span></div></div>
   <div class="pin" id="pin"><span class="dotw"></span><div><div class="b6">${T.reviewH}</div><div class="m" style="margin-top:2.500px">${T.reviewSub}</div></div>
     <div class="acts"><span class="btn">${T.viewChanges}</span><span class="btn">${T.requestChanges}</span><span class="btn pri" id="approve">${T.approve}</span></div></div>
   <div class="comp grp-chrome" id="comp"><span class="ph">${T.compose}</span><div class="tools">${ic('plus')}${ic('at')}</div><span class="send">${ic('up')}</span></div>
  </div>`;

  const mem = [['Forge', 'F', R.dev, T.owns], [W, WL, R.fe, T.requested], ['Alex Chen', 'A', R.be], ['Joye', 'J', R.owner], [C, CL, R.design], ['Jojo', 'J', R.front], ['Scout', 'S', R.qa], ['Tata', 'T', R.hop], ['Watch', 'W', R.ops]];
  const rail = `<div class="rail grp-chrome" id="rail"><div class="rh">${T.info}<span class="x">${ic('x')}</span></div>
    <div class="give">${T.handTo}</div>
    <div class="box st" id="rst"><span id="rstA" style="display:flex;align-items:center;gap:7px">${spin()}${T.merging}</span><span id="rstB" style="position:absolute;left:24px;display:flex;align-items:center;gap:7px"><span class="okc"></span>${T.done}</span></div>
    <div class="box rm">${T.reminders}<span class="selb">${T.following}${ic('down')}</span></div>
    <div class="hint">${T.remindHint}</div>
    <div class="h" style="top:302.5px">${T.team}${ic('down')}</div>
    <div class="box mem" id="mem">${mem.map((m, i) => `<div class="mr" id="mem${i}">${av(m[0], m[1], 's16')}<span>${m[0]}</span><span class="m">${m[2]}</span>${m[3] ? `<span class="r">${m[3]}</span>` : ''}</div>`).join('')}</div>
    <div class="h" style="top:682.5px">${T.plan}${ic('down')}</div>
    <div class="box plan"><div class="p0">${T.allDone}<span>${T.planTotal}</span></div><div class="bar"><i></i></div>
      ${T.railSteps.map(s => `<div class="ps"><span style="display:flex;color:var(--mfg)">${ic('check')}</span>${s[0]}<span class="r">${s[1]}</span></div>`).join('')}</div>
  </div>`;

  return `<div class="app" id="app"><div class="win" id="win"></div>${sb}${tb}${thread}${rail}</div>`;
}
window.buildApp = buildApp; window.APPIC = ic; window.APPSPIN = spin; window.APPAV = av;
