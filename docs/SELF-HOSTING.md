# Run your own server

中文版：[SELF-HOSTING.zh.md](SELF-HOSTING.zh.md).

Every team runs its own server, and so does a person who uses the app alone: there is no hosted
service. The server is a Cloudflare Worker with Durable Objects, a D1 database and an R2 bucket,
on your own Cloudflare account. You set it up once from a Linux machine with the scripts in this
repository; the app on your Mac or Windows computer then signs in to it.

Part 1 takes you from a fresh Ubuntu machine and a Cloudflare account to the app signed in and
this computer working for your AI members. Part 2 is for a team that also runs AI members on a
Linux host in microVMs; skip it when your own computer is enough.

Only Ubuntu (24.04 LTS) is supported for running these scripts. They also run on macOS, but this
guide does not cover it.

| What you get | Where |
| --- | --- |
| The server: Worker `chat-api-prod`, D1 database `chat-prod`, R2 bucket `chat-files-prod` | your Cloudflare account |
| The server address `https://chat-api-prod.<your-subdomain>.workers.dev` | `~/.config/team-chat/ops/deploy/servers.json` |
| The deploy config of your server | `~/.config/team-chat/ops/deploy/server.wrangler.jsonc` |
| The server's secrets and your owner token | `~/.config/team-chat/production/` (folder mode 700, files mode 600) |

`<your-subdomain>` and every other `<...>` below is a placeholder: use your own value.

## Part 1. Your server

### What you need

- A machine with Ubuntu 24.04 LTS where you can use `sudo`: a spare computer, a virtual
  machine or a small cloud server. Nothing keeps running on it after the setup; it is where you
  run the scripts and keep the secret files. Keep it, or copy its two folders (table above)
  somewhere safe: later updates of the server run from there.
- A Cloudflare account (the free plan is enough to start).
- A Mac (Apple silicon, macOS 13 or later) or a Windows 10/11 computer for the app.
- For your AI members to work on that computer: Claude Code or Codex, installed and signed in
  there.

### 1. Install the tools on Ubuntu

```sh
sudo apt-get update
sudo apt-get install -y git curl jq openssl unzip ca-certificates
# Node.js 22 or later (wrangler needs it; Ubuntu's own package is older)
curl -fsSL https://deb.nodesource.com/setup_22.x -o /tmp/nodesource_setup.sh
sudo -E bash /tmp/nodesource_setup.sh
sudo apt-get install -y nodejs
# bun 1.3
curl -fsSL https://bun.sh/install | bash -s bun-v1.3.10
exec "$SHELL" -l      # a new shell, so bun is on PATH
```

Check: `node --version` prints v22 or later, `bun --version` prints 1.3.10, and `jq --version`
works.

### 2. Get the code

Deploy the same version as the app you will install: the newest release.

```sh
git clone https://github.com/Joye-Co/team-chat.git
cd team-chat
git checkout "$(git describe --tags --abbrev=0 origin/main)"   # the newest release tag, v<version>
[ -x scripts/setup-server.sh ] || git checkout main             # releases up to v1.2.7 lack it
bun install --frozen-lockfile
```

`scripts/setup-server.sh` first ships in the release after v1.2.7. Until that release exists,
the second line keeps you on `main`; install the newest app all the same.

Check: `git describe --tags` prints `v<version>` (on `main`, `v<version>-<n>-g<commit>`), and `ls server/node_modules/.bin/wrangler`
finds wrangler. The server reports this version at `/v1/health` after the deploy.

### 3. Prepare the Cloudflare account

1. Sign up at <https://dash.cloudflare.com/sign-up> if you have no account.
2. Open **Workers & Pages** in the dashboard once. On a new account it asks you to choose your
   `workers.dev` subdomain: that is `<your-subdomain>` in your server's address.
3. Open **R2 Object Storage** and turn it on. Cloudflare asks for a payment method even for
   the free allowance.
4. Sign wrangler in. On a machine without a browser, use the device sign-in: it prints a link
   and a code that you open on any computer.

   ```sh
   cd server
   bunx wrangler login --device
   bunx wrangler whoami        # check: shows your account
   cd ..
   ```

   Instead of signing in you can use an API token (dashboard, **My Profile › API Tokens ›
   Create Token**) with the account permissions Workers Scripts Edit, D1 Edit, Workers R2
   Storage Edit and Account Settings Read. Give it to wrangler without writing it into your
   shell history:

   ```sh
   read -rs CLOUDFLARE_API_TOKEN && export CLOUDFLARE_API_TOKEN
   ```

When your login has more than one account, also `export CLOUDFLARE_ACCOUNT_ID=<account id>`
(`bunx wrangler whoami` lists them).

### 4. Set up and deploy the server

```sh
scripts/setup-server.sh production --owner-name "<your name>"
```

The script checks each step before it acts, so you can run it again at any time: it repairs
what is missing and changes nothing else. What it does:

| Step | What it creates | Kept in |
| --- | --- | --- |
| 1. prerequisites | nothing; for each missing tool or sign-in it says what to install or run | |
| 2. ops folder | the folder for your deployment details | `~/.config/team-chat/ops/` (or `$TC_OPS_DIR`) |
| 3. D1 and R2 | D1 database `chat-prod` (location hint `oc`), R2 bucket `chat-files-prod` | Cloudflare |
| 4. Worker config | `deploy/server.wrangler.jsonc` with an `env.production` block (Worker name, D1 id, bucket) | ops folder |
| 5. secret files | `admin-secret` (guards the admin routes) | `~/.config/team-chat/production/` |
| 6. deploy | D1 migrations, the Worker with its four Durable Objects and the daily archive cron; the server address in `deploy/servers.json`; then it waits until `/v1/health` reports the version | Cloudflare, ops folder |
| 7. Worker secrets | `ADMIN_SECRET` and `HOOK_SECRET_KEY` (it encrypts the AI 服务 key and hook secrets stored on the server; its file `hook-secret-key` is made here) | Cloudflare, `~/.config/team-chat/production/` |
| 8. first owner | you, as the team's owner, with the default group; your owner token; a sign-in code for the app | `owner-token`, `signin-<id>-<time>.txt` in `~/.config/team-chat/production/` |
| 9. next step | prints your server address, where the sign-in code is, and what to do in the app | |

The script never prints a secret, a token or the sign-in code; it prints the paths of their
files. To use other names, pass `--worker`, `--d1` and `--bucket` on the first run. To use your
own domain instead of `workers.dev`, add the route to the config and pass `--url <base>`.

Check:

```sh
curl https://chat-api-prod.<your-subdomain>.workers.dev/v1/health
# {"ok":true,"version":"<version>"}
bun server/scripts/admin.ts members --env production
# one line: <id>  human  owner  active  <your name>
```

If a step fails, the message says what to do; see also [When something goes wrong](#when-something-goes-wrong).

### 5. Install the app and sign in

1. Download the app of the same version from
   <https://github.com/Joye-Co/team-chat/releases>: the `.dmg` for a Mac, the
   `-setup.exe` for Windows. Install it and open it the first time as
   [INSTALL.md](INSTALL.md) describes (steps 2 and 3, or [Windows](INSTALL.md#windows)).
2. On the sign-in screen, choose **使用其他服务器…** and enter your server address,
   `https://chat-api-prod.<your-subdomain>.workers.dev`.
3. On Ubuntu, show your newest sign-in code:
   `cat "$(ls -t ~/.config/team-chat/production/signin-*.txt | head -n 1)"`
   (the path is the last thing step 4 printed; older files hold codes already used). Type it into the **邀请码** field and click
   **加入**.

The code works once and for 7 days. For a new one, run
`scripts/setup-server.sh production --sign-in-code`.

Check: the app opens as you, with your team's default group. 设置 › 机器 is there.

### 6. Let your AI members work on this computer

1. Install Claude Code or Codex on this computer and sign in to it, so that `claude --version`
   or `codex --version` works in a terminal. The app does not check the sign-in yet; run the
   CLI once and make sure it answers.
2. In the app open **设置 › 机器** and click **用这台电脑**. The app registers this computer
   with your server, writes the background service's config and token and starts it (launchd
   on a Mac, a scheduled task on Windows). It updates the service with the app.

Check: the computer is in the list in 设置 › 机器 and online. On Ubuntu,
`bun server/scripts/admin.ts machines --env production` shows it as `healthy`.

### 7. Your first AI member

In **团队管理**, click **添加 AI 成员**, let it work on this computer, then @
it in a topic of your group and give it something to do. [ONBOARDING.zh.md](ONBOARDING.zh.md)
explains the app.

To bring people in, make an invite code for each in the app (团队管理 › 成员), or on Ubuntu:
`bun server/scripts/admin.ts invite --env production --name <name>` (it writes the code to a
file and prints the path). They install the app from the releases page and sign in with your
server address and their code.

### Update the server

Keep the server at the version of the app: update it when a new release comes out.

```sh
cd team-chat
git fetch --tags origin
git checkout "$(git describe --tags --abbrev=0 origin/main)"
[ -x scripts/setup-server.sh ] || { git checkout main && git pull --ff-only; }
bun install --frozen-lockfile
scripts/setup-server.sh production
```

It deploys the new version (D1 migrations first) and leaves everything else as it is.
`scripts/deploy-server.sh production` does the deploy alone.

The app asks your server for its updates, and invite links download the app from it. Your
server has app files only when you give them to it; until then, people update by downloading
the new version from the releases page. To give your server the files of a release, install the
GitHub CLI (`sudo apt-get install -y gh`, then `gh auth login`) and run
`scripts/release.sh <version> --env production --reupload`: it downloads the DMG and the Windows
installer of that release, checks them against the release's SHA-256 digests and uploads them
with your admin secret.

### Keep these safe

| File | Why |
| --- | --- |
| `~/.config/team-chat/production/hook-secret-key` | Without it, the AI 服务 key and the hook secrets stored on the server cannot be read: an owner enters the key again and every hook is made again. The script never replaces this secret on the Worker. |
| `~/.config/team-chat/production/admin-secret` | Guards the admin routes. If you lose it, run the setup again: it makes a new one and puts it on the Worker. |
| `~/.config/team-chat/production/owner-token` | Lets the scripts act as you (`admin.ts`, `--sign-in-code`). The scripts cannot make a new one once the owner exists, so a lost file means doing those things in the app instead. |
| `~/.config/team-chat/ops/` | Your deploy config and server address. |

To run updates from another machine, copy both folders there (`scp -r`, keep the modes).

[RUNBOOK.md](RUNBOOK.md) covers backups (the daily archive to R2), restore, rollback and
rotating secrets. [ENVIRONMENTS.md](ENVIRONMENTS.md) explains the ops folder, the secret files
and a staging server next to production (`scripts/setup-server.sh staging`).

### When something goes wrong

| What you see | What to do |
| --- | --- |
| `missing: ...` at step 1 | Install what it names, then run the script again. |
| `wrangler is not signed in` | Step 3.4. With an API token, check that it is exported in this shell. |
| `your Cloudflare login has N accounts` | `export CLOUDFLARE_ACCOUNT_ID=<id>` and run again. |
| `could not create R2 bucket` | Turn R2 on in the dashboard (step 3.3). |
| `deployed, but wrangler printed no workers.dev address` | Your account has no `workers.dev` subdomain yet (step 3.2), or you use your own domain: run again with `--url <base>`. |
| `could not make the first owner` right after the first deploy | A new secret can take a moment to arrive: run the script again in a minute. |
| `this server has an owner already` | The owner was made from another machine: copy its `~/.config/team-chat/production/` folder here. |
| Saving the AI 服务 in the app fails with `hooks are not configured on this server` | The Worker has no `HOOK_SECRET_KEY`: run `scripts/setup-server.sh production`. |
| The app says 连不上服务器 | Check the address (with `https://`) and `curl <address>/v1/health`. |
| `/v1/health reports <x>, expected <y>` | The deploy did not take effect yet, or failed: run `scripts/deploy-server.sh production` again and read its output. |

Logs of the server: `cd server && bunx wrangler -c wrangler.deploy.jsonc tail --env production`
(after a deploy, which writes `wrangler.deploy.jsonc`).

## Part 2. A Linux host for the team's AI members

A team can run its AI members on one Linux host instead of on people's computers. Each run's
CLI then runs in its own Firecracker microVM ([machine-protocol.md](machine-protocol.md#microvm-runs-vm-linux)),
and people who sign in with an invite code use these AI members without setting up a computer.

### What it needs

- Ubuntu 24.04 LTS on x86_64, with root access over SSH.
- KVM: a bare-metal server, or a virtual machine with nested virtualization. Check:
  `ls -l /dev/kvm` exists, and `grep -Ecw 'vmx|svm' /proc/cpuinfo` is more than 0.
- Memory: each running VM may use up to 4 GiB (`[vm] mem_mib`); plan for that times the runs
  you allow at once (`max_parallel_runs`).
- Disk: the base image (a few GB), 32 GB of sparse scratch space per running VM, and the AI
  members' home folders in `/srv/runner/homes`. Start with 200 GB free.
- Your server from Part 1, and the Ubuntu machine with its `~/.config/team-chat/production/`.

### Install

1. **Host setup**, as root on the host:

   ```sh
   git clone https://github.com/Joye-Co/team-chat.git /root/team-chat
   cd /root/team-chat && git checkout v<version>      # the version of your server, or main
   bash scripts/runner/host-setup.sh
   ```

   It installs the system packages, Docker, Firecracker and jailer (checked against their
   SHA-256), the `runner` user (groups kvm and docker, lingering on), `/srv/runner` for images
   and VMs, IP forwarding, the NFS server, the VMs' firewall (`runner-vm-net.service`) and the
   root helper `/usr/local/sbin/runner-vm` that the daemon may run with sudo. It copies root's
   SSH keys to the `runner` user. It is safe to run again.
2. **Guest kernel.** The daemon boots `/srv/runner/kernels/vmlinux`, an uncompressed kernel with
   virtio block, virtio-vsock, overlayfs and the NFS client. No script in this repository
   makes it. A starting point is a Firecracker CI kernel (see Firecracker's getting-started
   guide, "Running Firecracker"): put it there as `vmlinux`, owned by `runner`. Step 5 shows
   whether it boots.
3. **Base image**, as root: `bash scripts/runner/build-base-image.sh`. It builds
   `scripts/runner/base-image/Dockerfile` (Ubuntu with git, Node, Claude Code, Codex, gh and
   more) into `/srv/runner/images/base-<date>.ext4` and points `base.ext4` at it.
4. **The daemon**, as runner (`ssh runner@<host>`; the rest of Part 2 runs as runner unless it
   says root). Today it is built from source on the host:

   ```sh
   curl --proto '=https' -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal
   . "$HOME/.cargo/env"
   git clone https://github.com/Joye-Co/team-chat.git ~/team-chat
   cd ~/team-chat && git checkout v<version>      # the version of your server, or main
   out="$(scripts/build-daemon.sh)"   # rustup installs the toolchain of rust-toolchain.toml
   mkdir -p ~/.local/bin
   install -m 755 "$out" ~/.local/bin/team-chat-daemon.new
   mv -f ~/.local/bin/team-chat-daemon.new ~/.local/bin/team-chat-daemon
   ```

   <!-- RELEASED DAEMON: when releases carry a Linux daemon, replace the commands above with the
        download of that asset and its SHA-256 check, and describe its self-update here. -->
   **Released daemon:** once releases carry a Linux daemon, this step becomes a download from
   the releases page; until then, use the commands above.
5. **Boot check**: `bash ~/team-chat/scripts/runner/boot-test.sh`. Expected: `vm-init: ready`,
   a line per tool with its version, and `firecracker exited after` a few seconds. A kernel
   that lacks something stops here with a kernel panic in the output.
6. **Register the host as a computer of the team.** On the Ubuntu machine of Part 1:

   ```sh
   bun server/scripts/admin.ts machine --env production --name <host name> --labels linux,vm
   ```

   It writes the machine token to `~/.config/team-chat/production/machine-<name>-<id>.token`
   and prints its path. Copy it to the host without showing it:

   ```sh
   ssh runner@<host> 'mkdir -p -m 700 ~/.config/team-chat/production'
   scp ~/.config/team-chat/production/machine-<name>-<id>.token runner@<host>:.config/team-chat/production/machine-token
   ssh runner@<host> 'chmod 600 ~/.config/team-chat/production/machine-token'
   ```

7. **CLI sign-ins.** As root, install Node.js 22 (the NodeSource lines of Part 1 step 1) and
   the CLIs: `npm install -g @anthropic-ai/claude-code @openai/codex`. Then as runner sign in:
   run `claude` and follow its sign-in, and `codex login --device-auth`. The VMs use these
   sign-ins: `~/.claude` and `~/.codex` of the runner user are shared with every VM.
8. **Config**, as runner: `team-chat-daemon example-config > ~/.config/team-chat/daemon.toml`,
   then edit it:

   ```toml
   server_url = "https://chat-api-prod.<your-subdomain>.workers.dev"
   token_file = "~/.config/team-chat/production/machine-token"
   name = "<host name>"
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

9. **Start it**, as runner:

   ```sh
   ~/.local/bin/team-chat-daemon doctor            # exit code 0: nothing wrong
   ~/.local/bin/team-chat-daemon install-service   # systemd user service team-chat-daemon.service
   ```

### Check

- `systemctl --user status team-chat-daemon` is `active (running)`; the log is
  `~/.local/state/team-chat-daemon/daemon.log`.
- On the Ubuntu machine of Part 1, `bun server/scripts/admin.ts machines --env production`
  shows the host as `healthy`; in the app it is online in 设置 › 机器.
- Let an AI member work on this host (pick the host where the app lets you choose the AI
  member's computer, or on the Ubuntu machine
  `bun server/scripts/admin.ts runner --env production --bot <member id> --runtime claude --labels vm`)
  and @ it in a topic. After its first run, check the VM network as runner:
  `python3 ~/team-chat/scripts/runner/net-check.py` (the internet works; the host, private
  ranges and other VMs do not).

### Update

- **Daemon**: as runner, `cd ~/team-chat && git fetch --tags && git checkout v<new version>`,
  then the last four lines of step 4, from `out="$(scripts/build-daemon.sh)"` on. Do not stop the service: the daemon sees
  the new file, waits until no run is active, exits, and systemd starts the new version.
  When releases carry a Linux daemon, this is where its self-update replaces these steps.
- **Base image**: as root, `git checkout` the new version in `/root/team-chat` and run
  `bash scripts/runner/build-base-image.sh` again. Runs that already started keep their image.
- **Host**: `unattended-upgrades` installs security updates; `host-setup.sh` may be run again
  after `git checkout` of a new version.

## What nobody has run for real yet

This guide was written from the scripts and checked without a Cloudflare account. These parts
have not been carried out end to end on a fresh Ubuntu machine and a new Cloudflare account:

- Part 1 steps 1 to 3 as written (NodeSource, the bun installer, `wrangler login --device`, the
  API token permissions, the dashboard steps for the subdomain and R2).
- `scripts/setup-server.sh` against Cloudflare: it is tested with a fake wrangler and a stub
  server (`scripts/test-setup-server.sh`), and its Worker config passes `wrangler deploy
  --dry-run`. Not run: real D1 and R2 creation, the first deploy, reading the `workers.dev`
  address from wrangler's output, `wrangler secret list` and `secret put` on a new Worker.
- Signing in with a self-hosted server in a release build of the app, 用这台电脑 against it, and
  `scripts/release.sh --reupload` from Ubuntu.
- Part 2 from a fresh host: the guest kernel, the daemon built on the host, the CLI sign-ins and
  the config above.
