# Mockups v3 (TeamChat 1.1.0)

The artboards of the v3 redesign (approved 2026-11-03). They are the visual truth for
[docs/DESIGN.md](../../DESIGN.md) v3. When this folder and DESIGN.md disagree on a value, DESIGN.md
wins; when DESIGN.md says nothing, the artboard wins.

Each artboard is a 1440 pt wide page. The `.dc.html` files are the source of each artboard (design-canvas format). They load
`./support.js` and the character art from `/_blob/...` URLs of the design canvas, so they do not
render on their own from this folder: read them for exact values (sizes, tokens, copy, motion
keyframes). The `<helmet>` block at the top of every file
holds the HELMET2 tokens (DESIGN.md §2).

| Artboard | Title | Size | Shows |
| --- | --- | --- | --- |
| [Main.dc.html](Main.dc.html) | 首页 | 1440 × 1160 | Rail A, 首页: Jojo's one-line priority, 需要你拍板 cards (approval, PR review, draft, choice), 做完了 today, the 正在做 column with a live thumbnail, 排队 |
| [Topic.dc.html](Topic.dc.html) | 话题 | 1440 × 900 | Topic page: title bar with state, timeline, a done card with proof, the pinned decision card above the 96 pt composer, the topic rail (负责, 计划, 改动, 产出, 用到的决定, 电脑, 用时和成本) |
| [SideChat.dc.html](SideChat.dc.html) | 单独问 | 1440 × 900 | The topic page with the 单独问 pane (380 pt): member picker, privacy note, answer with ref chips, the suggestion button that posts into the topic |
| [Live.dc.html](Live.dc.html) | 实时查看 | 1440 × 900 | The topic page with the 电脑 pane: the member's screen, 回放 scrubber, 验收标准, 接管 / 停止 footer; the expanded plan in the timeline |
| [Ask.dc.html](Ask.dc.html) | 交代或搜索 | 1440 × 900 | ⌘K over 首页: the 交代 rows (交给 Ledger ↵, 交给 Jojo 安排 ⌘↵) and search results (话题, 产出, 成员) |
| [Group.dc.html](Group.dc.html) | 群 | 1440 × 900 | 群 page for 开发: composer, 进行中 cards with real previews, 最近完成, the 群资料 rail (群里的决定, 最近的产出, 电脑) |
| [Members.dc.html](Members.dc.html) | 成员 | 1440 × 900 | 成员 page: filtered list (等你, 在做事, 空闲, 同事) and the Forge detail (现在, 这周, 最近交付, 最近的问题) with the 工作环境 rail |
| [Artifact.dc.html](Artifact.dc.html) | 产出 | 1440 × 900 | 产出 viewer: version picker, the page in its frame with an in-page choice, the 来龙去脉 rail (来自话题, 版本, 页面里的回应, 用到的决定, 被引用) |
| [RailOptions.dc.html](RailOptions.dc.html) | 侧栏的三种方案 | 1440 × 900 | The three rail modes side by side: A 以工作为中心, B 以「需要我」为中心, C 以成员为中心. All three ship; the user picks one in 设置 → 外观 |
| [CardsCompose.dc.html](CardsCompose.dc.html) | 组合和选择题 | 1440 × 5720 | Card v2.1: free composition of blocks (上下叠, 并排, 分组, 对比, 问卷, 标签页), the choice forms (单选, 多选, 排序, 打分, 量表, 确认, 图片选择, 矩阵, 时间段, 投票) and the extension rules |
| [Interact.dc.html](Interact.dc.html) | 每次操作都回到成员 | 1440 × 4936 | The interaction loop: every pick, chart point, table row, input and button goes back to the member, which changes the card in place (pending band, attribution line, conflict); 产出 pages that talk both ways |
| [Refs.dc.html](Refs.dc.html) | 引用提问和图 | 1440 × 6900 | 引用提问 (问这里, ⌥-click, chips in the composer, several parts at once, what the member receives) and diagrams in messages and cards (expand viewer, error and loading states) |
| [Media.dc.html](Media.dc.html) | 产出的格式 | 1440 × 8100 | 产出 media inline and full: live HTML, Markdown, images, video with frames and chapters, audio with waveform and transcript, PDF, code and tables, upload and preview states |
| [Cards.dc.html](Cards.dc.html) | 生成式界面组件 | 1440 × 7560 | Card v2: the card anatomy, the status band on every card (等你, 等别人, 处理中, 完成, 失败, 已过期, 已作废), the three colors, and the 12 kinds (选择, 批准, 草稿, 表单, 计划, 数据, 对比, 证据, 链接, 轻回应, 交互产出, 通知) in every state |

Not here: the landing page (not in 1.1.0), login and settings (unchanged layout from
[mockups-v2](../mockups-v2/), restyled with the v3 tokens).
