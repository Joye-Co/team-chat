# Mockups v3 (TeamChat 1.1.0)

The artboards of the v3 redesign that Joye approved on 2026-11-03. They are the visual truth for
[docs/DESIGN.md](../../DESIGN.md) v3. When this folder and DESIGN.md disagree on a value, DESIGN.md
wins; when DESIGN.md says nothing, the artboard wins.

Each artboard is a 1440 pt wide page. The PNGs are renders at device scale 1, in light and dark
mode. The `.dc.html` files are the source of each artboard (design-canvas format). They load
`./support.js` and the character art from `/_blob/...` URLs of the design canvas, so they do not
render on their own from this folder: read them for exact values (sizes, tokens, copy, motion
keyframes), and look at the PNGs for the result. The `<helmet>` block at the top of every file
holds the HELMET2 tokens (DESIGN.md §2).

| Artboard | Title | Size | Light | Dark | Shows |
| --- | --- | --- | --- | --- | --- |
| [Main.dc.html](Main.dc.html) | 首页 | 1440 × 1160 | [PNG](Main-light@1x.png) | [PNG](Main-dark@1x.png) | Rail A, 首页: Jojo's one-line priority, 需要你拍板 cards (approval, PR review, draft, choice), 做完了 today, the 正在做 column with a live thumbnail, 排队 |
| [Topic.dc.html](Topic.dc.html) | 话题 | 1440 × 900 | [PNG](Topic-light@1x.png) | [PNG](Topic-dark@1x.png) | Topic page: title bar with state, timeline, a done card with proof, the pinned decision card above the 96 pt composer, the topic rail (负责, 计划, 改动, 产出, 用到的决定, 电脑, 用时和成本) |
| [SideChat.dc.html](SideChat.dc.html) | 单独问 | 1440 × 900 | [PNG](SideChat-light@1x.png) | [PNG](SideChat-dark@1x.png) | The topic page with the 单独问 pane (380 pt): member picker, privacy note, answer with ref chips, the suggestion button that posts into the topic |
| [Live.dc.html](Live.dc.html) | 实时查看 | 1440 × 900 | [PNG](Live-light@1x.png) | [PNG](Live-dark@1x.png) | The topic page with the 电脑 pane: the member's screen, 回放 scrubber, 验收标准, 接管 / 停止 footer; the expanded plan in the timeline |
| [Ask.dc.html](Ask.dc.html) | 交代或搜索 | 1440 × 900 | [PNG](Ask-light@1x.png) | [PNG](Ask-dark@1x.png) | ⌘K over 首页: the 交代 rows (交给 Ledger ↵, 交给 Jojo 安排 ⌘↵) and search results (话题, 产出, 成员) |
| [Group.dc.html](Group.dc.html) | 群 | 1440 × 900 | [PNG](Group-light@1x.png) | [PNG](Group-dark@1x.png) | 群 page for 开发: composer, 进行中 cards with real previews, 最近完成, the 群资料 rail (群里的决定, 最近的产出, 电脑) |
| [Members.dc.html](Members.dc.html) | 成员 | 1440 × 900 | [PNG](Members-light@1x.png) | [PNG](Members-dark@1x.png) | 成员 page: filtered list (等你, 在做事, 空闲, 同事) and the Forge detail (现在, 这周, 最近交付, 最近的问题) with the 工作环境 rail |
| [Artifact.dc.html](Artifact.dc.html) | 产出 | 1440 × 900 | [PNG](Artifact-light@1x.png) | [PNG](Artifact-dark@1x.png) | 产出 viewer: version picker, the page in its frame with an in-page choice, the 来龙去脉 rail (来自话题, 版本, 页面里的回应, 用到的决定, 被引用) |
| [RailOptions.dc.html](RailOptions.dc.html) | 侧栏的三种方案 | 1440 × 900 | [PNG](RailOptions-light@1x.png) | [PNG](RailOptions-dark@1x.png) | The three rail modes side by side: A 以工作为中心, B 以「需要我」为中心, C 以成员为中心. All three ship; the user picks one in 设置 → 外观 |
| [Cards.dc.html](Cards.dc.html) | 生成式界面组件 | 1440 × 6860 | [PNG](Cards-light@1x.png) | [PNG](Cards-dark@1x.png) | Card v2: the card anatomy, the three colors, and the 12 kinds (选择, 批准, 草稿, 表单, 计划, 数据, 对比, 证据, 链接, 轻回应, 交互产出, 通知) in every state |

Not here: the landing page (not in 1.1.0), login and settings (unchanged layout from
[mockups-v2](../mockups-v2/), restyled with the v3 tokens).
