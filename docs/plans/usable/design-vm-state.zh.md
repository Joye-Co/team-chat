# 设计：虚拟机里留下什么（usable 线 8，B）

> 只管「每个任务开一台 Firecracker 虚拟机」的电脑（daemon 配了 `[vm]`）。不跑虚拟机的电脑（个人 Mac、Windows、用户自己的 VPS）保持真实家目录，不变。对所有电脑都生效的只有两条：CLI 自带的记忆关掉（§3.6），续接不上会话时补发完整提示（§3.2）。代码位置以 origin/main 716cd316 为准。

## 1 目标

负责人 2026-11-12 原话：

- 「这个也只是在用了firecracker vm这一情况下需要讨论的, 如果是用户自己的vps或者电脑等, 其实就永久保存就可以了」
- 「依赖缓存这种东西应该是跟着项目的」
- 「20gb对于rust这种依赖物很大的也不一定够,而且现在家目录没有清除机制」

| 归属 | 留下什么 | 什么时候清 |
| --- | --- | --- |
| 话题（工作线） | 代码、编译结果、CLI 会话（每个话题一套 `CLAUDE_CONFIG_DIR` / `CODEX_HOME` / `PI_CODING_AGENT_DIR`） | 跟工作线一起：空闲 72 小时，先把没提交的改动提交到分支再删 |
| 仓库 | 依赖缓存（cargo registry、sccache、npm/pnpm、pip/uv）和用户级工具（rustup 工具链、`cargo install`、bun、`npm -g`、pipx/uv tool），这个仓库所有成员、所有话题共用 | 主机磁盘不够时按最久没用先删；没有每个成员的固定大小 |
| 电脑 | 订阅登录、GitHub 机器账号 | 不进虚拟机，每次用时交进去（GitHub 归 C） |
| 基础镜像 | apt 库、docker | 管理员维护 |
| 其余 | 家目录里的其他东西、`/tmp` | 跟虚拟机一起没 |

所有 AI 成员的运行，在每台电脑上都关掉 CLI 自带的记忆；仓库里的 AGENTS.md / CLAUDE.md 照读，记忆只在产品里。#227 的「每个成员 20 GB 家目录镜像」和 root 辅助命令去掉。

## 2 现状

**成员家目录（#227）**
- `apps/daemon/src/vm_home.rs:1-17`：`<homes>/<member>` 是成员在每台虚拟机里的 `$HOME`；`home_gb>0`（默认 20，`config.rs:120-122`）时是 `runner-vm home-up` 挂上的 ext4 镜像（`vm_home.rs:106-127`）。`[vm].homes=""` 时 CLI 目录在 `<[vm].dir>/cli/<member>`（`vm_home.rs:94-103`）。`home.clear` 删镜像（`vm_home.rs:129-141`，`daemon.rs:1225`）。
- `config.rs:62-101`：`VmConfig` 用 `deny_unknown_fields`（`config.rs:63`），`homes`、`home_gb` 在 `config.rs:91-101`。
- `run.rs:538-541` 每次虚拟机运行前 `vm_home::ensure`；`run.rs:567-590` 用成员目录调 `vm_login::prepare`；`run.rs:543-549` 每次先 `github::clear_home`（`github.rs:585`），因为家目录会保留。
- `executor/firecracker.rs:305,504` 的 `bot_home` 把成员目录挂成 guest 的 `HOME`；`firecracker.rs:547-575` 的 `shares()`：运行目录、工作目录、git dir、成员目录、`[vm].shares`，并按前缀去重。
- `daemon.rs:158,625,1341` 的 `clearing`、功能 `homes`、`metrics.homes`；`health.rs:342-349` 的 `run_disk` 取工作盘和家目录盘里更满的；`doctor.rs:113` 查旧 helper。
- 协议：`docs/protocol.md` 的「Machine homes」、`server/src/monitor/homes.ts`、`server/src/bots/link.ts:1056-1061`、`packages/protocol/src/machine.ts`、`crates/chat-protocol/src/machine.rs`、fixtures `down-home.clear.json`、`up-heartbeat-homes.json`。桌面端没有界面。
- `scripts/runner/runner-vm:8-17` 的 `home-up` / `home-clear`；`runner-vm:139-150` 的 `export` 只许 `/srv/runner` 和 runner 家目录下的路径，拒绝 `images`、`kernels`、`home-images`、`homes` 本身。`host-setup.sh:67-74` 建 `homes`、`home-images`，还建一个没人用的 `/srv/runner/cache`（`host-setup.sh:69`）。
- `run.rs:76-98` 的 `vm_note` 告诉 CLI「家目录在所有运行之间保留……共 20 GB」。

**会话续接**
- 服务器续接话题里同类最后一次结束的运行的会话（`server/src/bots/desk/runs.ts:645-660`），包括单独问（没有工作线），也包括因工作线被占而用了自己运行目录的运行。发了 `resumeSessionId` 就把提示换成只含新内容的 `prompt_resume`（`runs.ts:838-840,870-872`）。
- daemon 找不到会话时悄悄开新会话：Claude `runtime/claude.rs:769-797`（`prepare_resume` 失败就 `--session-id`），Codex `thread/resume` 失败退回 `thread/start`（`runtime/codex.rs:325,481-490`），ACP 只在第一句提示里说「会话是新的」（`runtime/acp.rs:213`）。三种都只拿到短提示，CLI 不知道之前发生过什么。#227 之前家目录永久保留，这个问题很少出现；会话跟着工作线以后，72 小时后回来就一定出现。
- `vm_login.rs:46` 的 `cli_env(member)` 按成员分目录，同一成员几个话题同时跑共用一个目录；`vm_login.rs:207,252` 从主机目录拷会话；`vm_login.rs:342,391` 的 Codex / Pi `auth.json` 副本写进成员目录后不删。

**工作线和清理**：`workspace.rs:345` 的 `line_key`；`workspace.rs:375` 的 `claim_line` 一条工作线同时只给一个运行；`LineGuard`（`workspace.rs:423`）；`cleanup_lines`（`workspace.rs:881`）空闲 72 小时先提交再删整个 `<line>/`；`cleanup`（`workspace.rs:1011-1046`）磁盘低于 `min_free_gb` 时删已结束的运行目录。`RunMeta`（`workspace.rs:16`）没有成员 id。

**基础镜像** `scripts/runner/base-image/Dockerfile`：`FROM ubuntu:24.04`，有 Node、pnpm、bun、uv、pipx、gh、build-essential，没有 rustup、sccache。guest `PATH` 固定为 `GUEST_PATH`（`firecracker.rs:35`），`guest_env`（`firecracker.rs:166-183`）。guest 的 `HOME` 来自主机环境（`run.rs:326-329` 的 `BASE_ENV`，例如 `/home/runner`），vm-agent 在 overlay 上建它（`vm_agent.rs:900-904`），CLI 以主机 runner 的 uid 跑（`firecracker.rs:736`）。

**CLI 记忆**：daemon 没关。`build_env`（`run.rs:343-373`）先放 `rt.env`，`run.rs:566-573` 再并入 app 的 API key；Codex 只有旁问才加 `-c`（`runtime/codex.rs:820-855`）。运行时种类有 claude、codex、acp、pty（`runtime/mod.rs:154-160`），Claude Code 也可能经 acp 或 pty 跑。

**文案**：`apps/desktop/src/copy/admin.rs:301` 的 `FLEET_NOTE_DESC`「Linux 电脑上每个任务开一台独立的 VM，任务结束就销毁」。

## 3 设计

### 3.1 主机上的布局

```
<work_root>/lines/<line key>/work/         工作目录（不变）
<work_root>/lines/<line key>/cli/          这个话题的 .claude/ .codex/ .pi/agent/
<work_root>/runs/<run>/cli/                没有工作线的运行（单独问、工作线被占时的运行）
<work_root>/caches/<repo key>/             这个仓库的缓存，所有成员、所有话题共用
    cache.json   { key, repo, createdAt, lastUsedAt }   repo = 去掉 userinfo 的 URL 或本地路径
    .lock        flock：运行共享，清理独占
    cargo/ rustup/ sccache/ npm/ npm-global/ pnpm-store/ pnpm/ bun/ xdg-cache/
    pipx/ uv-tools/ uv-python/ go/ go-build/ bin/
```

缓存放在 `work_root` 下，不加配置字段：`VmConfig` 是 `deny_unknown_fields`，加字段会让回滚后的旧 daemon 读不了配置；放同一块盘也让 `run_disk` 只看一块盘。`repo key`：有 `repo.url` 时是 `sha256(url)[..16]`（同 `workspace.rs:199` 的镜像 key），只有 `repo.path` 时是 `sha256("path:"+canonical)[..16]`，没有仓库是 `scratch`。新模块 `apps/daemon/src/vm_cache.rs`。

### 3.2 话题：CLI 会话跟着工作线，续接不上要说出来

- `vm_login::cli_env(cli_root)`：用工作线时 `cli_root=<line>/cli`，否则 `<run dir>/cli`。`LineGuard` 加 `pub fn dir(&self) -> &Path`，`run.rs` 在 `prepare_with_github` 返回 line 之后算 `cli_root`。同一工作线同时只有一个运行（`claim_line`），所以一个 `cli/` 不会被两个运行同时写；几个话题同时跑各用各的。
- `RunMeta` 加 `bot_member_id: Option<String>`（serde default），运行开始时写。
- 续接时按顺序找会话文件，找到第一个就拷进 `cli_root`：① `cli_root`；② 同成员其他工作线的 `lines/*/cli`（`line.json` 的 `bot_member_id` 相同）；③ 同成员已结束运行的 `runs/*/cli`（`run.json` 的 `bot_member_id` 相同，覆盖单独问和工作线被占的运行）；④ 旧目录 `<[vm].homes>/<member>` 和 `<[vm].dir>/cli/<member>`（迁移用，§4）；⑤ 主机自己的 CLI 目录（已有，`vm_login.rs:207,252`）。`carry_claude_session` / `carry_codex_session` 改成接收候选目录列表；Pi 同样处理。
- **续接不上补完整提示（所有电脑）**：协议加可选字段 `run.assign.fullPrompt?: string`。服务器在 `runs.ts:870` 把提示换成 `prompt_resume` 时，把原来的完整提示放进 `fullPrompt`。daemon 的三种运行时在会话找不到时（Claude `prepare_resume` 返回 false、Codex `thread/resume` 出错退回 `thread/start`、ACP `lost`）改用 `fullPrompt`，并在 `run.done` 加可选 `resumeMissed: true`；服务器收到后把 `sent:<run>` 改成 `full`（`runs.ts:764` 的 run context 就显示真正发出的提示）。没有 `fullPrompt`（旧服务器）时行为不变。帧大小：提示加倍，仍在 1 MiB 帧上限（`link.ts:48`）之内；整帧会超过上限时服务器不放 `fullPrompt`，行为同今天。
- 登录副本只在一次运行期间存在：Codex / Pi 的 `auth.json` 写进 `cli_root`（mode 600），CLI 退出后、`LineGuard` 释放前删；daemon 启动时 `vm_login::sweep_auth` 删掉没有活动运行的 `lines/*/cli` 和 `runs/*/cli` 里剩下的 `auth.json`。Claude 仍只走 `CLAUDE_CODE_OAUTH_TOKEN` 环境变量并被遮掉。
- 清理不用加代码：`cleanup_lines` 删整个 `<line>/`，`cli/` 一起没；运行目录按原规则删。之后再来的追问靠 `fullPrompt` 拿到完整话题。

### 3.3 仓库：依赖缓存和用户级工具

**挂载**：`RunSpec` 加 `vm_shares: Vec<PathBuf>`（只有 firecracker 读），`run.rs` 放 `cli_root` 和缓存目录；`shares()` 不再加成员目录。`bot_home`、`Job.home` 删掉：guest 的 `HOME` 仍是主机的 HOME 路径（如 `/home/runner`），由 vm-agent 建在 scratch overlay 上，随虚拟机没。

**环境**：新函数 `vm_cache::guest_vars(cache: &Path) -> Vec<(String,String)>`，`run.rs` 在虚拟机运行时并进 env，只加用户运行配置 `env` 里没设的变量：

| 变量 | 值（C = 缓存目录） |
| --- | --- |
| `CARGO_HOME`、`RUSTUP_HOME` | C/cargo、C/rustup |
| `SCCACHE_DIR`、`RUSTC_WRAPPER` | C/sccache、`sccache` |
| `npm_config_cache`、`npm_config_prefix` | C/npm、C/npm-global |
| `npm_config_store_dir`、`PNPM_HOME`、`BUN_INSTALL` | C/pnpm-store、C/pnpm、C/bun |
| `XDG_CACHE_HOME` | C/xdg-cache（pip、uv 等的缓存） |
| `PIPX_HOME`、`PIPX_BIN_DIR`、`UV_TOOL_DIR`、`UV_TOOL_BIN_DIR`、`UV_PYTHON_INSTALL_DIR` | C/pipx、C/bin、C/uv-tools、C/bin、C/uv-python |
| `GOPATH`、`GOCACHE` | C/go、C/go-build |
| `CHAT_VM_PATH_EXTRA` | `C/bin:C/cargo/bin:C/bun/bin:C/npm-global/bin:C/pnpm:C/go/bin` |

`firecracker::guest_env` 只改一处：有 `CHAT_VM_PATH_EXTRA` 时把它插在 `GUEST_PATH` 的 `/opt/runner/bin` 之后（垫片永远第一），并去掉这个变量本身。vm-agent 启动 CLI 前：`RUSTC_WRAPPER=sccache` 而 PATH 上没有 sccache（旧基础镜像）就去掉它。

**基础镜像**：加 rustup 代理（`rustup-init` 核对 SHA-256，`CARGO_HOME=/opt/rust/cargo RUSTUP_HOME=/opt/rust/rustup --default-toolchain none --no-modify-path`，`/opt/rust/cargo/bin` 进 `GUEST_PATH` 末尾）和 sccache 官方预编译包（核对 SHA-256）。镜像里不带工具链。

**工具链由 daemon 先装好**：`host-setup.sh` 在主机装一份 rustup 到 `/usr/local/lib/runner-rustup`（同样核对 SHA-256，`--default-toolchain none`）。虚拟机运行开始、拿到缓存共享锁之后，工作目录有 `rust-toolchain.toml` / `rust-toolchain`（取 `channel`）或根目录有 `Cargo.toml`（取 `stable`）时，daemon 在主机上拿 `C/rustup/.install.lock` 的独占 flock，`RUSTUP_HOME=C/rustup CARGO_HOME=C/cargo rustup toolchain install <channel> --profile minimal -c clippy,rustfmt`，缓存还没有默认工具链时再 `rustup default <channel>`；已装好就几毫秒返回。同仓库同时开的运行在这把锁上排队，只下载一次。官方工具链按 glibc 2.17 构建，在主机（Ubuntu 24.04）装、在 guest（ubuntu:24.04）跑没有问题。限时 15 分钟；失败或主机没有 rustup 时只记日志，并在 `vm_note` 里加一句让 CLI 自己装。运行日志写 `toolchain <channel> ready (<ms>)`，等锁时运行状态消息写「准备工具链」（daemon 日志用英文）。

**系统说明** `vm_note`（`run.rs:76-98`，英文，写给 CLI）：工作目录和编译结果在这个话题的下一次运行里还在；这个仓库的依赖缓存和用户级工具在 C，已在 PATH 上，和这个仓库的其他运行共用；`$HOME`、`/tmp` 和系统目录里的其他东西运行结束就没了；缺 apt 包或 docker 时在回答里写明缺什么，并说明要管理员加进基础镜像；不要在虚拟机里登录任何东西。删掉「20 GB」那句。

**占用和清理**（`vm_cache.rs`）：
- 锁放在盘上：`C/.lock` 的 flock。运行从开始到结束拿共享锁（daemon 进程持有文件句柄）；清理和 `daemon caches clear` 用非阻塞独占锁，拿不到就跳过。运行开始时等独占锁最多 10 分钟。另一个进程（`daemon caches`）也能看到同一把锁。
- 开始和结束时更新 `cache.json.lastUsedAt`。
- `workspace::cleanup` 末尾：删完运行目录和 `kept/` 后，`work_root` 剩余空间仍低于 `min_free_gb` 时，从 `lastUsedAt` 最旧的缓存开始删，删一个量一次，够了或剩下的都在用就停。日志 `cache <key> removed (last used <ms>, <bytes>)`。没有大小上限，也不按时间过期。
- 每次运行前在主机清掉缓存里的已知凭证：`cargo/credentials`、`cargo/credentials.toml`、`npm-global/etc/npmrc`、`bun/bunfig.toml`。
- `daemon caches`（`main.rs` 新子命令，给能登上主机的管理员）：列 key、仓库、大小、最近使用；`daemon caches clear <key|all>` 遇到被锁住的缓存就拒绝并说明。没有协议，没有界面。
- `health::run_disk` 去掉家目录盘，只看 `work_root`。

### 3.4 电脑：登录

订阅登录仍按 `vm_login.rs`：不共享主机目录，每次给一份不带 refresh token 的副本，主机统一刷新；副本改放 `cli_root`、运行完就删（§3.2）。GitHub 由 C 部分做：daemon 的 credential helper 和 gh 包装器从主机的登录里取，不进虚拟机。家目录不再保留，`github::clear_home`（`github.rs:585`）和 `run.rs:543-549` 的调用删掉。

### 3.5 去掉 #227 的家目录

- 删 `vm_home.rs`（`lock`、`plain_id` 搬进 `vm_cache.rs`），`daemon.rs` 的 `clearing`、`on_home_clear`、`metrics.homes`、功能 `homes`，`doctor.rs:113` 的旧 helper 检查。收到 `home.clear` 只记日志 `homes are gone in this version`。
- `config.rs`：`homes`、`home_gb` 照样能解析，不再起作用（同 1.2.1 的 `approval_policy`），`homes` 只用来找旧会话。示例配置删这两行。`daemon doctor`：设了 `home_gb` 打 `note: [vm].home_gb is ignored`；`<homes>/../home-images` 里还有文件打 `warning: old home images are still on disk: run scripts/runner/host-setup.sh again as root`。
- 协议只加不改：daemon 不再报 `homes`、不再发可选的 `metrics.homes`；服务器的路由、schema、fixtures 保留，对新 daemon 回 `machine.too_old`（`link.ts:1060` 现有逻辑）。`docs/protocol.md`「Machine homes」加一句「daemon 从 <版本> 起不再保留成员家目录，也不再报告 `homes`」。

### 3.6 CLI 自带的记忆：所有电脑、所有运行都关

- `run.rs`：在 `build_env` 之后、并入 API key（`run.rs:566-573`）之后，最后一步强制 `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`（先 `retain` 掉同名项），对 claude、codex、acp、pty 每种运行都设，运行配置和 app 密钥都盖不掉。这个变量对别的 CLI 没有影响，所以经 ACP 或 pty 跑的 Claude Code 也关了。抽成 `fn seal_env(env: &mut Vec<..>)`，测试直接调。
- `runtime/codex.rs` 的 `app_server_args`：普通运行和旁问都加 `-c features.memories=false`。
- 经 ACP 适配器或 pty 跑的 Codex 没有统一的参数入口，这一版不管（§8）；仓库里的 AGENTS.md / CLAUDE.md、用户级 `~/.claude/CLAUDE.md` 不动。

### 3.7 界面文案

`FLEET_NOTE_DESC` 改成：「按 AI 成员能去的电脑挑一台在线、有空位、最闲的；都满了就排队。设成用虚拟机的电脑上，每个任务开一台独立的虚拟机：话题里的代码和仓库的依赖留在电脑上，其余随任务结束清掉；其他电脑上的东西一直保留。」不加别的界面。

## 4 兼容和迁移

- **不跑虚拟机的电脑**：只多了 §3.6 的设置和 §3.2 的 `fullPrompt` 回退；家目录、`~/.claude`、`~/.codex` 不变，不多任何设置或界面。
- **daemon 先自更新、host-setup 还没重跑**：新 daemon 不调 `home-up`。在主机重启前，旧镜像仍挂在 `<homes>/<id>`，续接时从那里（§3.2 第④步）拷会话；重启后镜像没挂，旧会话暂时找不到，续接走 `fullPrompt`，不会丢话题上下文，只丢 CLI 的逐字历史。重跑 host-setup 之后旧会话重新可用。旧 helper 的 `export` 已经允许 `work_root` 下的 `caches/<key>`、`lines/<key>/cli`，不需要新 helper。
- **重跑 host-setup.sh（root）**：新步骤 `retire_homes`，对每个 `<homes>/<id>`：是镜像就先挂上；只把 `.claude/projects`、`.codex/sessions`、`.pi/agent/sessions` 拷到 `<homes>/<id>.legacy/`（属主 runner），卸载，删镜像和原目录，`.legacy` 改名回 `<id>`。然后删 `home-images/`；`/srv/runner/cache` 是空目录就删掉，`install -d` 那一行去掉。`auth.json`、`hosts.yml`、`.git-credentials`、装过的工具都不搬。幂等：再跑一遍结果相同。
- **旧 daemon 碰上新 helper**：`home-up` 没了，得到 `bad slot`，退回不限大小的普通文件夹（`vm_home.rs:43-45`），照样能跑。
- **剩下的旧会话**：daemon 清理时删 `<homes>/*` 和 `<[vm].dir>/cli/*` 里 mtime 超过 14 天的子目录，**跳过挂载点**（`st_dev` 和父目录不同）和 `home-images` 还在时的整个 `<homes>`；`<homes>` 空了再删它本身。
- **丢掉的东西**：家目录里装过的工具不搬；Rust 工具链由 daemon 第一次运行时装进仓库缓存，其余工具 AI 成员按需重装。CHANGELOG 写明。
- **已合并的 PR**：#227 按 §3.5 删代码、改文档，不 revert；`metrics.homes`、`home.clear` 留在协议里不再用。#139 的「家目录保留」由话题 `cli/` 加仓库缓存取代。

## 5 安全

- 运行永远读不到订阅登录的 refresh token。Codex / Pi 的 access token 副本只在运行期间存在于 `cli_root`（600），运行结束和 daemon 启动时都删，从不写进缓存。
- 每台虚拟机只导出自己用到的：运行目录、工作目录、git dir、自己的 `cli_root`、自己仓库的 `caches/<key>`。新 `runner-vm` 再拒绝导出 `<DATA>/caches` 本身和任何名为 `caches` 的目录本身（只许它下面一层的 key 目录）。
- 接管时人的命令仍只看到工作目录和 git dir（`firecracker.rs:226-250` 不变），看不到 `cli/` 和缓存。
- 缓存共用意味着同一台电脑上同一个仓库的运行互相信任：一个运行能改掉后面运行要执行的工具（`cargo install` 的二进制、工具链、`npm -g`）。这和仓库本身的信任边界相同：能让 AI 成员改 `build.rs` 或 package scripts 的人也能做到。不同仓库不共用。出问题用 `daemon caches clear`。已知凭证文件每次运行前清掉。
- `cache.json` 存的 URL 去掉 userinfo；日志只写 key。`fullPrompt` 和 `prompt` 同等对待：daemon 不写日志、不落盘，只交给 CLI。
- 磁盘：缓存按 LRU 清；剩余低于 `min_free_gb` 一半时拒收新运行（现有 `disk_low`）。主机 rustup 只在 runner 用户下以 daemon 身份跑，下载走 rustup 自己的校验。

## 6 拆成哪些 PR

| PR | 做什么 | 改哪些文件 | 依赖 | 怎么测 |
| --- | --- | --- | --- | --- |
| B1 关记忆 | §3.6 | `apps/daemon/src/run.rs`（只加 `seal_env` 和调用）、`runtime/codex.rs`、`docs/machine-protocol.md`（Environment 一节）、`CHANGELOG.md` | 无 | cargo test：claude、pty 运行都有变量；`rt.env` 和 app key 设成 `0` 被盖成 `1`；`app_server_args` 普通和旁问都带参数；`tests/runtimes.rs` 假 Codex 收到参数；Windows CI 同一组测试 |
| B2 协议和服务器 | `run.assign.fullPrompt`、`run.done.resumeMissed`；服务器发送和改 `sent:` | `docs/machine-protocol.md`（run.assign / run.done）、`packages/protocol/src/machine.ts`、`crates/chat-protocol/src/machine.rs`、`protocol/fixtures/machine/`（新 `down-run.assign-full-prompt.json`、`up-run.done-resume-missed.json`）、`server/src/bots/desk/runs.ts`、`server/test/` | 无 | 两个 fixture 测试；vitest：有 `resumeSessionId` 时帧里有 `fullPrompt`；超 1 MiB 时不带；`resumeMissed` 后 run context 返回完整提示 |
| B3 daemon 用完整提示 | 三种运行时续接不上时用 `fullPrompt` 并报 `resumeMissed` | `runtime/mod.rs`（`RunSpec.full_prompt`、`Outcome.resume_missed`）、`runtime/claude.rs`、`runtime/codex.rs`（只改 resume 失败分支）、`runtime/acp.rs`、`daemon.rs`（填 run.done，只改那一行） | B2；和 B1 有 `codex.rs` 小冲突，B1 先合 | cargo test：假 Claude 会话文件不在时收到完整提示、`--session-id`；假 Codex `thread/resume` 出错后 `turn/start` 用完整提示；ACP lost 同理；run.done 带 `resumeMissed` |
| B4 主机脚本和镜像 | Dockerfile 加 rustup 代理和 sccache；`host-setup.sh` 装主机 rustup、`retire_homes`、去掉 `cache`/`home-images`；`runner-vm` 去掉 `home-up`/`home-clear`、拒绝导出 `caches` 本身 | `scripts/runner/base-image/Dockerfile`、`scripts/runner/host-setup.sh`、`scripts/runner/runner-vm` | 无 | 测试主机：一个镜像家目录、一个普通家目录跑 host-setup，只剩会话目录，`home-images` 没了；再跑一遍结果相同；`runner-vm export 0 /srv/runner/daemon/caches rw` 被拒、`.../caches/<key>` 可以；`boot-test.sh` 打出 `rustup --version`、`sccache --version` |
| B5 话题会话、去掉家目录 | §3.2（除协议部分）、§3.4、§3.5 | 删 `vm_home.rs`；新建 `vm_cache.rs`（先放 `lock`、`plain_id`）；`vm_login.rs`、`executor/firecracker.rs`（`shares`、删 `bot_home`）、`executor/mod.rs`（`vm_shares`）、`run.rs`、`workspace.rs`（`LineGuard::dir`、`RunMeta.bot_member_id`、旧目录 14 天清理）、`daemon.rs`、`health.rs`、`doctor.rs`、`config.rs`、`github.rs`（删 `clear_home`）、`metrics.rs`、`tests/integration.rs` | B1、B3 | cargo test：两个话题同时跑各有 `cli/`；续接从 `<line>/cli`、别的工作线、已结束运行目录、`<homes>/<id>`、`<dir>/cli/<id>` 都能找到；运行后没有 `auth.json`；`sweep_auth`；`shares()` 有 `cli_root` 无成员目录；无 `homes` 功能；带 `home_gb` 的旧配置能解析；14 天清理跳过挂载点（用临时目录模拟 `st_dev` 判断函数） |
| B6 仓库缓存 | §3.3 | `vm_cache.rs`、`executor/firecracker.rs`（`guest_env` 只加 `CHAT_VM_PATH_EXTRA` 一处、`GUEST_PATH` 末尾加 `/opt/rust/cargo/bin`）、`vm_agent.rs`（sccache 检查）、`run.rs`（`vm_note`、锁、工具链、`guest_vars`）、`workspace.rs`（`cleanup` 末尾）、`main.rs`（`caches` 子命令）、`doctor.rs` | B5 | cargo test：key 与镜像 key 一致；变量和 PATH 顺序（垫片第一）；用户 env 优先；flock：另一进程持有共享锁时 `caches clear` 拒绝；LRU 先删最旧、跳过被锁的（磁盘函数用替身）；凭证文件被删；工具链读取 `rust-toolchain.toml` 的 channel；`local-e2e.ts` 在虚拟机上跑两个话题 |
| B7 文档和文案 | §3.7、自托管、计划 | `apps/desktop/src/copy/admin.rs`、`docs/machine-protocol.md`（MicroVM 一节）、`docs/protocol.md`（Machine homes）、`docs/SELF-HOSTING.md`、`docs/SELF-HOSTING.zh.md`、`docs/plans/usable.zh.md`、`docs/plans/usable/machine.zh.md`、`CHANGELOG.md` | B4、B6 | 文案目录测试；fixture 测试不变 |

并行：B1、B2、B4 同时开工；B3 等 B2（和 B1 合并顺序：B1 先）；B5 等 B1、B3；B6 等 B5；B7 最后。和 C 部分的重叠：`github.rs`（B5 删 `clear_home`）、`run.rs`、`executor/firecracker.rs`（C 改 `guest_env` 里的 GitHub 变量，B6 只加 `CHAT_VM_PATH_EXTRA` 一处）。合并顺序：谁后合并谁合 main 解冲突；B6 不改 `guest_env` 的签名，C 的测试不用动。

## 7 真电脑上的验收

在团队 VPS（`[vm]`）、一台个人 Mac、一台 Windows 电脑上做：

1. 升级：VPS 的 daemon 自更新，root 重跑 `host-setup.sh`，管理员重建基础镜像。`/srv/runner/home-images` 和 `/srv/runner/cache` 不在了；`/srv/runner/homes/<id>` 下只有三类会话目录；`daemon doctor` 没有警告。
2. 升级前的一个话题里接着问：续接成功，`run.json` 的会话 id 不变，CLI 记得之前的话。
3. 在一个 Rust 仓库开话题 1，让开发 AI 成员 `cargo build`：运行日志有 `toolchain stable ready`，`<work_root>/caches/<key>/{cargo/registry,rustup/toolchains,sccache}` 有内容。同时开话题 2、3 同一仓库：只下载一次工具链（另外两个在等锁），都成功。之后话题 2 `cargo build -v` 没有 `Downloading`，`sccache -s` 有命中。
4. 同一成员话题 1 和 2 同时跑：两个都完成，`lines/<k1>/cli` 和 `lines/<k2>/cli` 分开；在话题 1 追问接的是话题 1 的会话。
5. 两个人在同一话题同时发消息（第二个运行拿到自己的运行目录），之后再追问：续接的会话从运行目录找到，CLI 记得两次对话。
6. 在任务里 `echo x > $HOME/foo && pip install --user httpie && npm i -g cowsay`。下一次运行：`$HOME/foo`（即 `/home/runner/foo`）和 httpie 没了，`cowsay` 还在。运行结束后 `find /srv/runner -name auth.json` 一个都没有。
7. 72 小时：测试主机 `keep_runs_hours=1`，工作线里留一处没提交的改动，等清理：工作线连 `cli/` 一起没了，分支上多了 `wip: uncommitted changes kept by cleanup`。再在这个话题追问：运行完成，`run.done` 带 `resumeMissed`，CLI 拿到完整话题，回答对得上之前的内容；网页上这次运行的上下文显示完整提示。
8. 磁盘：测试主机 `min_free_gb` 临时调到「当前剩余 + 5」，重启 daemon：日志先删最久没用的缓存，正在跑的仓库的缓存还在；`daemon caches clear all` 对正在用的缓存报拒绝。改回原值。
9. 记忆：在 Mac、Windows、VPS 上分别对 Claude Code 和 Codex 成员说「记住我喜欢用 pnpm」：CLI 配置目录里没有新的 `projects/*/memory/`，Codex 的 `memories/` 没有新文件；产品里出现一条记忆；进程环境（Mac/VPS `ps eww`，Windows 用 Process Explorer）有 `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1`，codex 命令行有 `features.memories=false`。
10. 个人 Mac：其余照旧，运行用 `~/.claude`，没有 `caches`，没有新设置或界面。

## 8 这一版不做的

- 缓存的大小上限、配额、按时间过期；机器页上的缓存用量和清空按钮（及协议）。
- 接管时人的命令看到仓库缓存。
- 保留 AI 成员装的 apt 包、docker 镜像（归基础镜像）。缺东西时 AI 成员只在回答里写明，由提问的人转告管理员；不自动通知管理员。
- 运行中 AI 成员自己再装别的工具链或 target 时的并发加锁（daemon 预装的那个已加锁）；出问题用 `daemon caches clear`。
- 几台电脑之间共用缓存、虚拟机快照和预热。
- 运行中把磁盘写满时停掉这个运行。
- 经 ACP 适配器或 pty 跑的 Codex 关记忆（它们没有统一的参数入口；Claude Code 走任何运行时都已关）。
- 把旧家目录里装过的工具迁到仓库缓存。

## 对评审的处理

全部采纳，除了：「把运行目录的会话拷回工作线」改为续接时直接搜已结束运行目录（§3.2 第③步），效果相同，少一次在别的运行占着工作线时写它的 `cli/`；「给 helper 一个固定的允许形状列表」只采纳拒绝 `caches` 本身，`lines` 规则去掉，因为 helper 不知道 `work_root`（`config.rs:217` 的默认值随用户变）。
