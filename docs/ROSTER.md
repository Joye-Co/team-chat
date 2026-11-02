# 机器人成员名册（暂定）

> **暂定**：这是第一版名册。外形来自 Joye 的 jojo-friends 角色家族；除 Jojo 外，岗位和职责
> 暂时照抄团队 Notion 的「团队名册」，Joye 之后会调整。数据文件是
> [`assets/bots/roster.json`](../assets/bots/roster.json)，两处要一起改。
> 机器人怎么接入（令牌、SDK、运行器）见 [bots.md](bots.md)。

jojo-friends 是 Joye 的私有个人 IP（`/Users/joye/Documents/code/jojo-friends`，未授权）：只在
本产品内使用，不要把它的源码或图发布到别处，也不要改动或提交到那个仓库。

## 名册

| 成员 | 角色 | 岗位 | 级别 | 汇报给 | 职责 | 项目 |
| --- | --- | --- | --- | --- | --- | --- |
| Jojo | `jojo` Jojo | 主助手 | owner | Joye | 团队的主助手：默认入口，把工作转给对应的成员，也做通用工作；运行在 Mac mini 上 | — |
| Tata | `o03` 塔塔 | 产品负责人 | member | Joye | 夹夹 / 粉丝群业务负责人：qqbot 负责人并负责产品；jiajia.fun、share.jiajia.fun；拉新群（数据飞轮）；管理 R2 桶 | qqbot、jiajia.fun |
| Pocket | `o05` 兜兜 | 幕僚长 | member | Joye | 幕僚长 / 总管：Joye 的统一入口（邮件、日程、待办、提醒、晨报），把事情转给对应 bot；维护名册和总入口；负责决策卡和 qqbot 合并卡；博客、GitHub 与个人品牌（原 Hugo 职责） | 博客、个人品牌、Joye-Co GitHub 组织、总入口与名册 |
| Forge | `o08` 薇薇 | 开发 | member | Tata | qqbot 开发：处理 issue、派 Cursor cloud agent、审 PR、清测试债（#59–#67）、修 CI 失败 | qqbot |
| Watch | `o11` 蒂奇 | 运维 | member | Tata | qqbot 运维：告警（#424）、staging 验收、事故复盘、上云（#317）、密钥和权限检查；推生产前先给 Joye 确认卡 | qqbot |
| Scout | `o07` 点点 | 测试验收 | member | Tata | qqbot QA：每天只读巡查 staging；推生产前在 staging 验完这一批并在群里确认 | qqbot |
| Ledger | `h02` 便当 | 数据 | member | Tata | qqbot 数据：报表、埋点追踪（#430）、知识库和审核队列、evals、成本 | qqbot |
| Taro | `h03` 芋圆 | 社媒发布 | member | Pocket | 社媒内容发布：把 Joye 的博客、项目和观点改写后排期发到 X、博客、B 站、小红书、抖音；每条发出前先给 Joye 确认 | 个人品牌（社媒发布） |

- Jojo 不在 Notion 名册里，它的职责是 Joye 定的。Jojo 跑在 Mac mini 上（运行器标签
  `mac-mini`）；用哪个编码 CLI（运行时）在装 daemon 时再定，所以 `roster.json` 里
  `runner.runtime` 是 `null`。
- 「汇报给」和「项目」只记在名册里，服务器不存。

## 头像和状态

成员的 `character`（协议里的 `Member.character`，26 个 jojo-friends id 之一）决定头像画哪个角色。
桌面端用角色的状态信号表示机器人此刻在做什么（`apps/desktop/src/characters/`）：

| 状态 | 什么时候 | 表情 |
| --- | --- | --- |
| offline | 机器人不在线（或已停用） | sleepy |
| working | 正在流式写一条回复 | focus |
| needs-input | 它负责的话题在等人回答或审（`waiting_answer`、`waiting_review`） | curious |
| error | 它负责的话题卡住了（`blocked`） | puzzled |
| success | 回复写完或它负责的话题变成完成后约 8 秒 | happy |
| idle | 其他时候 | calm |

按表中顺序取第一个符合的。没有角色的成员（人）保留原来的字母头像。每个状态配一个温和的表情；
jojo-friends 的规范说状态本身不改表情，所以表情是本产品的选择，在 `scripts/characters/render.tsx`
的 `PAIR` 里。

图是预先渲染好的 SVG：26 个角色 × 6 个状态 × 7 个尺寸（16、20、24、32、48、64、128 pt），放在
`apps/desktop/assets/characters/<id>/<status>-<size>.svg`，16 到 32 pt 用库自带的小尺寸简化画法。
重新渲染（会同时生成名册联系表 `~/.cache/team-chat/design/characters/roster-contact-sheet.png`，
不进 git）：

```sh
scripts/characters/render.sh            # 默认读取 ~/Documents/code/jojo-friends
```

## 改一个机器人的角色或岗位

1. 改 `assets/bots/roster.json` 和本文件的表。
2. 在每个环境里改服务器上的成员（只有 owner 能改角色，会写审计日志，所有在线客户端马上收到）：

```sh
bun server/scripts/admin.ts members --env staging                 # 找到成员 id（最后一列是角色）
bun server/scripts/admin.ts set-member --env staging --member <id> --character o12
bun server/scripts/admin.ts set-member --env staging --member <id> --role "新岗位"
bun server/scripts/admin.ts set-member --env staging --member <id> --character none   # 去掉角色
```

3. 换成 26 个之外的新角色是协议变更：先在 `packages/protocol/src/characters.ts`、
   `crates/chat-protocol/src/characters.rs` 和 `docs/protocol.md` 里加 id，再渲染图。

## 在一个环境里创建这些成员

需要该环境的 owner 令牌（`admin.ts bootstrap` 写在 `~/.config/team-chat/<env>/owner-token`）。
`<env>` 是 `local`、`e2e`、`staging` 或 `production`。每条 `bot` 命令打印新成员的 id 和令牌文件的
路径（不打印令牌本身）。

```sh
A="bun server/scripts/admin.ts"
E="--env staging"

$A bot $E --name Jojo   --role 主助手     --character jojo
$A bot $E --name Tata   --role 产品负责人 --character o03
$A bot $E --name Pocket --role 幕僚长     --character o05
$A bot $E --name Forge  --role 开发       --character o08
$A bot $E --name Watch  --role 运维       --character o11
$A bot $E --name Scout  --role 测试验收   --character o07
$A bot $E --name Ledger --role 数据       --character h02
$A bot $E --name Taro   --role 社媒发布   --character h03

# Jojo 的级别是 owner（新机器人默认是 member）。<jojo-id> 是上面打印的 id。
$A set-member $E --member <jojo-id> --level owner

# 装好 Mac mini 上的 daemon、选好运行时之后，让 Jojo 跑在那台机器上：
$A runner $E --bot <jojo-id> --runtime <codex|claude> --labels mac-mini
```

已经存在的机器人不要再 `bot` 一次（会多出一个同名成员），用 `set-member` 补上角色和岗位。
