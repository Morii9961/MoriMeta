# MoriMeta — Architecture

> **Version:** 0.3 · **Status:** 草案（未批准）· **Date:** 2026-09-26
> v0.3 的修改以 [SPIKE_REPORT](SPIKE_REPORT.md) 的 S0/S1/S2/S3/S7 结果为依据；未经验证的部分标为"候选"或"待 Sx"。
> 本文定义模块边界、数据流、进程模型与关键技术决策。安全细节见 [SAFETY_MODEL](SAFETY_MODEL.md)，字段语义见 [METADATA_MODEL](METADATA_MODEL.md)，威胁模型见 [SECURITY_MODEL](SECURITY_MODEL.md)。
> `[F-xx]` / `[V-xx]` 引用 [RESEARCH_NOTES](RESEARCH_NOTES.md)。

---

## 1. 架构目标

1. **正确性与安全性可在 UI 之外被测试**：所有决定"写什么、写到哪、如何写"的逻辑位于不依赖 Tauri 的 Rust 库中，可被 CLI 测试工具和故障注入测试直接驱动。
2. **前端不做决策**：前端只渲染后端给出的状态、收集用户意图；不计算 Plan、不拼参数、不接触文件系统路径做写操作。
3. **ExifTool 被当作不可信的强大工具**：它的输入由白名单构造，它的输出被验证，它只能写临时文件。
4. **所有写操作可恢复**：以 Operation Journal 为唯一事实来源；在 SAFETY_MODEL §0 的前提下，进程在任何时刻被终止后都能判定每个文件的状态（S2 原型已验证 NTFS 与 SMB 回环）。
5. **批量优先**：扫描、计划、执行均为流式、可取消、有背压。

---

## 2. 关键决策（ADR 摘要）

| ADR | 决策 | 状态 |
|---|---|---|
| ADR-01 | 桌面壳：**Tauri 2（2.11.x）+ React + TypeScript + Vite** | 建议保留，S5 验证（未做） |
| ADR-02 | 核心逻辑为 **UI 无关的 Rust workspace**；Tauri 仅为薄适配层 | 建议 |
| ADR-03 | ExifTool 13.59（规则：包含全部已知安全修复的最新版本）作为 `bundle.resources` 打包，由 Rust 直接启动；不使用 Tauri shell 插件与 externalBin。调用方式二选一（D-17）：A 官方 launcher（重命名为 `exiftool.exe`）；B 直接 `perl.exe exiftool.pl`（S0 验证行为等价） | 版本：S0 已验证；调用方式：待决 |
| ADR-04 | **Plan-based execution**：执行的是不可变、已预览并持久化的 Plan | 建议 |
| ADR-05 | 写入模型：锁句柄（READ，共享 READ\|DELETE）→ 经锁句柄备份 → ExifTool `-o` 以备份副本为源写临时文件 → 验证 → 核对身份 → `ReplaceFileW`（bak 名预先登记） | S2 原型验证（NTFS、SMB 回环） |
| ADR-06 | Operation Journal 与 History：**SQLite（WAL, synchronous=FULL）** + 备份目录内自描述 manifest | 建议（协议经 S2 以 JSON 行原型验证） |
| ADR-07 | MVP 不持久化 Library 元数据缓存；每次导入重新扫描 | 建议 |
| ADR-08 | 读取时自行调和；**写入采用显式映射**，不用 MWG Composite 写入（MWG 在 Latin IPTC 中静默写入 `?`） | S3 验证 |
| ADR-09 | Rust → TypeScript 类型自动生成（tauri-specta 或 ts-rs） | 待 V-18 |
| ADR-10 | 参数编码：写入命令带 `-ex`，值用 XML 字符引用；读取用 `-api StructFormat=JSONQ`；不使用 `#[CSTR]` | S1 验证（100,000/100,000） |
| ADR-11 | ExifTool 进程放入 Job Object（KILL_ON_JOB_CLOSE）；每条命令用随机 64 位 ID 作为终止标记 | S1 验证 |

### 2.1 ADR-01：为什么仍选 Tauri 2 + React（以及它可能错在哪里）

候选比较（面向"Windows 首发、5,000 行高密度表格、Rust 安全核心、以后跨平台"）：

| 方案 | 优势 | 劣势 | 结论 |
|---|---|---|---|
| **Tauri 2 + React** | 核心可用 Rust 编写并与 UI 解耦；安装包小；显式 Capability 权限模型 [F-63]；WebView2 由系统维护安全补丁；Web 生态有成熟的虚拟表格、无障碍组件、i18n | 依赖 WebView2（Win10/11 已随系统分发 [F-64]）；原生感需靠设计；IPC 序列化开销；无原生 MSIX [F-65] | **选用** |
| Electron | Chromium 版本可控、生态最大 | 体积与内存大；Node 主进程带来额外攻击面；核心仍需 Rust/原生模块才能获得同等可靠性 | 无决定性优势 |
| .NET（WinUI 3 / WPF / Avalonia） | Windows 原生、成熟工具链、DataGrid | 与 Rust 核心不一致（若核心也用 C# 则可行）；WinUI 3 打包复杂；Avalonia 跨平台但高密度表格需自研 | **唯一有竞争力的替代**；若团队更熟悉 C#，可整体切换 |
| Qt（C++/QML） | 原生性能 | LGPL/商业许可负担；C++ 在文件安全关键代码中的风险 | 不选 |
| 纯 Rust GUI（egui / iced / Slint） | 单语言 | 高密度可访问表格、IME、屏幕阅读器支持不成熟；Slint 许可证有条件 | 不选 |

**缓解"选错"的风险**：ADR-02 保证业务核心与 UI 解耦；若 S5 发现 WebView2 表格/IME/无障碍无法达标，更换 UI 壳只影响 `apps/desktop`。

S5 退出条件：5,000 行 × 20 列虚拟表格在中端笔记本上滚动 ≥ 50 fps；排序/筛选 < 200 ms；中文 IME 在表格内联编辑与输入框中正常；Narrator 可读出行与单元格。

### 2.2 ADR-03：ExifTool 的打包与调用

- Windows 包为 `exiftool(-k).exe` + `exiftool_files/`（34.5 MB、510 个文件，内含 Perl 5.32.1）[F-30]。Tauri externalBin 要求单文件可执行 [F-62]，因此以 `bundle.resources` 打包整个目录。
- 调用方式（D-17）：
  - **A. 官方 launcher**：打包时把 `exiftool(-k).exe` 重命名为 `exiftool.exe`（括号内容会被当作选项 [F-23]）。launcher 以 CC0 发布；CC0 不是 OSI 认可的许可证，这与某些签名渠道的条件有关（RELEASE_PLAN §4）。
  - **B. 直接调用 Perl**：`exiftool_files\perl.exe exiftool_files\exiftool.pl`，不打包 launcher。S0：194/194 个文件读取输出相同、8/8 个写入样本逐字节相同、长路径与中文路径正常；launcher 本身也在进程内运行 Perl，不另起子进程。
- 由 Rust 使用 `std::process::Command` 直接启动；前端**不获得任何进程启动能力**，不启用 `tauri-plugin-shell`。
- 版本锁定规则：**包含全部已知安全修复的最新版本**（当前 13.59；最新 production release 13.55 早于 13.59 的安全更新）。升级须通过完整回归语料。
- 构建时校验官方 SHA-256（已固定在 `research/exiftool.lock.json`）；运行时校验打包文件完整性（SECURITY_MODEL §5）。

---

## 3. 系统上下文

```text
┌──────────────────────────── MoriMeta.exe (Tauri process) ─────────────────────────────┐
│                                                                                        │
│  WebView2 (React UI)            IPC (typed commands, channels)          Rust core       │
│  ─────────────────────   ◄──────────────────────────────────────►   ───────────────   │
│  • renders state                                                    • app services     │
│  • collects intent                                                  • domain logic     │
│  • no fs / no process                                               • journal / backup │
│                                                                     • exiftool pool ───┼──► exiftool.exe ×N
│                                                                                        │     (stay_open, stdin argfile)
└────────────────────────────────────────────────────────────────────────────────────────┘
          │                                                   │                    │
          ▼                                                   ▼                    ▼
   (optional update check,                         %LOCALAPPDATA%\MoriMeta     User photo folders
    backend only, D-4)                             db / backups / logs          (lock; temp+replace on write)
```

---

## 4. 模块划分

### 4.1 Rust workspace

```text
crates/
├── mm-domain        纯领域模型与算法（无 IO、无 async、无外部进程）
├── mm-exiftool      ExifTool 进程池、协议、参数构造、输出解析、错误分类
├── mm-fs            文件系统原语：身份、规范化、ReplaceFileW、flush、空间、属性探测
├── mm-store         SQLite（journal、history、settings）、备份库、manifest
├── mm-core          应用服务：Scanner、Planner、Executor、UndoService、Recovery、PresetService
├── mm-testkit       测试语料管理、故障注入钩子、假 ExifTool 引擎（仅 dev-dependency）
└── mm-cli           开发与测试用命令行（MVP 不对外发布；v2 可演进为 headless）
apps/
└── desktop/
    ├── src-tauri/   Tauri 适配层：commands、events、capabilities、窗口与拖放
    └── src/         React 前端
```

依赖方向（严格单向，CI 用 `cargo-deny` 的 bans / 自定义检查约束）：

```text
mm-domain  ◄── mm-exiftool
    ▲      ◄── mm-fs
    │      ◄── mm-store
    └───────── mm-core ◄── src-tauri
                       ◄── mm-cli
```

| Crate | 职责 | 禁止 |
|---|---|---|
| `mm-domain` | FieldRegistry、值类型、时间模型、规则/条件/动作、模板求值、Plan/Change/Diff 计算、聚合（混合值）、字段↔Tag 映射表 | 任何 IO；依赖 tokio；知道 ExifTool 进程的存在 |
| `mm-exiftool` | `ExifToolPool`、`Session`（单进程 stay_open 协议）、`ArgBuilder`（从类型化 `WriteRequest` 构造 argfile 行）、JSON 解析为 `RawTagSet`、错误/警告分类、版本与完整性检查 | 接收字符串形式的"任意参数"；接触备份/日志 |
| `mm-fs` | FileIdentity（卷序列号 + 128-bit File ID）、路径规范化、reparse point/硬链接数/只读/云占位符/可移动介质探测、`replace_file`、`create_new`、`flush`、卷空闲空间、同卷判断 | 业务判断 |
| `mm-store` | 数据库 schema 与迁移、Journal 状态机持久化、备份写入/校验/恢复、manifest、保留策略 | 调用 ExifTool |
| `mm-core` | 编排：导入 → 扫描 → 计划 → 预览查询 → 执行 → 撤销 → 恢复；并发与取消；进度事件 | UI 类型 |
| `src-tauri` | 将 `mm-core` 服务暴露为命令；把 Channel/事件桥接到前端；文件对话框与拖放在 Rust 侧处理 | 业务逻辑 |

### 4.2 前端

```text
apps/desktop/src/
├── app/             Shell、路由（Library / Presets / History / Settings）、全局快捷键
├── features/
│   ├── library/     表格、筛选、列配置
│   ├── inspector/   单文件 / 批量编辑器
│   ├── time-tools/
│   ├── presets/     规则构建器、Preset 管理
│   ├── preview/     Plan 浏览、排除、确认
│   ├── jobs/        进度、完成摘要
│   ├── history/     Operation 列表与详情、Undo
│   └── recovery/
├── ipc/             生成的类型 + 命令封装（唯一可以调用 invoke 的地方）
├── state/           Zustand stores（UI 状态）+ 查询缓存（TanStack Query）
├── design/          tokens（CSS variables）、基础组件（基于 Radix primitives）
└── i18n/            en / zh-CN 资源（i18next）
```

- 表格：TanStack Table + TanStack Virtual。
- 前端持有的数据：当前 Session 的行摘要（按需分页拉取）、当前 Plan 的分页视图、UI 状态。**权威数据在 Rust**。
- 所有元数据值以纯文本渲染；禁止 `dangerouslySetInnerHTML`；不加载任何远程资源。
- 设计系统等待 Design 阶段的方向选择：按设计简报 §21–22，先做三个视觉方向供人工选择，选定后再展开完整界面与 DESIGN.md。产品 UI 的实现以此为前提（DEVELOPMENT_PLAN §4）；架构只约束 tokens 化与可访问性。

---

## 5. IPC 契约

### 5.1 Handle 模型（安全关键）

前端**永远不以路径作为写操作的参数**。

- 文件进入 Session 的唯一途径：Rust 侧打开的系统文件对话框，或 Rust 侧处理的窗口拖放事件（`WindowEvent::DragDrop`）。
- 后端为每个文件分配 `AssetId`（Session 内有效的不透明 ID）。前端拿到的路径仅用于显示。
- 写操作只能引用后端生成的 `PlanId`；执行命令需要携带 Plan 版本号与用户确认令牌（由 Preview 界面在用户确认时取得）。
- 输出目录（Clean Export，若 D-15 纳入）也只能来自 Rust 侧的目录对话框，后端返回 `OutputDirId`。

实现状态（2026-09-27，`mm-core::service`）：`Session`（`AssetId`，按卷 + File ID 去重；文件夹导入只依据目录列表，不跟随目录链接、不读取云占位符，跳过 XMP 与事务临时名 `.mmtmp-`/`.mmbak-`；其他格式目前只计数）、`PlanBook`（Plan 不可变；排除生成新版本，旧版本只读；按状态筛选分页，每页 200 行）、一次性确认令牌（绑定 Plan 版本，新版本使旧令牌失效）、`OperationGate`（写许可可并存；独占许可要求无写入且无待恢复 Operation）。单元测试 + 以真实 ExifTool 走完 导入 → Plan → 分页 → 排除 → 确认 → 执行 → 撤销 的集成测试。文件对话框、拖放、进度 Channel 与异步 Plan 生成属于 Tauri 适配层，尚未实现。

### 5.1a OperationGate（写操作与更新安装共用）

- 后端持有一个 OperationGate：执行写操作（Apply、Undo、Recovery、Export）需要取得"写入"许可；安装更新需要取得"独占"许可，且要求不存在运行中或待恢复的 Operation。取得独占许可后拒绝启动新的写操作。
- **前端不获得任何 updater 插件权限。** 更新只通过后端自定义命令 `update_check` / `update_download` / `update_install` 进行；`update_install` 在交给安装程序之前关闭 ExifTool 进程池并使 Journal 落盘。Windows 上安装时应用会被自动退出 [F-61]，该流程待 S6 验证。

### 5.2 命令（示意，最终由类型生成）

| 命令 | 说明 |
|---|---|
| `session_import_dialog(kind)` / `session_import_dropped(drop_id)` | 导入文件/文件夹；返回 `ImportJobId` |
| `session_rows(query)` | 分页/排序/筛选后的行摘要 |
| `asset_detail(asset_id)` | Inspector 数据：字段、来源、冲突、原始标签 |
| `selection_aggregate(asset_ids \| query)` | 批量编辑器的聚合状态 |
| `plan_create(selection, edits \| preset_id)` | 生成 Plan（异步，带进度） |
| `plan_page(plan_id, version, filter, page)` | Preview 分页数据 |
| `plan_exclude(plan_id, version, exclusions)` | 返回新版本 |
| `plan_preflight(plan_id, version)` | 空间、只读、锁定、指纹变化等检查 |
| `op_execute(plan_id, version, confirm_token)` | 执行，返回 `OperationId` |
| `op_cancel(operation_id)` | 协作式取消 |
| `history_list(page)` / `op_detail(id)` / `op_undo_plan(id, scope)` | Undo 同样先生成 Plan |
| `recovery_status()` / `recovery_resolve(op_id, action)` | 崩溃恢复 |
| `presets_*`、`settings_*`、`backup_usage/prune_plan` | |
| `update_check` / `update_download` / `update_install` | 仅后端实现，经 OperationGate（§5.1a） |

### 5.3 进度与事件

长任务使用 Tauri Channel 流式推送（批量合并，≤ 4 次/秒）：`ScanProgress`、`PlanProgress`、`ExecProgress { done, total, ok, warn, fail, skipped, current }`、`FileResult`（失败项即时推送）。

实现状态：`mm-core::executor::ExecProgress`（total、done、ok、failed、skipped、刚结算的文件）经 `ExecOptions.progress` 在每个文件结算后按完成顺序回调；批量合并与 Channel 属于适配层。`warn` 待有警告类结果时加入。

---

## 6. 核心数据流

### 6.1 导入与扫描

```text
Drop/Dialog (Rust)
  → Enumerate (mm-fs: walk, 不跟随目录 reparse point, 分类扩展名)
  → Identify (FileIdentity 去重, 探测只读/云占位符/硬链接/可移动介质)
  → Pair sidecars (<base>.xmp → RAW；<base>.<ext>.xmp 标记为 darktable)
  → Emit rows (文件名/类型立即显示)
  → Scan queue (按卷分组, 分块 100–200 文件/命令)
      → ExifToolPool.read(chunk, SCAN_TAGS) → RawTagSet per file
      → mm-domain::reconcile(RawTagSet [+ sidecar RawTagSet]) → FieldSnapshot (值 + 来源 + 冲突)
      → Session index (内存) + Fingerprint(size, mtime, file_id)
  → Stream row updates to UI
```

- 扫描只请求 SCAN_TAGS 列表（显示与计划所需字段），不读取全部标签；Inspector 打开单文件时再读取全部标签（`-a -G1 -s`，只读显示）。
- Clean Export 与隐私明细不受 SCAN_TAGS 限制：对源文件做完整读取（`-a -G0:1 -u -U`），并由 Rust 解析 JPEG 标记段（含 EOI 之后的数据），两者共同生成逐项 Preview（METADATA_MODEL §10.1）。
- sidecar 与主文件在同一个 ExifTool 命令中读取，在 Rust 中叠加。

### 6.2 编辑 → Plan → Preview

```text
UI intent (edits or preset_id + selection)
  → Planner (mm-core, 纯函数主体在 mm-domain):
      for each asset (并行):
        target  = FormatPolicy(asset)                  // Embedded | Sidecar(existing|new) | Unsupported
        changes = Rules.evaluate(snapshot)              // 条件基于原始快照
        writes  = FieldRegistry.map(changes, target)    // 具体 Tag 写入/删除
        checks  = charset / unsupported / protected / conflicts
  → Plan { id, version, entries[], summary, seed?, created_from } (不可变, 存于内存 + 摘要落库)
  → Preview 分页查询 / 排除 → Plan vN+1
  → Preflight: 指纹未变、可写、空间充足、备份位置可用
```

### 6.3 执行

详细状态机见 SAFETY_MODEL §4。概要：

```text
op_execute(plan, version, token)
  → Journal: create Operation (status=Running), 写入每个文件的 Planned 记录
  → 对每个文件（有界并发）:
      Lock (READ, share READ|DELETE) → 指纹 → Backup via lock handle (hash H0, verify)
      → ExifTool write temp from backup copy (-o) → Verify temp (V1–V5) → Journal ready (fsync)
      → Identity check (path File ID == lock File ID; bak 名不存在)
      → Commit (ReplaceFileW with registered bak / no-clobber rename) → Post-check → Done
      每一步前后写 Journal
  → Operation 完成摘要
```

### 6.4 Undo / Recovery

Undo = 生成"恢复 Plan"（每个文件：当前哈希是否等于该 Operation 的执行后哈希）→ Preview → 执行（恢复本身也备份当前状态）。
Recovery = 启动时扫描 Journal 中未终结的 Operation → 对每个文件检查磁盘实际状态 → 自动修复确定性中间态 → 其余交由用户选择。

---

## 7. ExifTool Adapter

### 7.1 进程启动

```text
program: A: <install>\resources\exiftool\exiftool.exe
         B: <install>\resources\exiftool\exiftool_files\perl.exe  (first arg: exiftool.pl)
args:    -config "" -charset filename=utf8 -stay_open True -@ -
env:     清空后仅设置 SystemRoot、TEMP/TMP(指向应用私有临时目录)
cwd:     %LOCALAPPDATA%\MoriMeta\run\exiftool-cwd （空目录，无 .ExifTool_config）
stdio:   stdin=pipe（UTF-8 argfile，由专用线程写入）, stdout/stderr=pipe（各由专用线程读取）
window:  CREATE_NO_WINDOW
job:     Job Object，JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE（MoriMeta 退出或崩溃时 ExifTool 随之终止，S1 R8）
```

依据：`-config ""` 必须为首参数 [F-08]；配置文件可从 cwd/HOME 自动执行代码 [F-07]；`PERL5OPT/PERL5LIB` 注入已复现 [F-31]；`-charset filename=` 须在 `-@` 之前 [F-11]；stderr 不并发消费会严重拖慢甚至阻塞 [F-38]。

启动后执行一次握手命令：`-ver`，校验版本等于构建时锁定的版本。

### 7.2 协议（单个 Session）

每条命令写入：

```text
<command args…>              // 每行一个参数，全部是普通行（ADR-10，不使用 #[CSTR]）
-echo4
{mm-end:<ID>:${status}}
-execute<ID>                 // ID：每条命令新生成的随机 64 位整数
```

- stdout 读取至 `{ready<ID>}`；stderr 读取至 `{mm-end:<ID>:<status>}` [F-09]。
- **只接受与当前 ID 相同的终止标记**；其他 ID 的标记一律视为数据（文件内容可以包含 `{ready1}` 之类的文本）。**不要求终止标记位于行首**：`-b` 输出没有结尾换行，ExifTool 的终止标记会直接跟在数据后面（S1 发现的两个原型缺陷）。
- stdin 由专用线程写入，调用方不会因 ExifTool 挂起而阻塞；stdout/stderr 由各自的线程持续读取（5,000 条错误的 stderr 洪泛测试通过）。
- 超时：基于文件大小与操作类型计算（读：基础 10 s + 每 MB 0.2 s；写：基础 30 s + 每 MB 0.5 s；S4 后校准）；超时 → 终止 → 重启 → 该文件失败（S1：挂起在 2 s 超时后被处理）。
- 进程意外退出：S1 中 5 ms 内检测到；连续 3 次启动失败 → 引擎不可用状态（UI 显示"ExifTool unavailable"）。
- 所有路径：绝对路径、正斜杠形式、经 `mm-fs` 规范化；拒绝含 `\r \n \0 |` 的路径；不以 `-`/`#` 开头（绝对路径天然满足）[F-32]。

### 7.3 ArgBuilder（唯一的参数来源）

`mm-exiftool` 只接受类型化请求，不接受字符串参数：

```rust
enum Request {
    Read  { files: Vec<AbsPath>, tags: TagSelection, mode: ReadMode },   // SCAN | FULL | HASH
    Write { source: Option<AbsPath>, output: TempPath, ops: Vec<TagOp> },
    CleanExport { source: AbsPath, output: NewPath, keep: KeepSpec },
}
enum TagOp { Set(TagRef, Value), Delete(TagRef), DeleteGroup(GroupRef), AddToList(TagRef, Value), RemoveFromList(TagRef, Value) }
```

- `TagRef` 只能由 FieldRegistry 或经过校验的 Advanced 白名单构造（类型系统保证），并对照永久禁止表（文件系统伪标签等，SECURITY_MODEL §4）。
- 值只出现在 `-GROUP:TAG=` 之后；永不使用 `<`（复制/重定向）语法承载用户数据。
- **值编码（ADR-10）：** 写入命令带 `-ex`；值中的 `& < > " '` 与 TAB/LF/CR 写成 XML 字符引用，值开头的空格写成 `&#32;`（exiftool.pl 会删除 `=` 后的一个空格）。NUL、其他 C0 控制字符、U+FFFE/U+FFFF 由编码器拒绝。不使用 `#[CSTR]`：它无法精确传递 `$` 与 `@`（S1）。
- **列表型标签与 `-tagsFromFile @` 同时使用时**，被赋值的标签必须从复制中排除（METADATA_MODEL §2.2）。
- 固定选项：写入不带 `-m` [F-36]；读取用 `-json -G1 -a -api StructFormat=JSONQ`（所有值带引号，S1），数值字段另取 `-n` 形式。
- 禁用选项：`-ee`、`-if`、`-p`、`-fileNUM`、`-api filter*`、`-geotag`、`-overwrite_original*`、`-tagsFromFile` 指向非计划内文件。

### 7.4 输出解析

- JSON（JSONQ）解析为 `RawTagSet { source_file, tags: Map<(Group1, TagName), RawValue> }`；以 `SourceFile` 与请求路径做精确匹配；每个请求的文件都必须有结果，缺失条目从 stderr 分类得到原因（T-02）。
- stderr 行分类：`Error:` / `Warning:` / `[minor]` / 已知消息模板 → `EngineDiagnostic { severity, code, file?, raw }`，再映射到用户可读错误（§10）。
- 未知消息保留原文（调试视图可见）并归类为 `Unknown`，不会被静默丢弃。

### 7.5 进程池

- 实现状态（2026-09-27，Phase 1b）：执行器已按 N 个 worker 并行（`mm-cli --workers N`，默认以逻辑核数 / 2 近似物理核数 / 2），每个 worker 一个 ExifTool Session；同步线程实现（尚未引入 tokio）；Journal 为单个 SQLite 连接加锁串行写入，每次写入仍在提交后才返回（I-8）；每卷 IO 许可按 §8.2 以 `GetDriveTypeW` 与 seek-penalty 查询判定介质，文件在整个事务期间持有其卷的许可。扫描与规划仍为单 Session。
- `ExifToolPool`：N 个 Session，默认 `N = clamp(physical_cores / 2, 1, 4)`，可在设置中调整；读写共用池，但写入任务优先。
- 每个 Session 独立 tokio 任务，串行处理命令；池负责分派、健康检查、重启与关闭（`-stay_open\nFalse`）。
- 应用退出：等待进行中的命令完成（最多数秒）后关闭；强制退出时直接 kill（安全，因为 ExifTool 从不直接写原文件）。

---

## 8. 并发与性能策略

### 8.1 原则

- 瓶颈通常是**存储 IO**，不是 CPU。并发按**卷**限制，而不是全局。
- 读取批量化（每命令多文件），写入逐文件（每命令一个文件，确保错误归属与事务边界清晰）。
- 所有长任务流式输出、有界队列、可取消。

### 8.2 调度

| 阶段 | 并发单位 | 默认上限 | 说明 |
|---|---|---|---|
| 枚举 | 每个导入根 1 个任务 | — | 边枚举边发出行 |
| 扫描 | ExifTool Session | N（≤4） | 每命令 100–200 文件（S4 校准） |
| 备份复制 + 哈希 | 每卷 IO 许可 | SSD 4 / HDD 1 / 网络盘 2 / 可移动介质 1 | 介质类型探测失败时按 HDD 处理 |
| ExifTool 写入 + 验证 | ExifTool Session | N | 与 IO 许可共同约束 |
| 提交（ReplaceFileW） | 每卷 | 与 IO 许可相同 | |

背压：执行管线各阶段之间使用有界通道（容量 = 2×N），防止备份阶段远超写入阶段导致临时空间暴涨。

### 8.3 规模策略

| 规模 | 策略 |
|---|---|
| 100 | 单批扫描即完成；Plan 同步生成；Preview 全量加载 |
| 1,000 | 分块扫描 + 流式更新；Plan 后台生成（进度条）；Preview 分页（每页 200 行）；聚合在后端计算 |
| 5,000 | 同上；前端只保留行摘要（约 1 KB/行 ≈ 5 MB）；Inspector 数据按需加载；Plan 详情（Tag 级）按需展开；执行期间 UI 仅接收合并后的进度与失败项 |

内存预算（初始目标，V-20 验证）：5,000 文件 Session 的 Rust 侧索引 < 200 MB；前端 < 300 MB。

### 8.4 已知数据点

读取吞吐与内容关系很大（S0/S1）：同一简单 JPEG ×500 约 715 files/s（Rust 会话，单进程）；含厂商 MakerNotes 的混合测试 JPEG 约 35 files/s。每个文件的完整事务（锁、备份、写临时文件、验证、提交）在 S2 原型中约 47 ms（小 JPEG，本地 NTFS）。真实大文件、HDD、NAS 的数字需在 S4 中测量 [V-06]。

---

## 9. 存储布局

```text
%LOCALAPPDATA%\MoriMeta\
├── db\morimeta.sqlite            History、Journal、Plan 摘要、设置（WAL）
├── backups\<operation-id>\       manifest.jsonl（只追加）+ manifest.json（快照）+ plan.json + 00000001.<ext> …（保留扩展名：ExifTool 以备份为写入源；可配置到其他位置）
├── presets\*.json                用户 Preset（schema_version）
├── config\settings.json          设置（config_version）；迁移前自动保留 .bak
├── logs\morimeta-YYYYMMDD.log    滚动日志（脱敏）
├── cache\previews\               Inspector 预览图缓存（可清理，v1 缩略图）
└── run\
    ├── exiftool-cwd\             ExifTool 工作目录（空）
    ├── tmp\                      ExifTool 的 TEMP
    └── instance.lock             单实例锁
```

照片目录中只会**短暂**出现：`<stem>.mmtmp-<rand>.<ext>`（ExifTool 输出的临时文件）与 `<stem>.mmbak-<rand>.<ext>`（ReplaceFileW 的同卷备份名），`<rand>` ≥ 64 位随机数。二者都在 Journal 中登记；提交前核对 bak 名处不存在文件（ReplaceFileW 会静默覆盖已存在的 bak 名文件，S2）；正常流程结束即删除，崩溃后由 Recovery 核对哈希后清理。

单实例：应用只允许一个实例（两个实例同时写同一批文件是数据风险）；第二个实例把拖入的文件转交给第一个实例。

---

## 10. 错误模型

```rust
struct UserFacingError {
    code: ErrorCode,          // 稳定枚举，用于文档与 i18n
    file: Option<AssetRef>,
    reason_key: I18nKey,      // "文件是只读的"
    detail: Option<String>,   // 原始诊断（仅在详情中展开）
    others_safe: bool,        // 其他文件是否受影响
    suggestion_key: I18nKey,  // "在资源管理器中取消只读属性后重试"
    retryable: bool,
}
```

错误分类（节选）：`ReadOnlyAttribute`、`AccessDenied`、`SharingViolation`（被其他程序占用）、`FileChangedSincePreview`、`DiskFull`、`PathTooLong`、`CloudPlaceholder`、`UnsupportedFormat`、`EngineFormatError`（ExifTool 拒绝写：文件结构问题）、`EngineMinorError`、`VerificationFailed`、`EngineTimeout`、`EngineUnavailable`、`CharsetLoss`、`Conflict`（Undo）、`Internal`。

原则：**永远不显示"ExifTool exited with code 1"作为唯一信息**；未知错误显示通用说明 + 可复制的诊断详情。

---

## 11. 配置、Schema 与迁移

| 数据 | 版本字段 | 迁移策略 |
|---|---|---|
| settings.json | `config_version` | 启动时逐版本迁移；迁移前备份；解析失败 → 以默认值启动并提示，原文件保留为 `.corrupt-<ts>` |
| presets/*.json | `schema_version` | 读取时升级到当前版本（内存中），保存时写新版本；未知更高版本 → 只读并提示需要升级应用 |
| SQLite | `PRAGMA user_version` + 迁移脚本 | 只向前迁移；迁移前复制数据库文件；**有未完成 Operation 时先完成 Recovery 再迁移** |
| 备份 manifest | `manifest_version` | 永远保持可读旧版本（备份可能跨多个应用版本存在） |
| Plan | Operation 开始时持久化**可执行内容**（每个文件的目标、指纹、TagOp、期望值），用于崩溃或取消后的"继续"（SAFETY_MODEL §9）；未执行的 Plan 只在内存中 | 与写入它的应用版本绑定：版本变化时不"继续"，只允许撤销或重新生成 Plan |

降级：检测到数据库版本高于应用 → 拒绝写操作，提示安装新版本；不自动降级数据。

---

## 12. 日志

- `tracing` + 滚动文件（7 天或 50 MB 上限）。
- 默认级别 info；**默认不记录**元数据值、GPS、完整路径；文件以 `asset#<n>` 与扩展名表示，路径以卷 + 哈希后的目录表示。
- 调试日志（设置中开启，有明确隐私提示，24 小时后自动关闭）：记录 ExifTool 命令（值被截断）与计时。
- "导出日志"时再次展示将包含的内容并允许选择是否包含路径。

---

## 13. 测试架构（概要）

详见 DEVELOPMENT_PLAN §5。架构上的支撑：

- `mm-domain` 纯函数 → 单元测试 + 属性测试（时间运算、规则合成、模板、聚合、Diff）。
- `mm-exiftool` → 对真实 ExifTool 的协议测试 + 模糊测试（值编码、文件名）。
- `mm-core` → 通过 `mm-testkit` 的故障注入点（每个 Journal 状态转换前后可触发 panic / 进程 kill / 模拟 IO 错误）运行 5,000 文件的崩溃恢复测试。
- 语料：raw.pixls.us CC0 样本 [F-70] + 自有样本（不入库，CI 从私有存储拉取）。

---

## 14. 仓库目录树（建议）

```text
MoriMeta/
├── apps/desktop/
│   ├── src/                     React 前端（§4.2）
│   ├── src-tauri/
│   │   ├── src/                 commands.rs, events.rs, dragdrop.rs, dialogs.rs
│   │   ├── capabilities/        default.json（仅自定义命令）
│   │   ├── resources/exiftool/  构建时注入（不入库，由脚本下载 + 校验）
│   │   └── tauri.conf.json
│   ├── index.html
│   └── package.json
├── crates/                      §4.1
├── tools/
│   ├── fetch-exiftool/          下载、校验 SHA-256、重命名、生成完整性清单
│   ├── corpus/                  测试语料下载与索引
│   └── licenses/                生成 THIRD_PARTY_NOTICES
├── tests/
│   ├── compat-lab/              兼容性实验室的手工测试脚本与记录模板
│   └── e2e/
├── research/                    Phase 0 可复现实验与原型（不属于产品代码；结果见 docs/SPIKE_REPORT.md）
├── docs/                        本目录
├── .github/                     workflows、ISSUE_TEMPLATE、PULL_REQUEST_TEMPLATE、dependabot
├── Cargo.toml                   workspace
├── deny.toml                    cargo-deny（许可证、漏洞、依赖方向）
├── README.md  LICENSE  CONTRIBUTING.md  SECURITY.md  PRIVACY.md  CHANGELOG.md
└── THIRD_PARTY_NOTICES.md       生成
```

---

## 15. 跨平台（后续）

- 核心 crate 已按平台隔离：`mm-fs` 提供 Windows 实现（ReplaceFileW）与 POSIX 实现（`rename(2)` + `fsync` 目录、`renameat2(RENAME_NOREPLACE)`/`linkat` 实现不覆盖创建）。
- macOS/Linux 上 ExifTool 为 Perl 脚本：打包官方 Perl 发行版 + 使用系统或内嵌 Perl（需单独评估）；macOS 特有伪标签与 CVE-2026-3102 类问题需额外防护 [F-06]。
- macOS 公证、Linux 包格式在 RELEASE_PLAN 后续阶段处理。
