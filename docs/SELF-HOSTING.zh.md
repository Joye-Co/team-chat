# 自己部署服务器

English: [SELF-HOSTING.md](SELF-HOSTING.md)。

每个团队都用自己部署的服务器，一个人自己用也一样：没有托管服务。服务器是你自己 Cloudflare 账号上的一个
Worker，加上 Durable Objects、一个 D1 数据库和一个 R2 存储桶。用这个仓库里的脚本，在一台 Linux 机器上
部署一次；之后 Mac 或 Windows 上的应用登录到它。

第一部分带你从一台全新的 Ubuntu 机器和一个 Cloudflare 账号开始，做到应用登录上、这台电脑能给 AI 成员干活。
第二部分给要在一台 Linux 主机上用虚拟机跑 AI 成员的团队；只用自己的电脑就够的话可以跳过。

运行这些脚本只保证 Ubuntu（24.04 LTS）。脚本在 macOS 上也能跑，但这份指南不讲。

| 部署完你有 | 在哪 |
| --- | --- |
| 服务器：Worker `chat-api-prod`、D1 数据库 `chat-prod`、R2 存储桶 `chat-files-prod` | 你的 Cloudflare 账号 |
| 服务器地址 `https://chat-api-prod.<你的子域名>.workers.dev` | `~/.config/team-chat/ops/deploy/servers.json` |
| 这台服务器的部署配置 | `~/.config/team-chat/ops/deploy/server.wrangler.jsonc` |
| 服务器的密钥和你的 owner 令牌 | `~/.config/team-chat/production/`（文件夹权限 700，文件 600） |

`<你的子域名>` 和下面所有 `<...>` 都是占位，换成你自己的值。

## 第一部分　你的服务器

### 需要准备

- 一台装了 Ubuntu 24.04 LTS、能用 `sudo` 的机器：闲置的电脑、虚拟机或者一台小云服务器都行。部署完它上面
  不常驻任何程序，只是运行脚本、保存密钥文件的地方。留着它，或者把它的两个文件夹（上表）复制到安全的地方：
  以后更新服务器要从这里运行。
- 一个 Cloudflare 账号（免费套餐就能开始）。
- 一台装应用的电脑：Mac（Apple 芯片，macOS 13 或更新）或 Windows 10/11。
- 想让 AI 成员在这台电脑上干活：在上面装好 Claude Code 或 Codex，并登录。

### 1. 在 Ubuntu 上装工具

```sh
sudo apt-get update
sudo apt-get install -y git curl jq openssl unzip ca-certificates
# Node.js 22 或更新（wrangler 需要；Ubuntu 自带的版本太旧）
curl -fsSL https://deb.nodesource.com/setup_22.x -o /tmp/nodesource_setup.sh
sudo -E bash /tmp/nodesource_setup.sh
sudo apt-get install -y nodejs
# bun 1.3
curl -fsSL https://bun.sh/install | bash -s bun-v1.3.10
exec "$SHELL" -l      # 开一个新 shell，让 bun 进 PATH
```

检查：`node --version` 是 v22 或更新，`bun --version` 是 1.3.10，`jq --version` 能运行。

### 2. 拿代码

部署和你要装的应用同一个版本：最新的发布版。

```sh
git clone https://github.com/Joye-Co/team-chat.git
cd team-chat
git checkout "$(git describe --tags --abbrev=0 origin/main)"   # 最新的发布标签 v<版本号>
[ -x scripts/setup-server.sh ] || git checkout main             # v1.2.7 及更早的发布没有它
bun install --frozen-lockfile
```

`scripts/setup-server.sh` 从 v1.2.7 之后的下一个发布版开始才有。那个版本发布前，第二行让你留在 `main` 上；
应用照样装最新的发布版。

检查：`git describe --tags` 输出 `v<版本号>`（在 `main` 上是 `v<版本号>-<n>-g<提交>`）；`ls server/node_modules/.bin/wrangler` 能找到 wrangler。
部署后服务器在 `/v1/health` 报告的就是这个版本。

### 3. 准备 Cloudflare 账号

1. 没有账号就在 <https://dash.cloudflare.com/sign-up> 注册一个。
2. 在控制台里打开一次 **Workers & Pages**。新账号会让你选 `workers.dev` 子域名，它就是服务器地址里的
   `<你的子域名>`。
3. 打开 **R2 Object Storage**，开通它。即使只用免费额度，Cloudflare 也会要你绑定一个付款方式。
4. 让 wrangler 登录。机器上没有浏览器时用设备登录：它会打印一个链接和一个码，在任何一台电脑上打开就行。

   ```sh
   cd server
   bunx wrangler login --device
   bunx wrangler whoami        # 检查：显示你的账号
   cd ..
   ```

   也可以不登录，改用 API 令牌（控制台 **My Profile › API Tokens › Create Token**），给它这几项账号权限：
   Workers Scripts Edit、D1 Edit、Workers R2 Storage Edit、Account Settings Read。交给 wrangler 时别让它
   进 shell 历史：

   ```sh
   read -rs CLOUDFLARE_API_TOKEN && export CLOUDFLARE_API_TOKEN
   ```

如果你的登录下有不止一个账号，再 `export CLOUDFLARE_ACCOUNT_ID=<账号 id>`（`bunx wrangler whoami` 会列出来）。

### 4. 设置并部署服务器

```sh
scripts/setup-server.sh production --owner-name "<你的名字>"
```

脚本每一步都先检查再动手，所以随时可以再跑一遍：缺什么补什么，别的不动。它做的事：

| 步骤 | 建出来什么 | 存在哪 |
| --- | --- | --- |
| 1. 前提 | 什么都不建；缺哪个工具、没登录，逐条告诉你装什么、运行什么 | |
| 2. ops 文件夹 | 放你这套部署信息的文件夹 | `~/.config/team-chat/ops/`（或 `$TC_OPS_DIR`） |
| 3. D1 和 R2 | D1 数据库 `chat-prod`（位置提示 `oc`）、R2 存储桶 `chat-files-prod` | Cloudflare |
| 4. Worker 配置 | `deploy/server.wrangler.jsonc`，带一个 `env.production`（Worker 名、D1 id、存储桶） | ops 文件夹 |
| 5. 密钥文件 | `admin-secret`（保护管理接口） | `~/.config/team-chat/production/` |
| 6. 部署 | D1 迁移；Worker，连同它的四个 Durable Object 和每天的归档定时任务；把服务器地址写进 `deploy/servers.json`；等到 `/v1/health` 报告这个版本 | Cloudflare、ops 文件夹 |
| 7. Worker 密钥 | `ADMIN_SECRET` 和 `HOOK_SECRET_KEY`（加密服务器上保存的 AI 服务 key 和集成密钥；它的文件 `hook-secret-key` 在这一步生成） | Cloudflare、`~/.config/team-chat/production/` |
| 8. 第一个 owner | 你，团队的 owner，和默认群；你的 owner 令牌；一个给应用用的登录码 | `~/.config/team-chat/production/` 里的 `owner-token`、`signin-<id>-<时间>.txt` |
| 9. 下一步 | 打印服务器地址、登录码在哪个文件、应用里接着做什么 | |

脚本从不打印密钥、令牌和登录码，只打印它们所在文件的路径。想用别的名字，第一次运行时加 `--worker`、`--d1`、
`--bucket`。想用自己的域名代替 `workers.dev`，在配置里加路由，再加 `--url <地址>`。

检查：

```sh
curl https://chat-api-prod.<你的子域名>.workers.dev/v1/health
# {"ok":true,"version":"<版本号>"}
bun server/scripts/admin.ts members --env production
# 一行：<id>  human  owner  active  <你的名字>
```

哪一步失败了，报错会说怎么办；也可以看后面的[出了问题](#出了问题)。

### 5. 装应用并登录

1. 从 <https://github.com/Joye-Co/team-chat/releases> 下载同一版本的应用：Mac 用 `.dmg`，Windows 用
   `-setup.exe`。按 [INSTALL.zh.md](INSTALL.zh.md) 安装并第一次打开（第 2、3 步，或[在 Windows 上安装](INSTALL.zh.md#在-windows-上安装)）。
2. 在登录界面选 **使用其他服务器…**，填你的服务器地址 `https://chat-api-prod.<你的子域名>.workers.dev`。
3. 在 Ubuntu 上看最新的登录码：`cat "$(ls -t ~/.config/team-chat/production/signin-*.txt | head -n 1)"`
   （准确路径是第 4 步最后打印的；更早的文件里是已经用过的码）。
   把它填进 **邀请码** 的格子，点 **加入**。

登录码只能用一次，7 天内有效。要新的就运行 `scripts/setup-server.sh production --sign-in-code`。

检查：应用以你的身份打开，有团队的默认群；设置里有 **电脑** 这一页。

### 6. 让 AI 成员在这台电脑上干活

1. 在这台电脑上装 Claude Code 或 Codex 并登录，让终端里 `claude --version` 或 `codex --version` 能运行。
2. 在应用里打开 **设置 › 电脑**，点 **用这台电脑**。应用会把这台电脑登记到你的服务器，作为你自己的电脑，写好后台服务的
   配置和令牌并启动它（Mac 上是 launchd，Windows 上是计划任务），以后跟着应用一起更新。之后 **团队管理 › 电脑** 里这台电脑的
   卡片检查三项：后台服务在运行、找到 CLI、已登录，缺哪项就说怎么补。详细见
   [INSTALL.zh.md 的「让 AI 成员在你的电脑上干活」](INSTALL.zh.md#让-ai-成员在你的电脑上干活)。

检查：设置 › 电脑 的列表里有这台电脑，显示在线，三项都好了。在 Ubuntu 上运行
`bun server/scripts/admin.ts machines --env production`，它显示为 `healthy`。

### 7. 第一个 AI 成员

在 **团队管理 › 成员** 里点 **添加 AI 成员**，工作方式选 **在电脑上工作**，选好运行方式，让它在这台电脑（或任意一台空闲电脑）
上干活，然后在群里的一个话题里 @ 它，
交代一件事。应用的用法见 [ONBOARDING.zh.md](ONBOARDING.zh.md)。

要拉人进来，给每个人生成一个邀请码：在应用里（团队管理 › 成员），或者在 Ubuntu 上运行
`bun server/scripts/admin.ts invite --env production --name <名字>`（邀请码写进文件，只打印路径）。
他们从发布页下载应用，用你的服务器地址和自己的邀请码登录。

### 更新服务器

服务器和应用保持同一版本：有新版本发布时更新它。

```sh
cd team-chat
git fetch --tags origin
git checkout "$(git describe --tags --abbrev=0 origin/main)"
[ -x scripts/setup-server.sh ] || { git checkout main && git pull --ff-only; }
bun install --frozen-lockfile
scripts/setup-server.sh production
```

它部署新版本（先跑 D1 迁移），别的都不动。只想部署的话用 `scripts/deploy-server.sh production`。

应用从你的服务器拿更新，邀请链接也从它下载应用。只有你把应用文件交给服务器后它才有；在那之前，大家从发布页
下载新版本来更新。要把某个发布版的文件交给你的服务器：装好 GitHub CLI（`sudo apt-get install -y gh`，然后
`gh auth login`），运行 `scripts/release.sh <版本号> --env production --reupload`。它下载这个发布版的 DMG
和 Windows 安装包，对照发布页上的 SHA-256 校验，再用你的 admin 密钥上传。

### 这些要保管好

| 文件 | 为什么 |
| --- | --- |
| `~/.config/team-chat/production/hook-secret-key` | 丢了它，服务器上保存的 AI 服务 key 和集成密钥就读不出来：owner 要重新填 key，每个集成要重建。脚本从不替换 Worker 上已有的这个密钥。 |
| `~/.config/team-chat/production/admin-secret` | 保护管理接口。丢了就再跑一遍设置脚本：它会生成新的并放到 Worker 上。 |
| `~/.config/team-chat/production/owner-token` | 让脚本以你的身份操作（`admin.ts`、`--sign-in-code`）。owner 建好后脚本没法再生成新的，丢了的话这些事只能在应用里做。 |
| `~/.config/team-chat/ops/` | 你的部署配置和服务器地址。 |

想在另一台机器上做更新，把这两个文件夹都复制过去（`scp -r`，保持权限）。

备份（每天归档到 R2）、恢复、回滚、换密钥见 [RUNBOOK.md](RUNBOOK.md)。ops 文件夹、密钥文件、在正式服务器旁边
再部署一个测试服务器（`scripts/setup-server.sh staging`）见 [ENVIRONMENTS.md](ENVIRONMENTS.md)。

### 出了问题

| 看到什么 | 怎么办 |
| --- | --- |
| 第 1 步出现 `missing: ...` | 装上它说的东西，再运行一遍。 |
| `wrangler is not signed in` | 看第 3 步第 4 条。用 API 令牌的话，确认它在当前 shell 里 export 了。 |
| `your Cloudflare login has N accounts` | `export CLOUDFLARE_ACCOUNT_ID=<id>`，再运行。 |
| `could not create R2 bucket` | 在控制台开通 R2（第 3 步第 3 条）。 |
| `deployed, but wrangler printed no workers.dev address` | 账号还没有 `workers.dev` 子域名（第 3 步第 2 条），或者你用的是自己的域名：加 `--url <地址>` 再运行。 |
| 第一次部署后马上出现 `could not make the first owner` | 新放上去的密钥要过一会儿才生效：等一分钟再运行。 |
| `this server has an owner already` | owner 是在另一台机器上建的：把那台的 `~/.config/team-chat/production/` 复制过来。 |
| 应用里保存 AI 服务时报 `the Worker secret HOOK_SECRET_KEY is not set on this server`（旧版本报 `hooks are not configured on this server`） | Worker 上没有 `HOOK_SECRET_KEY`：运行 `scripts/setup-server.sh production`。 |
| 保存或测试 AI 服务时报 `the saved API key cannot be read` | `HOOK_SECRET_KEY` 换过，存着的 key 解不开了：在 AI 服务里重新填一次 key。 |
| 应用提示「连不上服务器」 | 检查地址（要带 `https://`），再 `curl <地址>/v1/health`。 |
| `/v1/health reports <x>, expected <y>` | 部署还没生效，或者失败了：再运行 `scripts/deploy-server.sh production`，看它的输出。 |

服务器日志：`cd server && bunx wrangler -c wrangler.deploy.jsonc tail --env production`（部署过以后才有
`wrangler.deploy.jsonc`）。

## 第二部分　团队跑 AI 成员的 Linux 主机

团队可以把 AI 成员放在一台 Linux 主机上跑，不占大家的电脑。每次任务的 CLI 跑在它自己的 Firecracker 虚拟机里
（[machine-protocol.md](machine-protocol.md#microvm-runs-vm-linux)）；用邀请码进来的人不用配电脑，直接用这些
AI 成员。

### 需要什么

- x86_64 的 Ubuntu 24.04 LTS，能用 root 通过 SSH 登录。
- KVM：物理服务器，或者开了嵌套虚拟化的虚拟机。检查：`ls -l /dev/kvm` 存在，
  `grep -Ecw 'vmx|svm' /proc/cpuinfo` 大于 0。
- 内存：每台运行中的虚拟机最多用 4 GiB（`[vm] mem_mib`），乘上允许同时跑的任务数（`max_parallel_runs`）。
  可用内存不够再开一台（另外留出总内存的五分之一）时，daemon 不接新任务。
- 磁盘：基础镜像（几 GB）、每台运行中的虚拟机 32 GB 稀疏的临时空间、每个 AI 成员的家目录：
  `/srv/runner/home-images` 里一个 20 GB 的磁盘镜像（`[vm] home_gb`），只占实际用到的部分。
  先留 200 GB。
- 第一部分的服务器，以及那台存着 `~/.config/team-chat/production/` 的 Ubuntu 机器。

### 安装

1. **主机设置**，在主机上用 root：

   ```sh
   git clone https://github.com/Joye-Co/team-chat.git /root/team-chat
   cd /root/team-chat && git checkout v<版本号>      # 和服务器同一版本，或 main
   bash scripts/runner/host-setup.sh
   ```

   它装好系统包、Docker、Firecracker 和 jailer（对照 SHA-256 校验）、`runner` 用户（在 kvm 和 docker 组里，
   开了 linger）、放镜像和虚拟机的 `/srv/runner`、IP 转发、NFS 服务、虚拟机的防火墙（`runner-vm-net.service`），
   以及 daemon 能用 sudo 运行的 root 助手 `/usr/local/sbin/runner-vm`。它把 root 的 SSH 公钥复制给 `runner`
   用户。可以重复运行。
2. **虚拟机内核。** daemon 启动的是 `/srv/runner/kernels/vmlinux`：一个未压缩的内核，要有 virtio 块设备、
   virtio-vsock、overlayfs 和 NFS 客户端。仓库里没有脚本生成它。可以从 Firecracker 的 CI 内核开始（见
   Firecracker 入门指南的 "Running Firecracker" 一节）：放到这个位置，命名为 `vmlinux`，属主 `runner`。
   第 5 步能看出它能不能启动。
3. **基础镜像**，用 root：`bash scripts/runner/build-base-image.sh`。它把
   `scripts/runner/base-image/Dockerfile`（带 git、Node、Claude Code、Codex、gh 等的 Ubuntu）做成
   `/srv/runner/images/base-<日期>.ext4`，并让 `base.ext4` 指向它。
4. **daemon**，用 runner（`ssh runner@<主机>`；第二部分剩下的步骤除非写了 root，都用 runner）。现在要在主机上
   从源码编译：

   ```sh
   curl --proto '=https' -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal
   . "$HOME/.cargo/env"
   git clone https://github.com/Joye-Co/team-chat.git ~/team-chat
   cd ~/team-chat && git checkout v<版本号>      # 和服务器同一版本，或 main
   out="$(scripts/build-daemon.sh)"   # rustup 会装 rust-toolchain.toml 指定的工具链
   mkdir -p ~/.local/bin
   install -m 755 "$out" ~/.local/bin/team-chat-daemon.new
   mv -f ~/.local/bin/team-chat-daemon.new ~/.local/bin/team-chat-daemon
   ```

   <!-- RELEASED DAEMON: when releases carry a Linux daemon, replace the commands above with the
        download of that asset and its SHA-256 check, and describe its self-update here. -->
   **发布版 daemon：** 等发布物里有了 Linux 的 daemon，这一步改成从发布页下载；在那之前用上面的命令。
5. **启动检查**：`bash ~/team-chat/scripts/runner/boot-test.sh`。应该看到 `vm-init: ready`、每个工具一行版本号，
   以及几秒后的 `firecracker exited after`。内核缺东西的话，会在这里出现 kernel panic。
6. **把主机登记成团队的电脑。** 在第一部分那台 Ubuntu 机器上：

   ```sh
   bun server/scripts/admin.ts machine --env production --name <主机名> --labels linux,vm
   ```

   它把机器令牌写到 `~/.config/team-chat/production/machine-<名字>-<id>.token`，只打印路径。不显示内容地复制到主机：

   ```sh
   ssh runner@<主机> 'mkdir -p -m 700 ~/.config/team-chat/production'
   scp ~/.config/team-chat/production/machine-<名字>-<id>.token runner@<主机>:.config/team-chat/production/machine-token
   ssh runner@<主机> 'chmod 600 ~/.config/team-chat/production/machine-token'
   ```

7. **CLI 登录。** 用 root 装 Node.js 22（第一部分第 1 步里 NodeSource 那几行）和 CLI：
   `npm install -g @anthropic-ai/claude-code @openai/codex`。再用 runner 登录：运行 `claude` 按提示登录，
   以及 `codex login --device-auth`（也可以在 app 的 团队管理 › 订阅账号 里登录）。这些登录不共享给虚拟机：
   每个任务拿到一份改不了宿主机登录的副本（Codex 去掉 refresh token，由宿主机统一刷新），每个 AI 成员的
   会话记录放在它自己的家目录里。Claude Code 最好用长期令牌：用 runner 运行 `claude setup-token`，把令牌
   作为名为 `CLAUDE_CODE_OAUTH_TOKEN` 的 API 密钥加给这台主机（团队管理 › 订阅账号，给所有成员）；
   不加的话，每个任务拿宿主机当前的 access token。
8. **配置**，用 runner：`team-chat-daemon example-config > ~/.config/team-chat/daemon.toml`，然后改成：

   ```toml
   server_url = "https://chat-api-prod.<你的子域名>.workers.dev"
   token_file = "~/.config/team-chat/production/machine-token"
   name = "<主机名>"
   labels = ["linux", "vm"]
   work_root = "/srv/runner/daemon"
   max_parallel_runs = 4

   [runtimes.claude]
   kind = "claude"

   [runtimes.codex]
   kind = "codex"

   [vm]
   kernel = "/srv/runner/kernels/vmlinux"
   rootfs = "/srv/runner/images/base.ext4"
   ```

9. **启动**，用 runner：

   ```sh
   ~/.local/bin/team-chat-daemon doctor            # 退出码 0：没有问题
   ~/.local/bin/team-chat-daemon install-service   # systemd 用户服务 team-chat-daemon.service
   ```

### 检查

- `systemctl --user status team-chat-daemon` 是 `active (running)`；日志在
  `~/.local/state/team-chat-daemon/daemon.log`。
- 在第一部分那台 Ubuntu 机器上，`bun server/scripts/admin.ts machines --env production` 显示这台主机
  `healthy`；应用的 设置 › 机器 里它在线。
- 让一个 AI 成员在这台主机上干活（在应用里能选 AI 成员电脑的地方选它，或者在 Ubuntu 机器上运行
  `bun server/scripts/admin.ts runner --env production --bot <成员 id> --runtime claude --labels vm`），
  在话题里 @ 它。它跑完第一个任务后，用 runner 检查虚拟机的网络：
  `python3 ~/team-chat/scripts/runner/net-check.py`（能上外网；连不到主机、内网地址和别的虚拟机）。

### 更新

- **daemon**：用 runner，`cd ~/team-chat && git fetch --tags && git checkout v<新版本号>`，再执行第 4 步从
  `out="$(scripts/build-daemon.sh)"` 起的最后四行。不要停服务：daemon 发现程序文件换了，等手上没有任务时自己退出，systemd 启动新版本。
  等发布物里有了 Linux 的 daemon，这里改成它的自动更新。
- **基础镜像**：用 root，在 `/root/team-chat` 里 `git checkout` 新版本，再运行一次
  `bash scripts/runner/build-base-image.sh`。已经开始的任务继续用旧镜像。
- **主机**：`unattended-upgrades` 自动装安全更新；`git checkout` 新版本后可以再跑一遍 `host-setup.sh`。
  `scripts/runner/runner-vm` 或 `runner-vm.nft` 有改动时一定要再跑一遍（重装 root 辅助脚本、重载防火墙）；
  AI 成员的家目录会保留。
- **从把登录目录共享给虚拟机的旧版本升级**（家目录镜像之前）：用 root 在 `/root/team-chat` 里
  `git checkout` 新版本，再跑一遍 `bash scripts/runner/host-setup.sh`。daemon 会自己更新，可能比这一步早：
  在 `host-setup.sh` 跑完之前，任务照常能跑，只是 AI 成员的家目录还是不限大小的普通文件夹，daemon 的日志和
  `team-chat-daemon doctor` 会提示需要重装 root 辅助脚本。`daemon.toml`
  的 `[vm]` 里如果写过 `shares = [...]` 就删掉（daemon 反正会跳过这些目录）。每个 AI 成员原有的家目录在它
  下一次任务时搬进它的 20 GB 镜像；超过大小的保持为不限大小的文件夹，直到被清空。以前的会话能接着用：
  续接时把那一个会话文件拷进成员自己的目录。

## 还没有人真正走过的部分

这份指南是照着脚本写的，并在没有 Cloudflare 账号的情况下做了检查。下面这些还没有在一台全新的 Ubuntu 机器和
一个新的 Cloudflare 账号上完整走过：

- 第一部分第 1 到 3 步本身（NodeSource、bun 安装脚本、`wrangler login --device`、API 令牌的权限、控制台里选
  子域名和开通 R2）。
- `scripts/setup-server.sh` 对真的 Cloudflare：它用假的 wrangler 和本地的假服务器测过
  （`scripts/test-setup-server.sh`），它生成的 Worker 配置通过了 `wrangler deploy --dry-run`。没跑过的：真正
  建 D1 和 R2、第一次部署、从 wrangler 的输出里读 `workers.dev` 地址、对新 Worker 的 `wrangler secret list`
  和 `secret put`。
- 发布版应用登录自己部署的服务器、对它点「用这台电脑」、在 Ubuntu 上运行 `scripts/release.sh --reupload`。
- 第二部分从一台全新的主机开始：虚拟机内核、在主机上编译 daemon、CLI 登录和上面的配置。
