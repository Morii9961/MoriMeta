# MoriMeta — Phase 1 Report（JPEG 与 NEF sidecar：Creator / Copyright / 拍摄时间 / GPS）

> **Status:** 进行中 · 2026-09-27
> 记录 Phase 1b 的实现范围、验证方式与已知缺口。安全措辞仍以 SAFETY_MODEL §0 的前提为准。

## 1. 范围

不依赖设计决策的核心链路，经 `mm-cli` 端到端可测：

```text
scan → plan-creator → apply（逐文件事务）→ fsck → plan-undo → apply（撤销）
                         └─ 崩溃 → recover → resume
```

| Crate | 新增内容 |
|---|---|
| `mm-domain` | `plan`（Plan / PlanEntry / TagOp / Expect / EntryAction，可序列化并持久化）、`snapshot`、`creator`（读取调和与写入规划；注册表 v0，暂定）、`cp1252` |
| `mm-store` | SQLite Journal（WAL、`synchronous=FULL`、schema v1、拒绝更新版本的数据库）；`backups/<op>/` 下的 `manifest.jsonl`（只追加）、`manifest.json`（快照）、`plan.json`；数据库丢失时 `rebuild-journal` 据此重建 |
| `mm-core` | `engine`（会话自动重启、按大小的超时）、`planner`（预检、指纹、规划）、`verify`（V1–V5 + "预览后被修改"检查）、`executor`（SAFETY_MODEL §4.1 事务、10 个故障点、ReplaceFileW 失败处理、熔断、resume）、`recovery`（§10 判定表）、`undo`、`fsck` |
| `mm-cli` | `scan`、`plan-creator`、`apply`、`recover`、`resume`、`plan-undo`、`history`、`show`、`fsck`；单实例锁；`--crash-at` 需 `MM_FAULT_INJECTION=1` |

Creator 写入规则（METADATA_MODEL §2.2、§6）：EXIF `IFD0:Artist`（`; ` 连接）、XMP `dc:creator`；文件已有 IPTC 时写 `By-line`，Latin 字符集无法表示或超过 32 字节时**整个文件的该字段 Blocked**；已有 `XMP-tiff:Artist` 一并更新；写 IPTC 且已有摘要时更新 `IPTCDigest`。ExifTool 以**已核对哈希的备份副本**为写入源。

Copyright 写入规则（METADATA_MODEL §6、§8，PRODUCT_SPEC §6.7）：EXIF `IFD0:Copyright`、XMP `dc:rights` 默认语言；文件已有 IPTC 时写 `CopyrightNotice`（Latin 字符集不能表示或超过 128 字节 → 该文件的该字段 Blocked）；已有 `XMP-tiff:Copyright` 一并更新；读取优先级 XMP 默认语言 > EXIF > IPTC。实测（ExifTool 13.59，2026-09-27）：不带语言代码写 `XMP-dc:Rights` 会**删除其他语言的条目**，因此写入时明确使用 `XMP-dc:Rights-x-default`；读回键名不带后缀，V3 按此对应，其他语言条目若被改动仍判为附带变化（单元测试覆盖）。执行时“预览后字段值未变”的核对改为按字段分派，未知字段一律拒绝写入。

拍摄时间写入规则（METADATA_MODEL §5）：四种工具（Absolute、Shift、Sequence、Preserve Relative Timing）以整个选择为单位规划（Sequence 需排序、Preserve 需锚点），`mm-cli plan-time`。写 `ExifIFD:DateTimeOriginal`，默认同时写 `ExifIFD:CreateDate`（`--no-digitized` 关闭）；已存在时更新 `IFD0:DateTimeOriginal`、`XMP-exif:DateTimeOriginal`、`XMP-photoshop:DateCreated`、`XMP-xmp:CreateDate`、IPTC `DateCreated`/`TimeCreated`，不新建。各位置保持自身形态：仅日期的值仍只写日期，各自的偏移不变（MVP 工具不改偏移，D-18），亚秒由 Shift/Preserve 保留、由 Absolute/Sequence 删除。原先与拍摄时间不一致的位置也改为新时间，Preview 为每个这样的位置给出说明。实测（ExifTool 13.59，2026-09-27）：各格式按写入值读回；但**不带偏移写 `IPTC:TimeCreated` 时 ExifTool 填入本机时区**（本机 `+08:00`），因此 IPTC 时间只以其已有偏移写入，无法识别时该文件 Blocked。

GPS 写入规则（METADATA_MODEL §7）：设置写 GPS 目录的纬度/经度及其参考（N/S、E/W），给出海拔时写海拔及其参考，未给出时删除旧海拔（旧海拔属于另一个位置，Preview 说明）；已有 XMP GPS 一并更新；时间戳不改。移除删除整个 GPS 目录与已有 XMP GPS 标签（含时间戳），不动地名。GPS 值在规划与验证时按数值读取（`-GPS:all#` 等须位于 `-all` 之前才生效，实测），V2 以数值容差比较（坐标 1e-7°，海拔 1 mm）；V3 允许 ExifTool 新建 GPS 目录时自动加入的 `GPSVersionID`，删除整组时只放行消失的标签。**验证拦截的真实问题**：海拔参考写 `1` 时 ExifTool 13.59 写成 0（海平面以上），单独写负海拔会丢失符号——端到端测试中 V2 拒绝了该文件（原文件未动）；改为按名称写 `Below Sea Level` / `Above Sea Level`，读回数值 1/0。

NEF sidecar（SAFETY_MODEL §3、§4.2、§4.3）：FormatPolicy——JPEG 就地写入，NEF/NRW 写 `<stem>.xmp`，单独选中的 `.xmp` 只写 XMP 标签，其余只读；配对按 §3.1。读取时 sidecar 的值优先于 RAW 自身的值（与 Lightroom/Camera Raw 一致）；写入只用各字段的 W-S 标签（`dc:creator`、`dc:rights` 默认语言、`XMP-exif`/`photoshop`/`xmp` 时间、XMP GPS）。RAW 自身已有的 Creator/Copyright 不能经 sidecar 清除（Blocked），RAW 内 GPS 不能移除（Unsupported）。新建 sidecar 由 ExifTool 从空写出（实测：以 NEF 为源 `-o x.xmp` 会复制镜头、裁切、EXIF 时间等大量标签，违反 §3.1“只包含 MoriMeta 写入的字段”），对空源做 V1–V3、V5 验证后以不覆盖重命名提交（Journal 角色 `create`，撤销即移入备份库）；更新已有 sidecar 走 §4.1 事务。XMP 文件无图像数据，V4 仅对图像文件适用。sidecar 目标的“预览后未变”核对由 sidecar 指纹承担（其 Preview 值可能来自 RAW）。

## 2. 验证

`cargo test --workspace --release`：53 个测试全部通过（2026-09-27）。端到端测试 `crates/mm-cli/tests/e2e.rs` 使用锁定的 ExifTool 13.59 与 8 个 ExifTool 样本 JPEG（Writer、Nikon、Canon、XMP、Sony、Olympus、Pentax、GPS）：

| 测试 | 内容 | 结果 |
|---|---|---|
| 执行→撤销 | 中文作者名写入；Latin IPTC 的 GPS.jpg 被 Blocked 且不被触碰；fsck 干净；同值重新规划为 NoChange；撤销后逐字节一致 | 通过 |
| 逐步崩溃 | 故障点 1–10 × 文件序号 0、3，共 20 例：每例检查恢复前原内容仍在（路径/bak/备份库）→ 恢复待决时新写入被拒绝 → `recover` → 路径只为 H0 或 H1、无残留、fsck 干净 → `resume` 完成 → 撤销后逐字节一致 | 20/20 通过 |
| 随机终止 | 随机时刻终止 `mm-cli apply`，其后同上（默认 12 次，`MM_E2E_KILLS` 可调） | 12/12 通过 |
| 防护 | 只读、硬链接 → Blocked；预览后被改写 → Conflict 且不写入；执行时被独占打开 → Skipped | 通过 |
| 拍摄时间（四种 MVP 工具） | Shift：无时间的文件 Blocked，其余各移 1 小时；XMP 仅日期值保持仅日期、IPTC 日期随新时间、`IFD0:ModifyDate` 不变；Absolute：含无时间的文件；Sequence：按自然文件名每分钟一张；Preserve：锚点得新时间、其余移动相同量；带亚秒/偏移/XMP/IPTC 的文件：Shift 保留亚秒与各处偏移，Absolute 去掉亚秒、保留偏移；Shift 中途崩溃后恢复与继续；全部撤销逐字节一致 | 通过（3 个端到端测试） |
| GPS 设置与移除 | 8 个夹具设为 `35.6812345,139.7671234,40.5`：数值按容差核对，GPS.jpg 的时间戳与 MapDatum 保留；移除：只有带 GPS 的文件被写，写后无任何 GPS 标签；已有 XMP GPS 一并更新/移除；南纬、西经与海平面以下保持符号；全部撤销逐字节一致 | 通过（2 个端到端测试） |
| NEF + XMP sidecar | NEF 只写其 sidecar：新建的 sidecar 只含写入字段；更新保留其他属性；撤销新建 = 移入备份库，再撤销则重建；拍摄时间/GPS 写入 sidecar，再次规划以 sidecar 的值为起点；配对：已有 `.XMP` 大小写沿用、darktable `.NEF.xmp` 不动、同名另一 RAW → Blocked、同名 JPEG 写自身、NEF 与其 sidecar 同选合为一项；新建事务在故障点 1、5–10 崩溃后路径只为“无”或完整 sidecar；NEF 全程逐字节不变 | 通过（4 个端到端测试） |
| 真实 NEF（Z8 ×2、D850；CC0，SHA-256 锁定，23–58 MB） | 四个字段依次写入各自 sidecar，再逆序全部撤销：NEF 逐字节不变、sidecar 消失；Shift 保留相机写入的亚秒与偏移（D850 `…59.09+01:00`、Z8 `…25.67+02:00` 各移 1 小时） | 通过（语料未获取时跳过） |
| 并行执行（worker pool） | 4 个 worker、24 个文件：随机终止 8 次（每次多个文件处于事务中）→ 恢复、继续、撤销逐字节一致；4 个 worker 下的磁盘满：每个文件只为 done 或 cancelled（原内容不变），继续后全部完成；卷许可单元测试（超过卷上限的第 N+1 个事务等待）；熔断按完成顺序计数 | 通过 |
| Copyright | 写入 EXIF `IFD0:Copyright`、XMP `dc:rights` 默认语言、已有 IPTC 时 `CopyrightNotice`；Latin IPTC 上中文 Blocked 且文件不动、Latin 值写入 IPTC；其他语言的 `dc:rights` 保留；同值重新规划为 NoChange；崩溃（故障点 6、8）后恢复与继续；撤销逐字节一致 | 通过（3 个测试） |
| IPTC | Latin IPTC：中文作者 Blocked；`Zoë Morii`（cp1252 可表示）写入成功并更新 IPTCDigest；撤销成功 | 通过 |
| 注入 IO 错误 | 在文件 #2 的故障点 1–10 各注入一次 IO 错误（`--fail-at`）：Operation 不停留在 running；提交前出错 → 该文件 failed 且内容为 H0；提交后出错 → done；其他文件 done；无残留、fsck 干净；撤销后逐字节一致 | 10/10 通过 |
| Journal 写入失败 | SQLite 真实返回 `SQLITE_BUSY`（第二连接持写锁）：`begin`、文件 #2 的 4 个状态写、`finish`，各 once / persist | 12/12 通过 |
| 磁盘满（模拟 112） | 文件 #2 的故障点 1–10：Operation 暂停（cancelled），`resume` 完成，撤销后逐字节一致 | 10/10 通过 |
| Undo 路径 | Undo Operation 在文件 #2 的故障点 1–10 各终止一次、各注入 IO 错误一次 | 20/20 通过 |
| 空间预检 | 余量不可满足时拒绝执行，不登记、不写入 | 通过 |
| manifest / recover / resume 写失败 | manifest.json 在开始、结束、恢复时写入失败（5 例）；恢复已移回 bak 后记录失败（2 例）；`resume` 重新登记时失败（1 例，发现并修正缺陷） | 8/8 通过 |
| Undo 随机终止 | 随机时刻终止撤销 Operation，恢复、继续后逐字节还原 | 12/12 通过 |
| 撤销时文件已删除或移动（G-6） | 在原路径重建原件，移走的副本不动；预览后原路径出现新文件 → Conflict 且不覆盖；文件夹不存在 → Blocked 且不创建；重建事务在故障点 1、5–10 的终止与 IO 错误各 7 例；撤销“重建”→ 移入备份库 | 通过（17 例） |
| 移入备份库（撤销一次“重建”） | 被 Operation 创建的文件从不删除，而是移入备份库；再撤销则重建；事务在故障点 1–4、7–10 的终止与 IO 错误各 8 例 | 通过（16 例） |

2026-09-27 G-1 补充：矩阵、注入方式的真实程度与未覆盖项见 [PHASE1B_FAULT_MATRIX.md](PHASE1B_FAULT_MATRIX.md)。`cargo test --workspace`：62 个测试通过、0 失败（该次运行中真实磁盘满测试被跳过）；之后在 Morii 创建的 64 MB NTFS 测试卷上单独运行真实磁盘满测试（照片卷满、备份/Journal 卷满），两个场景通过，其中 SQLite 真实返回了 `SQLITE_FULL`。第二轮加入 manifest、recover、resume 写失败与 Undo 随机终止后：66 个测试通过、0 失败（真实磁盘满测试跳过，测试卷已卸载）。加入 G-6 后：69 个测试通过、0 失败；加入移入备份库后：70 个；加入 G-7 后：73 个；加入 Copyright 后 82 个；加入并行执行后 87 个（全量连续运行两次均通过；均跳过真实磁盘满测试）。

并行执行的一次简测（有限证据）：200 份夹具副本（8 种 × 25，每份 0.25–10 KB），debug 构建，本机 NVMe SSD，Creator 写入：1 个 worker 10.8 秒，4 个 worker 4.9 秒，200/200 完成。约 2.2 倍而非 4 倍，主要受共享 Journal 串行刷盘限制；大文件与 HDD/NAS 上的表现待 S4。引入并行后，崩溃测试会自然出现“另一个文件正处于 `ReplaceFileW` 中途”的状态；两处测试在崩溃后才规划第二个 Operation，因该路径暂时不存在而规划失败（一处间歇出现），已改为崩溃前规划——这是测试的前提问题，恢复本身对该状态的处理已被测试覆盖。

2026-09-27 事件记录：为 Copyright 字段核对 ExifTool 行为时，一次 PowerShell 5.1 下的探查命令丢失了空参数 `-config ""`，ExifTool 把 `-o` 当作配置文件名，结果**就地改写了锁定的测试夹具 `Writer.jpg`**（留下 `Writer.jpg_original`）。发现后已用 `_original` 恢复，8 个夹具的 SHA-256 与 [PHASE1B_SCALE_VALIDATION.md](PHASE1B_SCALE_VALIDATION.md) 记录一致。改写发生在 G-7 测试与提交之后、其后未运行任何测试，因此没有已记录的结果受影响。此后端到端测试在复制夹具前核对其 BLAKE3，夹具一旦变化即停止测试。

第二轮发现并修正：`resume` 重新登记待重试文件时若 Journal 写失败，已改回 `planned` 的文件会滞留在已结束（`cancelled`）的 Operation 中，恢复与 `resume` 都不再处理它们；现改为先设 `running`。

实现过程中发现并修正的问题：

- 事务中途出现错误（IO、引擎、Journal）时，原实现一律记为 failed；若错误发生在提交之后，会把已提交的文件误记为失败。现改为：用与崩溃恢复相同的判定表按 Journal 与磁盘状态结算该文件（未动 → failed，已提交 → done，异常 → attention）；Journal 本身出错时 Operation 保持 running，由下次启动的恢复处理。

验证机制拦截的真实问题：`Pentax.jpg` 写入后 MakerNotes 中的预览图指针 `Pentax:PreviewImageStart` 移动，V3 判定为附带变化而拒绝提交（原文件未触碰）。修正：文件偏移类标签（`…Offset`、`…Offsets`、`…Start`）在两侧都存在时视为版式派生；新增或消失仍判为附带变化；对应长度标签不得改变（单元测试覆盖）。

2026-09-27 补充：1,000 份 ExifTool JPEG 测试夹具副本的 Creator 写入与逐字节 Undo 已通过；1,000/1,000 写入、复读、撤销及 SHA-256 对比一致，两次 `fsck` 均无问题。样本、环境、首次受限环境失败及结论边界见 [PHASE1B_SCALE_VALIDATION.md](PHASE1B_SCALE_VALIDATION.md)。

本轮加入文件清单解析单测后，在允许 `ReplaceFileW` 的执行环境运行 `cargo test --workspace`：54 个测试通过，0 失败；`cargo fmt --all -- --check` 与 `cargo clippy --workspace --all-targets -- -D warnings` 通过。受限环境的错误 5 不计为产品测试结论。

## 3. 已知缺口（尚未实现或尚未验证）

| # | 缺口 | 计划 |
|---|---|---|
| G-1 | 已测：Journal 写入失败（SQLite 真实 BUSY 与满盘时的真实 FULL）、模拟与真实磁盘满（64 MB VHDX，两个填充时机）、Undo 路径崩溃/IO 错误、空间预检。manifest、`recover`、`resume` 写失败，Undo 随机终止。未测：更多故障位置与真实磁盘满的其他时机 | 本地继续补（[PHASE1B_FAULT_MATRIX.md](PHASE1B_FAULT_MATRIX.md) §4） |
| G-2 | 1,000 个重复小样本 JPEG 的 Creator→Undo 已通过；5,000 文件、1,000 个不同相机原片与真实大文件仍未做 | 等 S4 真实语料 |
| G-3 | 断电、exFAT、云同步目录、真实 NAS 未测 | SAFETY_MODEL §0 A-2/A-3 |
| G-4 | 就地写入只支持 JPEG；NEF/NRW 经 XMP sidecar（已实现，以 ExifTool 自带 Nikon.nef 与 3 个真实 Z8/D850 NEF 测试；第三方软件读 sidecar 待 V-03/V-07）；TIFF 需先补 S2/S3 同类验证 | Phase 3 前 |
| G-5 | 字段注册表 v0 暂定（creator、copyright） | S3 第三方测试后冻结 v1 |
| G-6 | 已实现：撤销时文件已被删除或移动 → 从备份在原路径重建（不覆盖的重命名提交，Journal 角色 `recreate`）。撤销这次重建：把文件移入备份库（锁定 → 复制到备份库并核对 → Ready → 不覆盖地改名为登记的 bak 名 → 核对后删除 bak；Journal 角色 `remove`），再撤销又重建，撤销链可以无限往复。重建的文件不保留原创建时间等属性 | Phase 3 的 sidecar 新建/撤销复用该事务 |
| G-7 | 已实现：`manifest.jsonl` 只追加记录（H0/H1 先刷盘）+ `plan.json`；`mm-cli rebuild-journal` 导入数据库中缺失的 Operation，再按正常恢复处理。已测：完成的 apply 与 undo 链重建后可继续撤销；在故障点 2、4、7、8、9 崩溃及 `ReplaceFileW` 中途状态下丢失数据库，重建、恢复、继续、撤销全部逐字节还原；截断的最后一行被忽略，中间行损坏则不导入。未测：数据库损坏而非丢失（需先移走损坏文件）、备份目录部分缺失；每个文件多两次刷盘的性能开销待 S4 测量 | 产品 UI 中的入口待 Phase 2 |
| G-8 | `resume` 要求应用与 ExifTool 版本不变；版本变化时只能撤销或重新规划 | 设计如此（ARCHITECTURE §11） |
| G-9 | 经 Git Bash 传入的非 ASCII 命令行参数会被代码页转换；CLI 提供 `--set-from UTF8_FILE` | 产品 UI 经 IPC 传值，不受影响 |
| G-10 | 构建：GNU 工具链下 SQLite 需要 PATH 中有 MinGW gcc；MSVC 工具链无此要求 | README 说明 |
