# 普通成员自己的 AI 成员和分给他的电脑

状态：设计已定，实现中。接在 [design-roles.zh.md](design-roles.zh.md) 后面。

## 1. 目标

负责人 2026-11-13 的原话（整理）：

> 实习生自己可以建 bot，建的 bot 可以随便加到他自己那台电脑上，一个人最多建一个。Linux 主机给每个实习生分一台，权限低一点。规格还是 2 核 4 GB、硬盘 30 GB 起、可以大一点；要在团队管理的电脑里能调。实习生用自己的订阅或 API 登录，不用我的；电脑（虚拟机主机）是我的。

落到这一版是四件事：

1. 普通成员能建 **1 个** 归自己的 AI 成员，能改它（名字、形象、岗位、运行方式、模型、停用），只能放在自己的电脑上。管理员不受限，照旧。
2. 管理员能给某个成员加一台电脑（电脑归那个成员），用来在团队的 Linux 主机上给实习生分电脑。
3. 管理员在 团队管理 › 电脑 里设一台虚拟机电脑的规格（CPU 核数、内存）。每个活都开一台新虚拟机，所以改了从下一个活起生效。
4. 一台 Linux 主机上能跑多个 daemon，每个是一台电脑、一个系统用户，虚拟机槽位互不相干。

## 2. 现状

- `POST /v1/bots`、`PATCH /v1/members/:id`（改 AI 成员）、`GET/PUT /v1/bots/:id/runner` 都只有管理员能用（`core/routes.ts:408`、`notify/members.ts:20`、`bots/routes.ts:189,252`）。
- 普通成员能加自己的电脑、在上面登录订阅、管密钥（`bots/routes.ts:283`，`monitor/accounts.ts`）。
- 虚拟机规格只在 daemon 的 `[vm] vcpus / mem_mib`（`apps/daemon/src/config.rs:101-105`），主机上现在是 2 核 4 GB。
- `runner-vm` 只认一个系统用户（`RUNNER_USER`），槽位从 0 开始，两个 daemon 会抢同一个 tap。

## 3. 设计

### 3.1 AI 成员归谁

- D1 `members.owner_id`（新迁移，NULL = 团队的 AI 成员，旧数据全是 NULL）。线上 `Member.ownerId`（可选）。
- `POST /v1/bots` 改为 `auth: member`：
  - 管理员：照旧；可带 `ownerId` 给某个人建（必须是在用的人）。
  - 普通成员：只能是人；`owner_id = 自己`；已有 1 个没停用的自己的 AI 成员时 403 `bot.own_limit`。上限写死 `MAX_OWN_BOTS = 1`，以后要改再做设置。
  - 判断「已有几个」和插入在同一条语句里，两个并发请求只有一个成功。
- `PATCH /v1/members/:id`：AI 成员的主人能改它的名字、岗位、形象、停用，不能改级别、汇报给、限额。
- `GET/PUT/DELETE /v1/bots/:id/runner`：管理员照旧；AI 成员的主人能读能改，但 `machineId` 必须是自己的电脑（`machine.not_yours`），不能用标签挑「任意一台」（没给 `machineId` 时 400 `bot.own_needs_machine`）。
- 派活：有主人的 AI 成员只落在主人自己的电脑上（`dispatch.ts` 里加一条过滤），管理员改它的设置也一样。别人 @ 它，活照样在主人的电脑上做，用主人的登录（个人电脑「只做主人交代的事」对主人自己的 AI 成员放开）。
- 主人被停用时，他的 AI 成员一起停用。

### 3.2 管理员给成员加电脑

`POST /v1/machines` 加可选 `ownerId`：只有管理员能带，电脑直接归那个人（`owner_id = ownerId`）。返回的机器令牌给管理员，由管理员装到主机上。

### 3.3 虚拟机规格

- D1 `machines.vm_vcpus`、`machines.vm_mem_mib`（NULL = 用 daemon 自己的配置）。
- `PATCH /v1/machines/:id` 加可选 `vm: { vcpus, memMib } | null`，只有管理员能改。范围：1–16 核、1024–65536 MiB。
- `Machine.vm`（可选）给客户端显示。
- `run.assign.vm`（可选，新字段）：服务器派活时带上这台电脑的规格。daemon 用它开虚拟机，但不超过 `[vm] max_vcpus / max_mem_mib`（默认是主机的核数和内存）；没带就用 `[vm] vcpus / mem_mib`。老 daemon 不认这个字段，照旧。
- 硬盘：每个分出去的电脑的工作目录放在主机上一个固定大小的磁盘镜像里（默认 50 GB），主机安装时定，电脑页已有的「剩余空间」就是它。

### 3.4 一台主机多个 daemon

- `[vm] slot_base`（默认 0）：这个 daemon 的槽位从这里开始。
- `runner-vm` 不再信环境变量，用 `SUDO_USER` 认人；`/etc/runner-vm.users` 每行 `用户 起始槽位 个数`，不在名单里、槽位不在自己范围里的一律拒绝；导出只许自己的家和 `/srv/runner/u/<用户>`。
- `host-setup.sh add-user <用户> <起始槽位> <个数> <硬盘 GB>`：建系统用户、加到 kvm 组、建磁盘镜像挂到 `/srv/runner/u/<用户>`、写 sudoers 和名单。共用只读的内核和根镜像。
- 每个分出去的电脑：`max_parallel_runs = 1`，规格 2 核 4 GB。

### 3.5 桌面端

- 成员页：普通成员看到「添加 AI 成员」，没建过才可点；表单的电脑只列自己的电脑。自己的 AI 成员能打开编辑。
- 电脑页（管理员）：虚拟机电脑多一行「规格」，选核数和内存；「添加电脑」可以选归谁。

## 4. 不做的

- 网关（统一的模型出口）以后再说。
- 用团队的订阅给实习生的 AI 成员，以后再说；现在各用各的登录。
- 普通成员的上限做成设置。

## 5. 上线顺序

1. 协议和服务端（3.1–3.3）。
2. daemon 和主机脚本（3.3 的 daemon 部分、3.4）。
3. 桌面端（3.5）。
4. 实习生 1、2 用邀请码进来后：主机上 `add-user`，管理员给他们各加一台电脑、各建一个归他们的 AI 成员放上去；他们自己在电脑上登录订阅。
