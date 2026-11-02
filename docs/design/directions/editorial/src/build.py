#!/usr/bin/env python3
"""Builds the Warm editorial mockups as self-contained HTML files.

Tokens live once in tokens.css; every screen inlines them, the Lucide icons
(read from the gpui-kit-assets crate that the app ships) and system fonts.
Run: python3 -I src/build.py  (from docs/design/directions/editorial)
"""
import glob
import html
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.dirname(HERE)
ICON_DIRS = glob.glob(os.path.expanduser(
    "~/.cargo/registry/src/*/gpui-kit-assets-0.8.1/assets/icons"))
if not ICON_DIRS:
    sys.exit("gpui-kit-assets-0.8.1 is not in the cargo registry; run cargo fetch")
ICON_DIR = ICON_DIRS[0]

CSS = open(os.path.join(HERE, "tokens.css"), encoding="utf-8").read()
_icon_cache = {}


def icon(name, size=16, cls=""):
    if name not in _icon_cache:
        svg = open(os.path.join(ICON_DIR, name + ".svg"), encoding="utf-8").read()
        body = re.search(r">(.*)</svg>", svg, re.S).group(1)
        _icon_cache[name] = re.sub(r"\s+", " ", body).strip()
    return (f'<svg class="i {cls}" width="{size}" height="{size}" viewBox="0 0 24 24" '
            f'fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" '
            f'stroke-linejoin="round" aria-hidden="true">{_icon_cache[name]}</svg>')


def esc(s):
    return html.escape(s, quote=True)


# ---------------------------------------------------------------- members
# tone: avatar tint family (see tokens.css .tone-*)
MEMBERS = {
    "joye": dict(name="Joye", ab="J", tone="clay", role="负责人", bot=False, presence="on"),
    "zhou": dict(name="周宁", ab="周宁", tone="sage", role="产品经理", bot=False, presence="on"),
    "lin": dict(name="林晓", ab="林晓", tone="slate", role="前端开发", bot=False, presence="on"),
    "chen": dict(name="陈一帆", ab="一帆", tone="ochre", role="后端开发", bot=False, presence="away"),
    "yu": dict(name="小雨", ab="小雨", tone="plum", role="设计", bot=False, presence="off"),
    "code": dict(name="小码", ab="码", tone="ink", role="后端开发", bot=True, presence="on"),
    "test": dict(name="小测", ab="测", tone="sage", role="测试", bot=True, presence="on"),
    "patrol": dict(name="阿巡", ab="巡", tone="slate", role="运维巡检", bot=True, presence="on"),
}


def avatar(mid, size=32, presence=False, ring="var(--content)", members=None):
    m = (members or MEMBERS)[mid]
    shape = "bot" if m["bot"] else "person"
    ab = m["ab"]
    if size <= 20 and len(ab) > 1 and not ab.isascii():
        ab = ab[-1]
    fs = {16: 8, 18: 8.5, 20: 9, 24: 10, 28: 11, 32: 12, 36: 13, 40: 14, 64: 22}.get(size, size * 0.38)
    if len(ab) == 1 and not ab.isascii():
        fs *= 1.12
    mark = ""
    if m["bot"] and size >= 24:
        ms = 12 if size < 32 else 14
        mark = (f'<span class="botmark" style="width:{ms}px;height:{ms}px;box-shadow:0 0 0 1.5px {ring}">'
                f'{icon("bot", ms - 4)}</span>')
    dot = ""
    if presence:
        ds = 8 if size <= 24 else 10
        dot = (f'<span class="presence p-{m["presence"]}" style="width:{ds}px;height:{ds}px;'
               f'box-shadow:0 0 0 2px {ring}"></span>')
    return (f'<span class="av {shape} tone-{m["tone"]}" style="width:{size}px;height:{size}px;'
            f'font-size:{fs:.1f}px">{esc(ab)}{mark}{dot}</span>')


def short_name(name, limit=12):
    """Chip names: at most 6 CJK or 12 Latin columns, then an ellipsis; the verb stays."""
    w, out = 0, ""
    for ch in name:
        w += 1 if ord(ch) < 0x2E80 else 2
        if w > limit:
            return out.rstrip() + "…"
        out += ch
    return out


def chip(kind, text, quiet=False):
    q = " quiet" if quiet else ""
    if text.startswith("等") and text[-1] in "审答" and len(text) > 3:
        verb = "回答" if text.endswith("回答") else "审"
        text = "等" + short_name(text[1:len(text) - len(verb)]) + verb
    return f'<span class="chip st-{kind}{q}">{esc(text)}</span>'


def traffic(active=True):
    cls = "" if active else " inactive"
    return f'<div class="traffic{cls}"><i></i><i></i><i></i></div>'


# ---------------------------------------------------------------- sidebar
def sidebar(data):
    rows = []
    rows.append(f'''
    <div class="sb-top">{traffic(data.get("active", True))}</div>
    <div class="sb-search">{icon("search", 14)}<span>搜索</span><kbd>⌘K</kbd></div>''')
    if data.get("banner"):
        rows.append(f'<div class="sb-banner">{icon("wifi-off", 13)}<span>{esc(data["banner"])}</span></div>')
    rows.append('<div class="sb-section"><span>待办</span><span class="sb-count num">'
                f'{len(data["todo"])}</span></div>')
    for t in data["todo"]:
        rows.append(f'''<div class="sb-todo"><span class="todo-mark {t["kind"]}">{icon(t["icon"], 12)}</span>
          <span class="sb-todo-main"><span class="sb-todo-title">{esc(t["title"])}</span>
          <span class="sb-todo-sub">{esc(t["sub"])}</span></span></div>''')
    rows.append('<div class="sb-section"><span>群</span></div>')
    for g in data["groups"]:
        sel = " selected" if g.get("selected") else ""
        unread = " unread" if (g.get("count") or g.get("mention")) else ""
        badge = ""
        if g.get("mention"):
            badge += f'<span class="badge mention num">@{esc(str(g["mention"]))}</span>'
        elif g.get("count"):
            badge += f'<span class="badge num">{esc(str(g["count"]))}</span>'
        if g.get("muted"):
            badge += f'<span class="muted-ic">{icon("bell-off", 12)}</span>'
        rows.append(f'''<div class="sb-row{sel}{unread}"><span class="mono tone-{g["tone"]}">{esc(g["mono"])}</span>
          <span class="sb-label">{esc(g["name"])}</span>{badge}</div>''')
    rows.append('<div class="sb-section"><span>私聊</span></div>')
    for d in data["dms"]:
        badge = f'<span class="badge num">{d["count"]}</span>' if d.get("count") else ""
        unread = " unread" if d.get("count") else ""
        rows.append(f'''<div class="sb-row dm{unread}">{avatar(d["id"], 20, presence=True, ring="var(--sidebar)", members=data.get("members"))}
          <span class="sb-label">{esc((data.get("members") or MEMBERS)[d["id"]]["name"])}</span>{badge}</div>''')
    rows.append(f'''<div class="sb-foot">{avatar("joye", 24, presence=True, ring="var(--sidebar)")}
      <span class="sb-me"><b>Joye</b><small>负责人</small></span>
      <span class="iconbtn">{icon("settings", 15)}</span></div>''')
    return '<aside class="sidebar">' + "".join(rows) + "</aside>"


# ---------------------------------------------------------------- topic list
def topic_list(data):
    out = [f'''<section class="topics">
      <header class="tl-head"><div><h2>{esc(data["group"])}</h2>
        <p class="num">{esc(data["group_meta"])}</p></div>
        <span class="iconbtn" title="新话题">{icon("square-pen", 16)}</span></header>
      <div class="tl-rows">''']
    for t in data["topics"]:
        sel = " selected" if t.get("selected") else ""
        un = " unread" if t.get("unread") else ""
        ch = chip(t["status"], t["chip"], t.get("quiet", False)) if t.get("chip") else ""
        cnt = f'<span class="badge num">{t["unread"]}</span>' if t.get("unread") else ""
        if t.get("mention"):
            cnt = f'<span class="badge mention num">@{t["mention"]}</span>' + cnt
        out.append(f'''<div class="tl-row{sel}{un}">
          <div class="tl-l1"><span class="tl-title">{esc(t["title"])}</span><time class="num">{esc(t["time"])}</time></div>
          <div class="tl-l2">{ch}<span class="tl-prev"><b>{esc(t["who"])}</b>{esc(t["prev"])}</span>{cnt}</div>
        </div>''')
    out.append("</div></section>")
    return "".join(out)


# ---------------------------------------------------------------- timeline
def msg(mid, time, body, cont=False, members=None, extra_cls=""):
    m = (members or MEMBERS)[mid]
    if cont:
        return f'''<div class="msg cont {extra_cls}"><span class="gutter-time num">{esc(time)}</span>
          <div class="msg-body">{body}</div></div>'''
    role = f'<span class="role">{esc(m["role"])}</span>' if m.get("role") else ""
    return f'''<div class="msg {extra_cls}">{avatar(mid, 32, members=members)}
      <div class="msg-main"><div class="msg-head"><span class="author">{esc(m["name"])}</span>{role}
      <time class="num">{esc(time)}</time></div><div class="msg-body">{body}</div></div></div>'''


def ack(mid, time, text, members=None):
    m = (members or MEMBERS)[mid]
    return f'''<div class="msg ack-line"><span class="ack-av">{avatar(mid, 20, members=members)}</span>
      <div class="ack-text"><b>{esc(m["name"])}</b><span>{esc(text)}</span><time class="num">{esc(time)}</time></div></div>'''


def mention(name, me=False):
    return f'<span class="at{" me" if me else ""}">@{esc(name)}</span>'


def code_block(lang, code_html):
    return f'''<div class="code"><div class="code-head"><span>{lang}</span><span class="iconbtn sm">{icon("copy", 13)}</span></div>
      <pre>{code_html}</pre></div>'''


CHART_SVG = '''<svg viewBox="0 0 440 220" width="440" height="220" class="chart" aria-hidden="true">
  <rect width="440" height="220" fill="var(--img-bg)"/>
  <g stroke="var(--img-grid)" stroke-width="1">
    <line x1="44" y1="40" x2="420" y2="40"/><line x1="44" y1="90" x2="420" y2="90"/>
    <line x1="44" y1="140" x2="420" y2="140"/><line x1="44" y1="190" x2="420" y2="190"/></g>
  <g font-size="10" fill="var(--img-label)" font-family="-apple-system,'PingFang SC'" style="font-variant-numeric:tabular-nums">
    <text x="12" y="44">100%</text><text x="16" y="94">99%</text><text x="16" y="144">98%</text><text x="16" y="194">97%</text>
    <text x="44" y="212">10/28</text><text x="150" y="212">10/30</text><text x="256" y="212">11/01</text><text x="362" y="212">11/03</text></g>
  <path d="M44 62 L80 58 L116 66 L152 61 L188 112 L224 150 L260 141 L296 168 L332 120 L368 72 L404 52 L420 49"
     fill="none" stroke="var(--img-line)" stroke-width="2" stroke-linejoin="round" stroke-linecap="round"/>
  <path d="M332 120 L368 72 L404 52 L420 49" fill="none" stroke="var(--img-line2)" stroke-width="2.4" stroke-linecap="round"/>
  <circle cx="332" cy="120" r="3.5" fill="var(--img-line2)"/>
  <text x="300" y="108" font-size="10" fill="var(--img-label)" font-family="-apple-system,'PingFang SC'">修复上线</text>
  <text x="56" y="28" font-size="11" font-weight="600" fill="var(--img-title)" font-family="-apple-system,'PingFang SC'">支付回调成功率（按小时）</text>
</svg>'''


def status_card(title, chip_html, fields, primary, secondary, icon_name="git-pull-request"):
    f = "".join(f'<div class="f"><dt>{esc(k)}</dt><dd class="num">{v}</dd></div>' for k, v in fields)
    return f'''<div class="card status-card">
      <div class="sc-head">{icon(icon_name, 16, "sc-ic")}<span class="sc-title">{esc(title)}</span>{chip_html}</div>
      <dl class="sc-fields">{f}</dl>
      <div class="sc-actions"><button class="btn primary">{esc(primary)}</button>
      <button class="btn secondary">{esc(secondary)}</button></div></div>'''


def artifact_card(title, meta, kind_icon="file-text"):
    return f'''<div class="card artifact-card"><span class="art-tile">{icon(kind_icon, 18)}</span>
      <span class="art-main"><b>{esc(title)}</b><small class="num">{esc(meta)}</small></span>
      <span class="art-open">{icon("external-link", 14)}<span>打开</span></span></div>'''


def timeline_main():
    ts = '<span class="tk">const</span> <span class="fn">nextDelay</span> = (attempt: <span class="ty">number</span>) =>\n'
    ts += '  Math.<span class="fn">min</span>(<span class="nu">30_000</span> * <span class="nu">2</span> ** attempt, <span class="nu">6</span> * HOUR)\n\n'
    ts += '<span class="tk">if</span> (attempt >= MAX_ATTEMPTS) <span class="tk">return</span> <span class="fn">toManualReview</span>(event)'
    parts = []
    parts.append('<div class="day"><span>今天 · 11月3日</span></div>')
    parts.append(msg("zhou", "09:40",
        f'<p>{mention("小码")} 支付宝的回调这周又丢了 3 笔。能不能把重试改成指数退避？上限 6 次，间隔从 30 秒开始，失败的进对账队列。</p>'))
    parts.append(ack("code", "09:40", "收到，我来改。预计 20 分钟内提交。"))
    parts.append(msg("lin", "09:52",
        '<blockquote><b>周宁</b>上限 6 次，间隔从 30 秒开始</blockquote>'
        '<p>6 次可能不够。微信支付的文档建议重试到 24 小时，按这个间隔第 6 次才 16 分钟。我倾向 8 次，封顶 6 小时一次。</p>'))
    parts.append(msg("code", "10:31",
        '<p>改好了。重试间隔按 30 秒起翻倍，第 8 次之后进人工对账，核心逻辑如下：</p>'
        + code_block("TypeScript", ts)
        + status_card("PR #482 支付回调改为指数退避重试", chip("review", "等你审"),
                      [("分支", '<code>fix/pay-callback-retry</code>'), ("检查", '全部通过 12/12'),
                       ("改动", '<span class="plus">+184</span> <span class="minus">−37</span>')],
                      "批准合并", "请求修改")
        + artifact_card("重试策略说明", "页面 · 第 2 版 · 刚刚更新")))
    parts.append('<div class="newdiv"><span>新消息</span></div>')
    parts.append(msg("lin", "10:40",
        '<p>回调成功率这两天的曲线，修复上线后应该能回到 99.9% 以上。</p>'
        f'<figure class="img">{CHART_SVG}</figure>'))
    parts.append(msg("test", "10:44",
        f'<p>{mention("周宁")} 我在 staging 上回放了上周的 1,286 笔回调。前 200 笔里有 2 笔第一次重试仍然超时，第二次都成功了；'
        '正在看剩下的部分<span class="caret"></span></p>', extra_cls="streaming"))
    return "".join(parts)


def timeline(title, chip_html, body, composer_html, typing="周宁 正在输入…", members_stack=("zhou", "lin", "code", "test"), anchor="bottom"):
    stack = "".join(avatar(m, 20, ring="var(--content)") for m in members_stack)
    typing_html = (f'<div class="typing"><span class="dots"><i></i><i></i><i></i></span>{esc(typing)}</div>'
                   if typing else "")
    return f'''<main class="timeline">
      <header class="tv-head"><div class="tv-title"><h1>{esc(title)}</h1>{chip_html}</div>
        <div class="tv-tools"><span class="stack">{stack}<span class="more num">+4</span></span>
        <span class="iconbtn">{icon("bell", 16)}</span><span class="iconbtn on">{icon("panel-right", 16)}</span></div></header>
      <div class="tv-scroll {anchor}"><div class="tv-col">{body}</div></div>
      {'<div class="jump">' + icon("arrow-down", 13) + '<span class="num">2 条新消息</span></div>' if anchor == "top" else ""}
      <div class="tv-bottom">{typing_html}{composer_html}</div>
    </main>'''


def composer(text_html, popup=True, placeholder=False):
    pop = ""
    if popup:
        pop = f'''<div class="popover mention-pop">
          <div class="mp-row active">{avatar("code", 24, ring="var(--surface-hi)")}<span class="mp-name"><mark>小</mark>码</span><span class="mp-role">后端开发</span>{icon("corner-down-left", 13, "mp-enter")}</div>
          <div class="mp-row">{avatar("test", 24, ring="var(--popover)")}<span class="mp-name"><mark>小</mark>测</span><span class="mp-role">测试</span></div>
          <div class="mp-row">{avatar("yu", 24, ring="var(--popover)")}<span class="mp-name"><mark>小</mark>雨</span><span class="mp-role">设计 · 离线</span></div>
        </div>'''
    body = f'<span class="ph">写消息，Enter 发送，Shift+Enter 换行</span>' if placeholder else text_html
    return f'''<div class="composer-wrap">{pop}<div class="composer{" focused" if popup else ""}">
      <div class="cm-text">{body}</div>
      <div class="cm-bar"><span class="iconbtn sm">{icon("paperclip", 15)}</span><span class="iconbtn sm">{icon("image", 15)}</span>
      <span class="iconbtn sm">{icon("at-sign", 15)}</span><span class="grow"></span>
      <span class="send">{icon("arrow-up", 15)}</span></div></div></div>'''


# ---------------------------------------------------------------- side panel
def side_panel():
    arts = [("file-text", "重试策略说明", "页面 · 第 2 版"),
            ("file-spreadsheet", "callback-replay-1103.csv", "文件 · 184 KB"),
            ("image", "回调成功率.png", "图片 · 1280×640"),
            ("scale", "重试上限定为 8 次", "决定 · 周宁、林晓")]
    a = "".join(f'''<div class="sp-art"><span class="art-tile sm">{icon(i, 14)}</span>
        <span class="art-main"><b>{esc(t)}</b><small class="num">{esc(m)}</small></span></div>''' for i, t, m in arts)
    steps = [("done", "读取回调处理代码", "10:02"), ("done", "修改 6 个文件", "10:18"),
             ("done", "运行测试 12/12", "10:27"), ("done", "提交 PR #482", "10:31"),
             ("wait", "等你审", "")]
    s = "".join(f'''<li class="{k}"><span class="stp"></span><span>{esc(t)}</span><time class="num">{tm}</time></li>''' for k, t, tm in steps)
    return f'''<aside class="panel">
      <header class="sp-head"><h3>详情</h3><span class="iconbtn sm">{icon("x", 14)}</span></header>
      <div class="sp-scroll">
      <dl class="sp-meta">
        <div><dt>状态</dt><dd>{chip("review", "等你审")}</dd></div>
        <div><dt>负责人</dt><dd>{avatar("code", 20, ring="var(--panel)")}<span>小码</span></dd></div>
        <div><dt>在等</dt><dd>{avatar("joye", 20, ring="var(--panel)")}<span>你</span></dd></div>
        <div><dt>成员</dt><dd class="sp-stack">{"".join(avatar(x, 20, ring="var(--panel)") for x in ("zhou","lin","chen","code","test"))}<span class="more num">+3</span></dd></div>
      </dl>
      <div class="sp-label"><span>产出</span><span class="num">4</span></div>{a}
      <div class="sp-label"><span>工作过程</span><span>小码</span></div>
      <ol class="steps">{s}</ol>
      </div></aside>'''


# ---------------------------------------------------------------- data sets
MAIN_SIDEBAR = dict(
    todo=[dict(kind="review", icon="git-pull-request", title="支付回调重试方案", sub="开发 · 等你审"),
          dict(kind="at", icon="at-sign", title="周末值班表", sub="运维 · 阿巡 @你")],
    groups=[dict(name="全员", mono="全", tone="sand", count=3),
            dict(name="开发", mono="开", tone="clay", selected=True, mention=1),
            dict(name="产品", mono="产", tone="sage", count=12),
            dict(name="运维", mono="运", tone="slate", mention=1),
            dict(name="设计评审", mono="设", tone="plum", muted=True)],
    dms=[dict(id="lin", count=2), dict(id="code"), dict(id="chen"), dict(id="yu")])

MAIN_TOPICS = dict(
    group="开发", group_meta="9 位成员 · 28 个话题",
    topics=[
        dict(title="支付回调重试方案", status="review", chip="等你审", time="10:44", who="小测：", prev="我在 staging 上回放了上周的…", unread=2, selected=True),
        dict(title="消息搜索支持两字词", status="progress", chip="进行中", time="10:15", who="林晓：", prev="分词表换成新词典以后，「支付」…", unread=4, mention=1),
        dict(title="登录页改版的文案", status="answer", chip="等林晓回答", quiet=True, time="09:58", who="周宁：", prev="邀请码那一行要不要再短一点？"),
        dict(title="iOS 推送证书过期", status="blocked", chip="卡住了", time="昨天", who="阿巡：", prev="Apple 后台续期失败，需要账号持有人…"),
        dict(title="打包 DMG 的签名问题", status="review", chip="等陈一帆审", quiet=True, time="昨天", who="小码：", prev="已改成 ad-hoc 签名，安装说明…"),
        dict(title="新同学的开发环境", time="周一", who="Joye：", prev="清单放在产出里了，有问题直接问。"),
        dict(title="周报自动汇总", status="done", chip="完成", time="周一", who="小码：", prev="以后每周五 18:00 发到全员群。"),
        dict(title="告警降噪：合并重复的超时告警", status="done", chip="完成", time="10月30日", who="阿巡：", prev="同类告警 10 分钟内只发一次。"),
    ])


def window(inner, theme, extra_cls=""):
    return f'<div class="win {extra_cls}">{inner}</div>'


def page(title, body, theme="light", w=1440, h=900, extra_css=""):
    return f'''<!doctype html><html lang="zh-Hans" data-theme="{theme}"><head><meta charset="utf-8">
<meta name="viewport" content="width={w}"><title>{esc(title)}</title>
<style>{CSS}
html,body{{width:{w}px;height:{h}px}}
{extra_css}</style></head><body>{body}</body></html>'''


def main_window(theme, sb=None, tl=None, tv_title="支付回调重试方案", tv_chip=None, tv_body=None,
                comp=None, panel=True, typing="周宁 正在输入…", active=True, anchor="bottom"):
    sbd = dict(sb or MAIN_SIDEBAR)
    sbd["active"] = active
    tv = timeline(tv_title, tv_chip if tv_chip is not None else chip("review", "等你审"),
                  tv_body if tv_body is not None else timeline_main(),
                  comp if comp is not None else composer('<p>补充一点：对账队列满 50 笔时 '
                      '<span class="at draft">@小</span><span class="ime-caret"></span></p>'),
                  typing=typing, anchor=anchor)
    cls = "with-panel" if panel else ""
    return (f'<div class="win main {cls}">{sidebar(sbd)}{topic_list(tl or MAIN_TOPICS)}{tv}'
            f'{side_panel() if panel else ""}</div>')


# ---------------------------------------------------------------- settings
def settings_page(theme):
    nav = [("user-round", "我的资料"), ("users", "成员"), ("monitor", "机器"), ("bell", "通知"),
           ("wallet", "成本"), ("stethoscope", "诊断"), ("code-xml", "开发者视图")]
    n = "".join(f'<div class="st-nav-row{" selected" if t == "成员" else ""}">{icon(i, 15)}<span>{t}</span>'
                f'{"<small>即将推出</small>" if t == "成本" else ""}</div>' for i, t in nav)
    rows = [("joye", "管理员", "在线", ""), ("zhou", "管理员", "在线", ""), ("lin", "普通成员", "在线", ""),
            ("chen", "普通成员", "离开", ""), ("yu", "普通成员", "离线", ""),
            ("code", "管理员", "在线", "机器 · Mac mini"), ("test", "普通成员", "在线", "机器 · Mac mini"),
            ("patrol", "管理员", "在线", "外部接入")]
    r = ""
    for mid, lvl, pres, how in rows:
        m = MEMBERS[mid]
        me = '<span class="you">你</span>' if mid == "joye" else ""
        how_html = f'<small>{esc(how)}</small>' if how else ""
        r += f'''<div class="mt-row">{avatar(mid, 28, presence=True, ring="var(--content)")}
          <span class="mt-name"><b>{esc(m["name"])}{me}</b><small>{esc(m["role"])}</small></span>
          <span class="mt-how">{how_html}</span>
          <span class="select">{esc(lvl)}{icon("chevron-down", 13)}</span>
          <span class="iconbtn sm">{icon("ellipsis", 15)}</span></div>'''
    dialog = f'''<div class="overlay"></div>
      <div class="dialog" role="dialog">
        <div class="dlg-head"><h3>邀请成员</h3><span class="iconbtn sm">{icon("x", 14)}</span></div>
        <p class="dlg-text">把这个邀请码发给新成员。它只能用一次，24 小时后失效，关闭后不会再显示。</p>
        <div class="code-field"><span class="num">TC7K-QM4X-PA93</span><span class="copied">{icon("check", 13)}已复制</span></div>
        <dl class="dlg-meta"><div><dt>权限</dt><dd><span class="select">普通成员{icon("chevron-down", 13)}</span></dd></div>
          <div><dt>有效期</dt><dd class="num">至 11月4日 10:52</dd></div></dl>
        <div class="dlg-foot"><button class="btn ghost">再生成一个</button><span class="grow"></span>
          <button class="btn secondary">完成</button><button class="btn primary">{icon("copy", 13)}复制邀请码</button></div>
      </div>'''
    swin = f'''<div class="settings-win">
      <div class="st-titlebar">{traffic(True)}<span>设置</span></div>
      <div class="st-body"><nav class="st-nav">{n}</nav>
      <section class="st-content"><div class="st-head"><div><h2>成员</h2><p class="num">5 位成员 · 3 位自动成员</p></div>
        <span class="grow"></span><button class="btn secondary">{icon("bot", 14)}添加自动成员</button>
        <button class="btn primary">{icon("user-plus", 14)}邀请成员</button></div>
        <div class="mt">{r}</div></section></div>{dialog}</div>'''
    bg = main_window(theme, active=False, panel=False, comp=composer("", popup=False, placeholder=True), typing="")
    return f'<div class="desk">{bg}<div class="desk-dim"></div>{swin}</div>'


# ---------------------------------------------------------------- login
def login_page(theme):
    return f'''<div class="win login"><div class="lg-top">{traffic(True)}</div>
      <div class="lg-col">
        <div class="lg-icon">{ICON_SVG}</div>
        <h1>欢迎使用 TeamChat</h1>
        <p class="lg-sub">用管理员发给你的邀请码登录。登录后，凭证保存在这台 Mac 的钥匙串里。</p>
        <label class="fld"><span>服务器地址</span><div class="input"><span class="num">chat.example.com</span></div></label>
        <label class="fld"><span>邀请码或登录凭证</span><div class="input focus"><span class="num">TC7K-QM4X-PA9</span><span class="ime-caret"></span></div>
          <small>向管理员要一个邀请码，邀请码只能用一次</small></label>
        <button class="btn primary lg">登录</button>
      </div>
      <p class="lg-foot num">版本 0.4.0</p></div>'''


# ---------------------------------------------------------------- switcher
def switcher_page(theme):
    base = main_window(theme, comp=composer("", popup=False, placeholder=True), typing="")
    res = [
        ("话题", [("git-pull-request", "<mark>支付</mark>回调重试方案", "开发", chip("review", "等你审"), True),
                 ("message-square-text", "<mark>支付</mark>宝对账差异汇总", "产品", chip("done", "完成"), False),
                 ("message-square-text", "<mark>支付</mark>页面的错误提示", "产品", chip("answer", "等周宁回答", True), False)]),
        ("群", [("hash", "<mark>支付</mark>与风控", "6 位成员", "", False)]),
        ("产出", [("file-text", "重试策略说明", "页面 · 开发 · <mark>支付</mark>回调重试方案", "", False)]),
    ]
    g = ""
    for label, items in res:
        g += f'<div class="cmd-group">{label}</div>'
        for ic, t, sub, ch, act in items:
            g += f'''<div class="cmd-row{" active" if act else ""}">{icon(ic, 15, "cmd-ic")}
              <span class="cmd-title">{t}</span>{ch}<span class="cmd-sub">{sub}</span></div>'''
    pal = f'''<div class="cmd">
      <div class="cmd-input">{icon("search", 16)}<span class="q">支付</span><span class="ime-caret"></span><span class="grow"></span><kbd>esc</kbd></div>
      <div class="cmd-list">{g}</div>
      <div class="cmd-foot"><span><kbd>↑</kbd><kbd>↓</kbd>选择</span><span><kbd>↵</kbd>打开</span><span><kbd>⌘</kbd><kbd>↵</kbd>在新窗口打开</span></div></div>'''
    return f'<div class="desk">{base}<div class="cmd-scrim"></div>{pal}</div>'


# ---------------------------------------------------------------- worst case
WC_MEMBERS = dict(MEMBERS)
WC_MEMBERS.update({
    "al": dict(name="Aleksandra Wiśniewska-Kowalczyk", ab="AW", tone="plum", role="Senior Product Design Engineer, Platform Infrastructure", bot=False, presence="on"),
    "wang": dict(name="王", ab="王", tone="ochre", role="实习", bot=False, presence="away"),
    "fox": dict(name="🦊 Fox", ab="F", tone="clay", role="设计", bot=False, presence="off"),
    "longbot": dict(name="夜间构建与发布流水线守护者（staging + production）", ab="守", tone="ink", role="自动发布", bot=True, presence="on"),
    "j": dict(name="J", ab="J", tone="sage", role="", bot=False, presence="on"),
})


def worst_page(theme):
    sb = dict(
        banner="连接已断开，正在重连…",
        members=WC_MEMBERS,
        todo=[dict(kind="review", icon="git-pull-request", title="跨部门联合项目：2026 Q4 支付与风控合规整改专项的第二阶段验收清单", sub="跨部门联合项目临时沟通群（2026 Q4 支付与风控合规整改专项） · 等你审"),
              dict(kind="at", icon="at-sign", title="🦊🦊🦊", sub="王 @你")],
        groups=[dict(name="跨部门联合项目临时沟通群（2026 Q4 支付与风控合规整改专项）", mono="跨", tone="sand", count="999+", mention="99+", selected=True),
                dict(name="全", mono="全", tone="clay", count=1),
                dict(name="Benachrichtigungseinstellungen", mono="B", tone="sage", count=58),
                dict(name="🚀🚀 发布 🚀🚀", mono="发", tone="slate", muted=True, count=999),
                dict(name="ops-alerts-production-cn-north-1-critical", mono="o", tone="plum")],
        dms=[dict(id="al", count="999+"), dict(id="wang"), dict(id="fox", count=1), dict(id="longbot"), dict(id="j")])
    tl = dict(group="跨部门联合项目临时沟通群（2026 Q4 支付与风控合规整改专项）", group_meta="1,284 位成员 · 12,048 个话题",
              topics=[
        dict(title="跨部门联合项目：2026 Q4 支付与风控合规整改专项的第二阶段验收清单（含历史遗留问题）", status="review", chip="等Aleksandra Wiśniewska-Kowalczyk审", quiet=True, time="10:44", who="Aleksandra Wiśniewska-Kowalczyk：", prev="https://example.com/workspaces/acme/projects/q3-launch/docs/9f8e7d6c5b4a?tab=comments", unread="999+", mention="99+", selected=True),
        dict(title="王", status="answer", chip="等你回答", time="10:15", who="王：", prev="？"),
        dict(title="Benachrichtigungseinstellungen_und_Datenschutzrichtlinien_v12_FINAL", status="blocked", chip="卡住了", time="09:58", who="J：", prev="IMG_20250914_183022_HDR_portrait_edited_edited.HEIC"),
        dict(title="🦊🦊🦊🦊🦊🦊🦊🦊🦊🦊🦊🦊🦊🦊🦊🦊🦊🦊", status="progress", chip="进行中", time="昨天", who="🦊 Fox：", prev="👩🏽‍💻👩🏽‍💻👩🏽‍💻"),
        dict(title="（无标题）", time="10月12日", who="夜间构建与发布流水线守护者（staging + production）：", prev="&lt;script&gt;alert(1)&lt;/script&gt; **不应加粗**"),
        dict(title="Q3 Board Deck — FINAL (revised) v12 [approved by legal].pdf", status="done", chip="完成", time="2025年12月31日", who="Joye：", prev="1"),
    ])
    long_url = "https://example.com/workspaces/acme/projects/q3-launch/docs/9f8e7d6c5b4a?tab=comments&amp;filter=unresolved&amp;highlight=9f8e7d6c-5b4a-4c3d-8e2f-1a0b9c8d7e6f"
    body = '<div class="day"><span>2025年12月31日</span></div>'
    body += msg("al", "23:30", f'<p>{mention("夜间构建与发布流水线守护者（staging + production）")} {mention("王")} {mention("Joye", me=True)} 请看这个链接，第二阶段的验收项都在里面：</p><p class="url"><a>{long_url}</a></p>', members=WC_MEMBERS)
    body += msg("wang", "23:31", '<p>？</p>', members=WC_MEMBERS)
    body += msg("j", "23:32", '<p>AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA</p>', members=WC_MEMBERS)
    body += ack("longbot", "23:31", "收到。我先跑一遍全部检查，结果出来再回到这里。", members=WC_MEMBERS)
    body += '<div class="day"><span>今天 · 11月3日</span></div>'
    body += msg("longbot", "10:44", status_card("Benachrichtigungseinstellungen_und_Datenschutzrichtlinien_v12_FINAL_approved_by_legal 发布到 production-cn-north-1", chip("blocked", "卡住了"),
        [("分支", '<code class="trunc">release/2026-11-03-hot…risk-control-compliance</code>'), ("检查", '1,283/1,284 通过'),
         ("改动", '<span class="plus">+12,345,678</span> <span class="minus">−9,999</span>')],
        "重新运行全部检查并发布到 production", "取消"), members=WC_MEMBERS)
    body += msg("fox", "10:46", '<p>👩🏽‍💻👩🏽‍💻👩🏽‍💻 🦊 &lt;script&gt;alert(1)&lt;/script&gt; **不应加粗** &amp;amp;</p>', members=WC_MEMBERS)
    body += '<div class="newdiv"><span>新消息</span></div>'
    body += msg("al", "10:51", artifact_card("Q3 Board Deck — FINAL (revised) v12 [approved by legal] — 第二阶段验收清单与风控合规整改说明.pdf", "文件 · 第 128 版 · 99.8 MB", "file-text"), members=WC_MEMBERS)
    comp = composer('<p>' + "这是一段很长的草稿，" * 6 + '<span class="ime-caret"></span></p>', popup=False)
    return main_window(theme, sb=sb, tl=tl, tv_title=tl["topics"][0]["title"],
                       tv_chip=chip("review", "等Aleksandra Wiśniewska-Kowalczyk审", True), tv_body=body, comp=comp,
                       panel=False, typing="3 人正在输入…")


# ---------------------------------------------------------------- icon
ICON_SVG = open(os.path.join(HERE, "icon.svg"), encoding="utf-8").read()
ICON_SVG_SMALL = open(os.path.join(HERE, "icon-small.svg"), encoding="utf-8").read()


def icon_sheet():
    def tile(svg, s):
        return f'<div class="ic-tile" style="width:{s}px;height:{s}px">{svg}</div>'
    rows = ""
    for bgname in ("light", "dark"):
        cells = "".join(f'<div class="ic-cell">{tile(ICON_SVG if s >= 64 else ICON_SVG_SMALL, s)}<small class="num">{s} px</small></div>'
                        for s in (128, 64, 32, 16))
        rows += f'<div class="ic-row ic-{bgname}"><span class="ic-lbl">{"浅色" if bgname=="light" else "深色"}</span>{cells}</div>'
    return f'''<div class="ic-sheet"><div class="ic-hero">{tile(ICON_SVG, 520)}
      <div class="ic-copy"><h1>「 」</h1><p>两只直角引号是中文里的“对话”。左边陶土色，右边墨色：成员之间来回说话，内容在中间。</p>
      <p class="num">1024 × 1024 · macOS 圆角矩形网格 824 px · 小尺寸用加粗的简化版</p></div></div>{rows}</div>'''


def write(name, content):
    path = os.path.join(OUT, name)
    with open(path, "w", encoding="utf-8") as f:
        f.write(content)
    print("wrote", os.path.relpath(path, OUT))


def build():
    write("main-light.html", page("主窗口 浅色", main_window("light"), "light"))
    write("main-dark.html", page("主窗口 深色", main_window("dark", anchor="top"), "dark"))
    write("settings-members.html", page("设置 成员", settings_page("light"), "light"))
    write("settings-members-dark.html", page("设置 成员 深色", settings_page("dark"), "dark"))
    write("login.html", page("登录", login_page("light"), "light"))
    write("login-dark.html", page("登录 深色", login_page("dark"), "dark"))
    write("switcher.html", page("快速切换", switcher_page("light"), "light"))
    write("switcher-dark.html", page("快速切换 深色", switcher_page("dark"), "dark"))
    write("worst-case.html", page("极端数据", worst_page("light"), "light"))
    write("worst-case-dark.html", page("极端数据 深色", worst_page("dark"), "dark"))
    write("icon.html", page("应用图标", icon_sheet(), "light", 1440, 1180))
    write("icon-1024.html", page("应用图标 1024", f'<div style="width:1024px;height:1024px">{ICON_SVG}</div>',
                                 "light", 1024, 1024, "body{background:transparent}"))


if __name__ == "__main__":
    build()
