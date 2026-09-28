# MoriMeta — Phase 1b JPEG 写入路径故障注入矩阵

> 2026-09-27；对应 `PHASE1_REPORT.md` G-1 与 `SAFETY_MODEL.md` §12 第一行。本记录只说明下述注入方式在 8 个 ExifTool 测试夹具副本上的结果；注入方式本身的真实程度逐项注明，模拟结果不代替真实磁盘满、断电、exFAT 或 NAS 测试。测试代码：`crates/mm-cli/tests/e2e.rs`、`crates/mm-store/src/lib.rs`、`crates/mm-fs/src/lib.rs` 的单元测试。

## 1. 注入方式

| 方式 | 触发 | 真实程度 |
|---|---|---|
| 进程终止 | `--crash-at SEQ:STEP`：`TerminateProcess` 自身；另有随机时刻 `kill` | 真实进程终止（不含断电：OS 缓存仍会落盘） |
| IO 错误 | `--fail-at SEQ:STEP`：在故障点返回 `io::Error` | 模拟：故障点处的前一步已成功完成 |
| 磁盘满（模拟） | `--disk-full-at SEQ:STEP`：在故障点返回 Win32 112 | 模拟：只检验 112 的分类与处理路径 |
| Journal 写入失败 | `--journal-fail-at begin \| finish \| SEQ:STATE`，可加 `--journal-fail-persist` | **由 SQLite 真实返回**：目标写入前，第二个连接以 `BEGIN IMMEDIATE` 持有写锁，目标 UPDATE/事务得到 `SQLITE_BUSY`。`once` 只让这一次写失败；`persist` 保持锁到进程结束，之后所有 Journal 写都失败。`SQLITE_FULL` 在真实磁盘满测试中由 SQLite 实际返回（见下行）；`SQLITE_IOERR` 未产生 |
| 磁盘满（真实） | `--fill-at SEQ:STEP --fill-dir DIR`：在故障点把 DIR 所在卷的剩余空间全部占用 | 真实 `ERROR_DISK_FULL` 与 `SQLITE_FULL`；`mm_fs::fill_volume` 拒绝填充大于 2 GiB 的卷，不能指向用户磁盘。在 Morii 以管理员创建的 64 MB NTFS VHDX（`tests/fault-lab/small_volume.ps1`）上运行 |
| 空间预检 | `--space-reserve BYTES` 替换 1 GiB 备份卷余量 | 真实调用 `GetDiskFreeSpaceExW`，只把阈值调到不可满足 |

所有注入选项都要求 `MM_FAULT_INJECTION=1`。故障点编号见 `crates/mm-core/src/executor.rs` 文件头：1 锁前 · 2 锁定/指纹后 · 3 备份复制后 · 4 `BackedUp` 落盘后 · 5 临时文件写出后 · 6 验证后 · 7 `Ready` 落盘后 · 8 `ReplaceFileW` 后 · 9 `Committed` 落盘后 · 10 删除 bak 后（`Done` 落盘前）。

每例的检查：恢复前 I-3（每个文件的执行前内容在原路径、登记的 bak 名或备份库中至少一处）；恢复后原路径只为该 Operation 的 H0 或 H1、无 `.mmtmp-`/`.mmbak-` 残留、`fsck` 0 问题；随后 `resume` 与 Undo，最终逐字节等于初始内容（BLAKE3）。

## 2. 矩阵与结果

8 个 ExifTool 夹具副本（Writer、Nikon、Canon、XMP、Sony、Olympus、Pentax、GPS），Creator 设为 `Morii`。

| 路径 | 注入 | 位置 | 例数 | 结果 |
|---|---|---|---:|---|
| Apply | 进程终止 | 故障点 1–10 × 文件 0、3 | 20 | 通过（已有） |
| Apply | 随机终止 | 150–1050 ms | 12 | 通过（已有） |
| Apply | IO 错误 | 故障点 1–10 × 文件 2 | 10 | 通过（已有） |
| Apply | 磁盘满（模拟） | 故障点 1–10 × 文件 2 | 10 | 通过（新增） |
| Apply | Journal 写入失败 | `begin`、文件 2 的 `backed_up`/`ready`/`committed`/`done`、`finish`，各 once / persist | 12 | 通过（新增） |
| Undo | 进程终止 | 故障点 1–10 × 文件 2 | 10 | 通过（新增） |
| Undo | IO 错误 | 故障点 1–10 × 文件 2 | 10 | 通过（新增） |
| Apply | 空间预检不足 | 执行前 | 1 | 通过（新增） |
| Apply | 磁盘满（真实） | 照片卷满于 2:4；备份/Journal 卷满于 2:2 | 2 | 通过（64 MB VHDX，见下） |
| Apply / recover | manifest.json 写入失败 | Operation 开始时、结束时（once / persist），恢复时 | 5 | 通过（第二轮新增） |
| recover | Journal 写入失败 | 恢复已把 bak 移回原路径之后，记录失败（once / persist） | 2 | 通过（第二轮新增） |
| resume | Journal 写入失败 | 重新登记待重试文件时 | 1 | **发现缺陷并修正**（见下） |
| Undo | 随机终止 | 50–750 ms | 12 | 通过（第二轮新增） |
| Undo（重建已删除文件，G-6） | 进程终止、IO 错误 | 故障点 1、5–10 × 文件 2 | 14 | 通过（第三轮新增）：原路径始终只为“不存在”或完整原件 |
| Undo（移入备份库，撤销一次重建） | 进程终止、IO 错误 | 故障点 1–4、7–10 × 文件 2 | 16 | 通过（第四轮新增）：内容始终在原路径、bak 名或备份库之一，最终只在原路径或备份库 |
| Apply | IO 错误、磁盘满（模拟） | 故障点 1、4、7、8、10 × 首个文件（0）与末个文件（7） | 20 | 通过（第六轮新增）；**发现并修正一处状态报告问题**（见下） |
| Undo | 磁盘满（模拟） | 故障点 1–10 × 文件 2 | 10 | 通过（第六轮新增）：撤销 Operation 暂停，每个文件为执行后或原始内容，`resume` 完成撤销 |
| Apply（更新已有 NEF sidecar） | 进程终止、IO 错误 | 故障点 1–10 × 唯一文件 | 20 | 通过（第六轮新增）：sidecar 始终为第一次或完整的第二次内容；两次撤销后 sidecar 不存在，NEF 逐字节不变 |
| Apply（JPEG、新建 sidecar） | 损坏临时输出（`--tamper-at`） | 故障点 5 之后、验证前 × 文件 2 / 唯一文件 | 11 | 通过（第六轮新增）：意外警告 → V1，截断 → V5（重读出现 JPEG 格式错误），改动一个图像数据字节 → V4，删除 `IFD0:Make` → V3（Canon MakerNotes 随之不可解析，也被列出），增加标签 → V3，改错目标值与多追加一项 → V2；文件记为 failed，原文件不变，无残留 |

`cargo test --workspace`（2026-09-27，本机，`ReplaceFileW` 可用）：第一轮 62 个、第二轮 66 个测试通过、0 失败；两次运行中 `real_disk_full_on_small_volume` 均因未设置 `MM_E2E_SMALL_VOLUME` 而跳过。第一轮之后在 64 MB 测试卷上以 `MM_E2E_SMALL_VOLUME` 单独运行该测试：通过（两个场景）；该卷随后已卸载，第二轮未重跑。`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings` 通过。

### 真实磁盘满（64 MB NTFS VHDX）

- **照片卷满**（照片在小卷，数据目录在本机磁盘；在文件 2 的 `BackedUp` 落盘后填满）：ExifTool 写临时文件失败（V1 报错，其文本只含源文件名）；按目标卷剩余空间判定为磁盘满，文件 2 `cancelled`、原内容不变，其后文件 `cancelled`，Operation 暂停。卷仍满时 `recover` 无事可做；删除填充文件后 `resume` 完成，撤销逐字节还原，无残留。
- **备份/Journal 卷满**（数据目录在小卷，在文件 2 备份复制前填满）：备份复制得到真实 Win32 112。Journal 与备份在同一卷，因此随后的 Journal 写入也由 SQLite 真实返回 `SQLITE_FULL`（`database or disk is full`），`apply` 以错误退出、Operation 停在 `running`。两次运行中 `SQLITE_FULL` 出现的时机不同：第一次在文件 2 与其后两个文件已记为 `cancelled` 之后（恢复后文件 5–7 为 `not_started`），第二次更早。卷仍满时 `recover` 同样因 `SQLITE_FULL` 退出，未删除或改动任何文件，I-3 仍成立；删除填充文件后 `recover`、`resume`、撤销全部正确。
- 第一次运行在最后的 `resume` 处失败，原因是测试本身：`resume` 与撤销沿用默认 1 GiB 备份卷余量，64 MB 卷不可能满足，被空间预检拒绝（预检行为正确）。测试改为在小卷上传 `--space-reserve 0` 后重跑通过。

### 观察到的行为

- **Journal `begin` 失败**：登记事务整体回滚，History 为空，没有文件被触碰。
- **提交前的 Journal 写失败**（`backed_up`、`ready`）：文件 2 的 `ReplaceFileW` 从未在 `Ready` 记录落盘之前执行（I-8）；原路径仍为 H0。once 模式下该文件结算为 failed，其余文件完成；persist 模式下 `apply` 以错误退出、Operation 停在 `running`，新写入被 `RecoveryPending` 拒绝，`recover` 后按判定表归为未开始，`resume` 完成。
- **提交后的 Journal 写失败**（`committed`、`done`）：按磁盘哈希结算为 done。此前该文件的报告不带任何错误说明；现在记录 `…; commit confirmed on disk`，使“提交由磁盘而非完整 Journal 确认”可见。
- **`finish` 失败**：所有文件已是终态，Operation 停在 `running`；`recover` 将其结为 `recovered`。
- **磁盘满（模拟）**：按 SAFETY_MODEL §8.13 实现为暂停——当前文件经判定表结算：提交前为 `cancelled`（原内容不变，`resume` 会重试），提交后为 done；其后所有文件 `cancelled`，Operation 状态 `cancelled`，说明为 `paused: a volume is full; free space, then resume`。按此前的代码，磁盘满只会使文件逐个 failed 直到熔断，且 failed 文件不会被 `resume` 重试（代码核对，未单独测试）。
- **Undo 路径**（Restore 分支）的崩溃、IO 错误与随机终止与 Apply 路径同样恢复；Undo 部分失败后，再次对原 Operation 规划 Undo 只恢复仍被修改的文件。
- **`resume` 的 Journal 写失败（缺陷，已修正）**：`resume` 原先先把待重试文件逐个改回 `planned`，最后才把 Operation 设为 `running`。第 3 个重新登记写入失败时，文件 2、3 停在 `planned`，而 Operation 仍为 `cancelled`：恢复不检查已结束的 Operation，`resume` 只重试 `not_started`/`cancelled`，这两个文件从此无法重试（照片未被触碰，没有数据风险）。改为先设 `running` 再重新登记；失败后 Operation 对恢复可见，`recover` 将其归为未开始，`resume` 完成。回归测试 `resume_journal_failure_leaves_operation_recoverable`。
- **最后一个文件提交后才停止（第六轮，已修正）**：磁盘满发生在末个文件的提交之后（故障点 8–10）时，所有文件都已完成，但 Operation 仍记为 `cancelled`，说明为"卷已满，释放空间后继续"，而此时没有可继续的文件。用户取消与失败熔断在同一边界也会如此。改为：只有仍有未开始或已取消的文件时才记为 `cancelled` 并附说明；否则按文件结果记为 `completed` 或 `completed_with_errors`。回归断言在 e2e `io_errors_and_disk_full_at_the_first_and_last_file`。
- **恢复自身的 Journal 写失败**：合成 `ReplaceFileW` 中途终止的状态（原路径不存在、原内容在登记的 bak 名下），恢复已把 bak 移回原路径后记录失败。磁盘动作已发生而记录缺失，再次 `recover` 按磁盘哈希判定为未开始，不重复动作也不丢数据。
- **manifest.json 写入失败**（真实文件系统拒绝：在临时名处放一个目录，Win32 5）：开始时失败 → Operation 已登记但未触碰任何文件，`recover` 归为未开始；结束时失败 → 所有文件已完成，Journal 状态为 `completed`，`apply` 仍报错；恢复时失败 → Journal 已记为 `recovered`。三者都不引入未记录的不可逆动作。**但 manifest 会停留在失败前的内容**：手动核对结束时失败的情形，Journal 为 `completed`，manifest 仍是开始时的版本（状态 `running`、每个文件 `planned`、无 H0），之后的 `recover` 也不会重写它。

### 与规格的差异（代码核对；第五轮已处理）

- 第二轮发现：SAFETY_MODEL §6.1 要求 manifest “执行期间追加写入并 fsync”，而实现只在开始、结束与恢复时整体重写；manifest 写入失败后也没有补写。第五轮以只追加的 `manifest.jsonl` 解决：每次 Journal 变化追加一行，H0/H1 记录先刷盘；`manifest.json` 快照即使停留在旧内容，重建也不依赖它。数据库丢失后的重建与恢复见 PHASE1_REPORT G-7。

## 3. 本轮随测试加入的实现

- `mm-core`：SAFETY_MODEL §6.2 的执行前空间预检（`apply` 与 `resume`）：备份卷需 `Σ 文件大小 × 1.05 + 1 GiB`，每个目标目录所在卷需 `最大文件 × 1.1`；不满足时返回 `InsufficientSpace`，不登记 Operation、不写任何文件。
- `mm-core`：磁盘满分类（Win32 112/39、`SQLITE_FULL`），以及上述暂停语义。ExifTool 写失败只以文本报告，因此在其失败后查询目标卷剩余空间，低于 `文件大小 × 1.1` 时按磁盘满处理；这一启发式在真实照片卷满的场景中生效，但只覆盖了“卷完全填满”一种情形。
- `mm-core`：`resume` 先把 Operation 设为 `running` 再重新登记待重试文件（第二轮）。
- `mm-store`：`WriteFault`（测试用 Journal 写入失败；第二轮加入 `manifest[:N]` 与 `resume` 的重新登记写入）；`mm-fs`：`volume_space`、`is_disk_full`、`fill_volume`（≤ 2 GiB 卷）。
- `mm-cli`：上述注入选项；`tests/fault-lab/small_volume.ps1`（管理员）创建并挂载 64 MB NTFS VHDX 以运行真实磁盘满测试。

## 4. 仍未验证与所需条件

| 项 | 状态 | 需要 |
|---|---|---|
| 真实磁盘满的其他位置 | 只在两个填充时机（2:4 照片卷、2:2 数据卷）各做一次；未在提交后、Undo 中、或卷“接近满”而非完全满时测试 | 需要再次挂载测试卷（管理员运行 `small_volume.ps1`） |
| `SQLITE_IOERR` 形式的 Journal 失败 | 未产生 | 需要能注入存储 IO 错误的环境 |
| `manifest.jsonl` 追加失败 | 2026-09-28 已单独注入（模拟 IO 错误，`--journal-fail-at log:N`）：登记期间与执行中第 0、3、12、20、33 次追加，单次或持续失败。单次失败按 Journal 与磁盘就地结算该文件（提交前 → 失败、原文件不变；提交后且磁盘确认 → 完成并保留错误说明），Operation 继续；持续失败使 Operation 停在 running，恢复后继续、撤销，全部逐字节还原 | 通过（e2e `manifest_log_append_failures_recover`） |
| 故障点位置 | IO 错误与模拟磁盘满已覆盖首个、中间（2）、末个文件；进程终止与 Journal 写失败仍只在 1–2 个文件序号上做 | 可在本地扩大 |
| 规模 | 每例 8 个小文件；SAFETY_MODEL §12 要求 5,000 文件规模重做 | 真实语料（D-13）或合成规模测试 |
| 断电、exFAT/FAT32、云同步目录、真实 NAS | 未测（G-3） | 虚拟机、介质、同步目录、NAS（D-13） |
