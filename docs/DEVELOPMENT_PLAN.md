# MoriMeta — Development Plan

> **Version:** 0.3 · **Status:** 已批准（2026-09-30，按 [DECISIONS](DECISIONS.md) 修订）· **Date:** 2026-09-26
> 本文定义从架构确认到 1.0 公开发布的路径、每阶段的退出条件、测试策略、风险与待决策事项。
> `[V-xx]` 见 [RESEARCH_NOTES](RESEARCH_NOTES.md) 验证登记表；Spike 结果见 [SPIKE_REPORT](SPIKE_REPORT.md)。

---

## 1. 开发原则

1. **先验证，后实现**：Spike 结论会修改文档；未验证的结论不写成实施规格。
2. **先有安全测试，再有功能**：故障注入与不变量检查器在第一个产品写操作功能之前完成。
3. **垂直切片**：尽早打通"导入 → 预览 → 执行 → 撤销"链路（仅 JPEG + 一个字段），再横向扩展。
4. **核心先于界面**：所有写逻辑先经 `mm-cli` 可测，再接 UI。
5. **不依赖待决事项的部分先做**：许可证、隐私范围、签名路线、时区修正、ExifTool 调用方式未决定前，只实现与之无关的基础部分（§4 Phase 1）。
6. **每个 MVP 功能的完成定义**：单元/集成测试、故障注入覆盖（若涉及写入）、i18n 文案、键盘可用、错误态、文档。

---

## 2. 阶段总览

```text
Phase 0  Spikes & 验证        S0–S7；关闭 MVP 阻塞的 V-xx，更新文档（进行中）
Phase 1  Foundation          mm-domain / mm-exiftool / mm-fs / mm-store / mm-testkit / mm-cli（无 UI）
Phase 2  Vertical Slice      JPEG + Creator 端到端；UI 以设计方向选定为前提
Phase 3  MVP Features        时间工具、GPS、NEF sidecar、隐私（按 D-15）、Rules/Presets、History、Settings、i18n、a11y
Phase 4  Hardening & Release 规模故障注入、兼容性实验室、性能、安全审查、打包、签名（按 D-2）、更新、文档
         → Private Alpha → Public Preview/Beta → 1.0
```

规模估计（单名熟练开发者、全职，仅供排序参考，S4 后重估）：Phase 0 剩余 ≈ 2–3 周（主要是需要人工参与的 S3 第三方部分、S4、S5、S6）；Phase 1 ≈ 5–7 周；Phase 2 ≈ 4–5 周；Phase 3 ≈ 10–14 周；Phase 4 ≈ 6–8 周。

---

## 3. Phase 0 — Spikes

每个 Spike 产出：可复现的原型与脚本（`research/`，不属于产品代码）、测量数据（`research/results/`）、结论写回文档。

| Spike | 目标 | 退出条件 | 状态（2026-09-26） |
|---|---|---|---|
| **S0 版本与复现** | 锁定 ExifTool 版本；用脚本在锁定版本上复现 RESEARCH_NOTES 引用的本机实验；验证直接调用 Perl | 引用的实验结果都能由脚本复现，或更正；直接调用 Perl 的等价性有结论 | **完成**：13.59；11/12 复现，F-28 更正，F-38/F-11 修订；直接调用 Perl 等价（是否采用待 D-17） |
| **S1 协议** | stay_open 会话、编码、stderr 并发、超时/崩溃重启、注入 | 10 万次随机值往返 0 差异；注入用例全部被安全编码或拒绝；崩溃与挂起在一个超时周期内处理 | **完成**：100,000/100,000（两种调用方式）；注入、伪造终止标记、崩溃、挂起、stderr 洪泛、孤儿进程均通过 |
| **S2 文件事务** | 锁句柄与 ExifTool/ReplaceFileW 共存；提交方式对比；故障注入；多种文件系统 | 每种环境、每个注入点记录路径、内容、恢复结果；据此写定 SAFETY_MODEL §0 | **部分完成**：NTFS 与 SMB 回环完成（0 违例）；exFAT、云同步目录、断电未做 |
| **S3 字段与 sidecar** | 显式/MWG 映射、IPTC 编码与长度、时间字段、sidecar 保留、MakerNotes 序列号、C2PA；第三方软件显示 | 注册表 v1 冻结；兼容性结果表发布到 `tests/compat-lab/` | **ExifTool 侧完成**；第三方软件（LR Classic 15、ACR/Bridge、Capture One、NX Studio、darktable、digiKam、Photo Mechanic）未做，需要人工与授权（D-13） |
| **S4 性能** | 真实语料在 NVMe/SATA/HDD/NAS 上的扫描与写入吞吐；worker 数；Journal 开销；持久化 Plan 的体积；内存 | PRODUCT_SPEC §7.1 目标被确认或修订 | 未开始（需要真实语料，D-13） |
| **S5 UI 技术验证** | Tauri 2.11 + React：5,000 行虚拟表格、排序筛选、中文 IME、Narrator/NVDA、IPC、类型生成；capability 中无插件权限 | ADR-01 的退出条件满足，否则启动 ADR-01 复审 | **自动部分完成**（Tauri 2.12，`research/spikes/s5-ui`）：排序/筛选 < 50 ms、IPC 5,000×30 约 40 ms、Channel 3 万事件/秒、插件命令被 ACL 拒绝；中端笔记本帧率、中文 IME、Narrator/NVDA、缩放与 Win10 为人工项，未做。不涉及产品视觉设计 |
| **S6 打包、更新、许可证** | NSIS per-user 与 `exiftool_files`；升级/卸载；Defender；后端 updater 门禁与安装时退出流程；许可证清单；签名渠道条件核对 | 一次完整演练（不做公开发布、不申请签名、不购买证书） | 未开始 |
| **S7 隐私导出** | 按 D-15 (c) 的方向验证 Clean Export（范围未批准） | 预览移除集合 = 实际移除集合；输出通过段级与标签级检查；无法证明干净的文件被阻止 | **完成（JPEG，53 个源）**；真实 C2PA、HDR gain map、更多相机直出 JPEG 待补充 |

---

## 4. Phase 1–4 里程碑

### Phase 1 — Foundation（无 UI）

不依赖待决事项、可以先做的部分（**Phase 1a，已开始**）：

- `mm-exiftool`：会话（S1 协议）、编码器（ADR-10）、Job Object、超时/重启、JSONQ 解析；调用方式 A/B 均支持，由配置选择（D-17 不阻塞）。
- `mm-domain`：MVP 四个时间工具的纯函数与属性测试（跨日/跨年/闰年/亚秒）；值校验（可接受字符）。
- `mm-fs`：File ID、锁句柄、`ReplaceFileW`、不覆盖重命名、刷盘、哈希复制。

Phase 1a 当前状态（2026-09-26，根目录 Cargo workspace `crates/`，未设许可证字段，等待 D-1）：

| Crate | 内容 | 测试 |
|---|---|---|
| `mm-exiftool` | `Line`/`Command`（只能经校验构造的参数行；`-ex` 值编码；路径与标签名校验；渲染层再断言）、`Session`（随机 64 位终止 ID、三线程 IO、Job Object、超时/崩溃处理）、调用方式 A/B | 9 个单元测试 + 3 个集成测试（对锁定的 ExifTool 13.59：两种调用方式各 5,000 个随机值往返、注入与伪造终止标记、外部终止/超时/drop） |
| `mm-fs` | File ID、锁句柄、探测（只读/reparse/云占位符/硬链接）、`ReplaceFileW`、不覆盖重命名、`ensure_absent`、刷盘、哈希复制、随机 64 位名 | 7 个测试（锁与替换共存、失败不改磁盘、硬链接/只读探测等） |
| `mm-domain` | MVP 四个时间工具（Absolute、Shift、Sequence、Preserve Relative Timing）、EXIF/XMP 时间解析与格式化、自然排序、文本值校验 | 14 个测试（跨日/跨年/闰日/范围溢出、v0.1 §12.3 与 §12.6 示例、亚秒与偏移规则） |

依赖（含传递依赖）均为 MIT/Apache-2.0 等宽松许可（blake3、constant_time_eq 另有 CC0 选项，r-efi 另有 LGPL 选项，均可选 MIT/Apache），与 D-1 的两个候选都兼容。`cargo clippy -D warnings`、`cargo fmt --check` 通过。

之后（**Phase 1b**）：FieldRegistry v1（等 S3 第三方部分）、Plan/Diff/聚合、`mm-store`（schema v1、Journal 状态机、持久化 Plan、备份库、manifest）、`mm-testkit`（故障注入点、语料、不变量检查器）、`mm-cli`（`scan`、`plan`、`apply`、`undo`、`recover`、`fsck`）。

**退出条件：** `mm-cli` 可对 1,000 个 JPEG 执行 Creator 修改与 Undo；故障注入矩阵（SAFETY_MODEL §12）在 JPEG 就地写入路径上 0 违例。

### Phase 2 — Vertical Slice

- 核心链路：Handle 模型、导入、Plan 与 Preview 数据接口、执行进度、完成摘要、History 与 Undo、启动时 Recovery。
- **产品 UI 的前提（设计简报 §21–24）：** 先完成三个视觉方向的探索并由用户选定，再展开选定方向的完整界面与 DESIGN.md。在选定之前，Phase 2 只做到 `mm-cli` 与后端接口，不实现产品界面；S5 的技术验证原型不作为产品 UI。
- **退出条件：** 场景 D（批量版权）在 5,000 个 JPEG 上端到端完成并可撤销；UI 不冻结（UI 部分以设计方向选定为前提）。

### Phase 3 — MVP Features

按依赖顺序：

1. NEF + sidecar（读取叠加、新建/更新 sidecar、两层 Inspector、RAW 提示文案）
2. 时间工具：Absolute、Shift、Sequence、Preserve Relative Timing（v0.1 §33）；时区修正取决于 D-18
3. GPS 设置与移除（RAW 的"移除 GPS"整体 Unsupported）
4. Rules 与 Presets（简化版条件、模板变量、导出/导入、内置通用 Preset）
5. 隐私功能：按 D-15 的决定实现（就地移除 GPS 各选项共有；Clean Export 与/或就地移除序列号）
6. 导入环境检测（只读、云占位符、可移动介质、链接、C2PA）与对应 UX
7. Settings、备份管理与保留策略、First Launch
8. i18n（en、zh-CN 全覆盖）、键盘与可访问性、错误态全集（设计简报 §17）

**退出条件：** PRODUCT_SPEC §3.2 场景 A–E 全部可完成；所有 MVP 功能满足完成定义。

### Phase 4 — Hardening & Release

- 5,000 文件规模的故障注入与断电模拟（SAFETY_MODEL §12 全部）。
- 兼容性实验室全量复测（S3 矩阵 × 最终实现）；S7 扩充真实样本。
- 性能达标（§7.1）；内存预算。
- 安全审查：威胁清单逐项验证（SECURITY_MODEL §12）；依赖审计；恶意元数据/文件名语料。
- 打包、签名（按 D-2）、更新、卸载；THIRD_PARTY_NOTICES；公开文档。
- **Private Alpha**（5–10 名摄影师，使用副本数据）→ **Public Preview/Beta**（按 D-2 的签名组合）→ **1.0**。

---

## 5. 测试策略

### 5.1 测试层级

| 层级 | 对象 | 工具 |
|---|---|---|
| 单元/属性 | `mm-domain` 全部纯函数 | `cargo test`（属性测试库待选，须为宽松许可） |
| 协议/模糊 | `mm-exiftool` 编码与解析；文件名与值 | S1 的 10 万次往返迁入集成测试；模糊测试 |
| 集成 | 真实 ExifTool + 临时目录语料：读、写、验证、提交、Undo | `mm-testkit` |
| 故障注入 | 每个 Journal 转换前后注入进程终止/IO 错误/磁盘满；随机时刻终止 | `mm-testkit`（沿用 S2 的子进程驱动方式） |
| 断电 | 虚拟机硬重置 | 夜间任务（需要虚拟机，D-13） |
| 前端 | 组件与交互 | Vitest + Testing Library；Playwright（以 S5 结论为准） |
| 兼容性实验室 | 第三方软件对写入结果的读取 | 手工脚本 + 记录模板（`tests/compat-lab/`） |
| 性能 | 扫描/计划/执行基准 | 语料基准脚本，结果按版本记录 |

### 5.2 测试语料

- 来源：raw.pixls.us（CC0 [F-70]，已用 3 个 NEF，见 `research/corpus.lock.json`）、ExifTool 测试图片、自拍样本（需人工提供，D-13）、合成的恶意/畸形样本（S1/S7 已有生成脚本）。
- 语料不入 Git 仓库；公开仓库只包含清单（URL + SHA-256）与下载脚本。

### 5.3 兼容性实验室矩阵（MVP，S3 第三方部分，需人工）

| 软件 | 检查内容 |
|---|---|
| Lightroom Classic 15.x | NEF sidecar 字段可见性、`.acr` 共存、Read Metadata from Files、JPEG 内嵌字段、中文、OffsetTimeOriginal |
| Adobe Bridge / ACR | 同上 |
| Capture One | sidecar 读取设置与行为 |
| Nikon NX Studio | 是否读取 XMP sidecar；ExifTool 写过的 JPEG 显示；MakerNotes 序列号置空后的镜头信息 |
| darktable | `.xmp` 导入行为 |
| digiKam | sidecar 命名设置与读取 |
| Windows 资源管理器 / Photos | 已部分完成（S3：UTF-8 EXIF 作者/版权显示正确；拍摄时间忽略偏移）；Win10 22H2 未测 |
| 浏览器（Chrome/Edge/Firefox） | Clean Export 结果的方向与色彩 |

### 5.4 CI

- PR：`fmt`、`clippy -D warnings`、单元/属性/集成测试（小语料）、前端 lint/test、`cargo-deny`（许可证白名单只含宽松许可，直到 D-1 决定）、`npm audit`、依赖方向检查、capability 差异检查。
- Nightly：大语料集成、故障注入矩阵、模糊测试时间片、性能基准趋势。
- Release：签名（按 D-2）、SBOM、THIRD_PARTY_NOTICES、校验和、更新清单。

---

## 6. 风险登记

| # | 风险 | 可能性 | 影响 | 应对 |
|---|---|---|---|---|
| K-1 | 第三方软件不按预期读取最小 NEF sidecar | 中 | 高 | S3 第三方部分；必要时 sidecar 包含同步字段 |
| K-2 | ExifTool 升级引入写入缺陷 | 中 | 高 | 版本锁定 + 语料回归 + V2/V3/V4 验证 |
| K-3 | WebView2 无法满足表格/IME/无障碍 | 低-中 | 中 | S5；核心与 UI 解耦（ADR-02） |
| K-4 | 性能不达标（验证与备份开销；含 MakerNotes 的文件读取慢） | 中 | 中 | S4；并行与按卷调度；不以牺牲验证为代价 |
| K-5 | 签名渠道：SignPath 批准的时间与条件（CC0 launcher、单人角色）、OV 费用与身份验证 | 中 | 中 | D-2 选定组合与后备；D-16、D-17 |
| K-6 | 许可证合规（Strawberry Perl 组件、GPL 源码提供义务） | 中 | 高 | S6 + 法律意见 |
| K-7 | 范围蔓延 | 高 | 中 | PRODUCT_SPEC §8；新增需求先写入 Roadmap 并评估安全影响 |
| K-8 | ExifTool 安全更新义务（2026 年已四次） | 中 | 中 | SLA 与自动化回归 |
| K-9 | Lightroom 等覆盖 sidecar 引发误解 | 中 | 中 | UI/文档说明；FAQ |
| K-10 | 未验证的环境（exFAT、云同步、断电）上的行为 | 中 | 高 | SAFETY_MODEL §0 如实列出；默认限制（可移动介质禁止就地写入）；补做测试 |

---

## 7. v1 / v2 Roadmap（依赖关系）

| 版本 | 功能 | 前置条件 |
|---|---|---|
| v1.1 | 更多描述字段、Metadata JSON/CSV 导出、缩略图列 | 注册表 v2；缩略图缓存隐私设计 |
| v1.2 | DNG 写入、CR2/CR3/ARW/RAF 的 sidecar 写入 | 兼容性实验室扩展 |
| v1.3 | Range/Random 时间工具、仅改偏移、双机参照同步、`{index}`、嵌套条件 | — |
| v1.4 | GPX 同步 | 独立的安全设计 |
| v1.5 | D-15 未纳入 MVP 的隐私形式、手动镜头信息、清除 sidecar 中的 GPS 覆盖值 | D-15 |
| v1.6 | HEIC/AVIF/PNG/WebP 写入 | V-08 |
| v1.x | NEF 直接写入（高级） | 机型级兼容认证 |
| v2 | Rename Engine、Advanced 写入、Folder Watch、CLI/Headless、Preset 分享、macOS/Linux、只读扫描进程沙箱化 | 各自设计文档 |

---

## 8. 需要人工决策的事项

> 2026-09-30：全部事项已决定，见 [DECISIONS](DECISIONS.md) §2；下表保留原选项作为背景。

| # | 决策 | 状态与选项 | 建议 / 说明 | 影响 |
|---|---|---|---|---|
| D-1 | **许可证** | **已决定（2026-09-27）：GPL-3.0-or-later**；公开仓库 Morii9961/MoriMeta | 比较见 RELEASE_PLAN §7.1；不因签名渠道而替用户选择（二者都满足 OSI 要求） | 贡献流程、代码复用、衍生版本 |
| D-2 | **Windows 签名路线** | 候选组合见 RELEASE_PLAN §4.2 | 需要用户分别决定：是否接受发布者显示为 SignPath Foundation；首发是否签名、用哪种方式；后备方案及切换条件 | 发布时间、成本、SmartScreen |
| D-3 | 发布者身份 | 个人 / 注册主体 | 取决于 D-2 | 证书申请 |
| D-4 | 更新检查默认值 | 默认开 / 默认关 / 首次启动询问 | 首次启动询问 | 隐私声明；安装页 |
| D-5 | 可移动介质就地写入 | 默认禁止（可解除）/ 允许并警告 | 默认禁止（exFAT 未验证） | 部分用户习惯 |
| D-6 | 文件修改时间默认 | 更新 / 保留 | 更新（SAFETY_MODEL §8.9） | 按修改时间排序的习惯 |
| D-7 | 备份保留默认 | 30 天 + 10% 容量 / 其他 | 30 天 + 10% | 磁盘占用 |
| D-8 | DNG 写入是否进入 MVP | MVP / v1.2 | v1.2 | 范围 |
| D-9 | Session 持久化 | 不持久化 / 持久化缓存 | MVP 不持久化 | 重复扫描耗时 |
| D-10 | ExifTool Perl 运行时 | 官方 Windows 包 / 自建 | 官方包（R-3 残余风险） | 维护成本 |
| D-11 | 最低 Windows 版本与架构 | Win10 22H2 + Win11，x64；ARM64 仿真 | x64 首发 | 测试矩阵 |
| D-12 | 文档语言 | 中文为主 / 英文为主 / 双语 | 设计文档中文；公开文档双语，英文为准 | 翻译工作量 |
| D-13 | 验证资源 | LR Classic、Capture One、NX Studio 等授权与测试机；Nikon Z 相机直出 JPEG/NEF 样本；带真实 C2PA 的样本；可做断电测试的虚拟机；可安全使用的云同步测试目录；exFAT 介质 | 需要用户提供或授权 | S3 第三方部分、S4、S7 扩充、断电与环境测试 |
| D-14 | 团队规模与时间预期 | — | 用于重估 §2 | 计划可信度 |
| **D-15** | **隐私功能的 MVP 范围** | (a) 就地清理 GPS + 序列号等；(b) (a) + Clean Export；(c) Clean Export + 就地移除 GPS | 当前按 (c) 做验证（S7 通过），**未批准**。取舍见 PRODUCT_SPEC §6.8.1 | MVP 范围、Preset、UI |
| D-16 | SignPath 团队角色 | 单人能否兼任 Authors/Reviewers/Approvers | 条款未说明，需向 SignPath 书面确认（仅在 D-2 采用 SignPath 时需要） | 申请可行性 |
| D-17 | ExifTool 调用方式 | A 官方 launcher（CC0）/ B 直接调用 Perl | 技术上二者等价（S0）；取决于 D-2 对 CC0 的要求 | 打包内容 |
| D-18 | 时区修正是否进入 MVP | 进入 / v1 | 新增建议，见 PRODUCT_SPEC §6.5.2 | 时间工具 UI 与验证量 |

---

## 9. 进入产品实现前的确认清单

> 2026-09-30：已完成（[DECISIONS](DECISIONS.md) §1）。

- [ ] 审阅 PRODUCT_SPEC §11 的变更（尤其"收窄已列 MVP"与"待决"两类）。
- [ ] 确认 ARCHITECTURE ADR-01/02/05/08/10/11。
- [ ] 确认 SAFETY_MODEL §0 前提与 §2 对外措辞。
- [ ] 决定 D-15、D-18；D-1、D-2 可在 Phase 4 之前决定，但越早越好（签名渠道可能需要较长时间）。
- [ ] 提供 D-13 所列资源，或确认哪些环境不在首个版本的支持范围内。
- [ ] 设计：按设计简报 §21 完成三个视觉方向并选定一个（产品 UI 的前提）。
