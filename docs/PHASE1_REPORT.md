# MoriMeta — Phase 1 Report（JPEG + Creator 垂直链路）

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
| `mm-store` | SQLite Journal（WAL、`synchronous=FULL`、schema v1、拒绝更新版本的数据库）；`backups/<op>/manifest.json` 冗余记录 |
| `mm-core` | `engine`（会话自动重启、按大小的超时）、`planner`（预检、指纹、规划）、`verify`（V1–V5 + "预览后被修改"检查）、`executor`（SAFETY_MODEL §4.1 事务、10 个故障点、ReplaceFileW 失败处理、熔断、resume）、`recovery`（§10 判定表）、`undo`、`fsck` |
| `mm-cli` | `scan`、`plan-creator`、`apply`、`recover`、`resume`、`plan-undo`、`history`、`show`、`fsck`；单实例锁；`--crash-at` 需 `MM_FAULT_INJECTION=1` |

Creator 写入规则（METADATA_MODEL §2.2、§6）：EXIF `IFD0:Artist`（`; ` 连接）、XMP `dc:creator`；文件已有 IPTC 时写 `By-line`，Latin 字符集无法表示或超过 32 字节时**整个文件的该字段 Blocked**；已有 `XMP-tiff:Artist` 一并更新；写 IPTC 且已有摘要时更新 `IPTCDigest`。ExifTool 以**已核对哈希的备份副本**为写入源。

## 2. 验证

`cargo test --workspace --release`：53 个测试全部通过（2026-09-27）。端到端测试 `crates/mm-cli/tests/e2e.rs` 使用锁定的 ExifTool 13.59 与 8 个 ExifTool 样本 JPEG（Writer、Nikon、Canon、XMP、Sony、Olympus、Pentax、GPS）：

| 测试 | 内容 | 结果 |
|---|---|---|
| 执行→撤销 | 中文作者名写入；Latin IPTC 的 GPS.jpg 被 Blocked 且不被触碰；fsck 干净；同值重新规划为 NoChange；撤销后逐字节一致 | 通过 |
| 逐步崩溃 | 故障点 1–10 × 文件序号 0、3，共 20 例：每例检查恢复前原内容仍在（路径/bak/备份库）→ 恢复待决时新写入被拒绝 → `recover` → 路径只为 H0 或 H1、无残留、fsck 干净 → `resume` 完成 → 撤销后逐字节一致 | 20/20 通过 |
| 随机终止 | 随机时刻终止 `mm-cli apply`，其后同上（默认 12 次，`MM_E2E_KILLS` 可调） | 12/12 通过 |
| 防护 | 只读、硬链接 → Blocked；预览后被改写 → Conflict 且不写入；执行时被独占打开 → Skipped | 通过 |
| IPTC | Latin IPTC：中文作者 Blocked；`Zoë Morii`（cp1252 可表示）写入成功并更新 IPTCDigest；撤销成功 | 通过 |
| 注入 IO 错误 | 在文件 #2 的故障点 1–10 各注入一次 IO 错误（`--fail-at`）：Operation 不停留在 running；提交前出错 → 该文件 failed 且内容为 H0；提交后出错 → done；其他文件 done；无残留、fsck 干净；撤销后逐字节一致 | 10/10 通过 |
| Journal 写入失败 | SQLite 真实返回 `SQLITE_BUSY`（第二连接持写锁）：`begin`、文件 #2 的 4 个状态写、`finish`，各 once / persist | 12/12 通过 |
| 磁盘满（模拟 112） | 文件 #2 的故障点 1–10：Operation 暂停（cancelled），`resume` 完成，撤销后逐字节一致 | 10/10 通过 |
| Undo 路径 | Undo Operation 在文件 #2 的故障点 1–10 各终止一次、各注入 IO 错误一次 | 20/20 通过 |
| 空间预检 | 余量不可满足时拒绝执行，不登记、不写入 | 通过 |

2026-09-27 G-1 补充：矩阵、注入方式的真实程度与未覆盖项见 [PHASE1B_FAULT_MATRIX.md](PHASE1B_FAULT_MATRIX.md)。`cargo test --workspace`：62 个测试通过、0 失败（该次运行中真实磁盘满测试被跳过）；之后在 Morii 创建的 64 MB NTFS 测试卷上单独运行真实磁盘满测试（照片卷满、备份/Journal 卷满），两个场景通过，其中 SQLite 真实返回了 `SQLITE_FULL`。

实现过程中发现并修正的问题：

- 事务中途出现错误（IO、引擎、Journal）时，原实现一律记为 failed；若错误发生在提交之后，会把已提交的文件误记为失败。现改为：用与崩溃恢复相同的判定表按 Journal 与磁盘状态结算该文件（未动 → failed，已提交 → done，异常 → attention）；Journal 本身出错时 Operation 保持 running，由下次启动的恢复处理。

验证机制拦截的真实问题：`Pentax.jpg` 写入后 MakerNotes 中的预览图指针 `Pentax:PreviewImageStart` 移动，V3 判定为附带变化而拒绝提交（原文件未触碰）。修正：文件偏移类标签（`…Offset`、`…Offsets`、`…Start`）在两侧都存在时视为版式派生；新增或消失仍判为附带变化；对应长度标签不得改变（单元测试覆盖）。

2026-09-27 补充：1,000 份 ExifTool JPEG 测试夹具副本的 Creator 写入与逐字节 Undo 已通过；1,000/1,000 写入、复读、撤销及 SHA-256 对比一致，两次 `fsck` 均无问题。样本、环境、首次受限环境失败及结论边界见 [PHASE1B_SCALE_VALIDATION.md](PHASE1B_SCALE_VALIDATION.md)。

本轮加入文件清单解析单测后，在允许 `ReplaceFileW` 的执行环境运行 `cargo test --workspace`：54 个测试通过，0 失败；`cargo fmt --all -- --check` 与 `cargo clippy --workspace --all-targets -- -D warnings` 通过。受限环境的错误 5 不计为产品测试结论。

## 3. 已知缺口（尚未实现或尚未验证）

| # | 缺口 | 计划 |
|---|---|---|
| G-1 | 已测：Journal 写入失败（SQLite 真实 BUSY 与满盘时的真实 FULL）、模拟与真实磁盘满（64 MB VHDX，两个填充时机）、Undo 路径崩溃/IO 错误、空间预检。未测：manifest 写失败、`recover`/`resume` 自身的写失败注入、更多故障位置 | 本地继续补（[PHASE1B_FAULT_MATRIX.md](PHASE1B_FAULT_MATRIX.md) §4） |
| G-2 | 1,000 个重复小样本 JPEG 的 Creator→Undo 已通过；5,000 文件、1,000 个不同相机原片与真实大文件仍未做 | 等 S4 真实语料 |
| G-3 | 断电、exFAT、云同步目录、真实 NAS 未测 | SAFETY_MODEL §0 A-2/A-3 |
| G-4 | 只支持 JPEG；TIFF 需先补 S2/S3 同类验证 | Phase 3 前 |
| G-5 | 字段注册表 v0 暂定（仅 creator） | S3 第三方测试后冻结 v1 |
| G-6 | 撤销时文件已被删除或移动：当前 Blocked，未实现"恢复到原路径" | Phase 1b 后续 |
| G-7 | 数据库丢失时仅凭 manifest 恢复：未实现 | Phase 4 前 |
| G-8 | `resume` 要求应用与 ExifTool 版本不变；版本变化时只能撤销或重新规划 | 设计如此（ARCHITECTURE §11） |
| G-9 | 经 Git Bash 传入的非 ASCII 命令行参数会被代码页转换；CLI 提供 `--set-from UTF8_FILE` | 产品 UI 经 IPC 传值，不受影响 |
| G-10 | 构建：GNU 工具链下 SQLite 需要 PATH 中有 MinGW gcc；MSVC 工具链无此要求 | README 说明 |
