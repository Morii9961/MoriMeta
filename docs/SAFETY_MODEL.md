# MoriMeta — Safety Model

> **Version:** 0.3 · **Status:** 草案（未批准）· **Date:** 2026-09-26
> 本文是"不因软件错误损坏用户数据"的工程化定义。§1 的不变量是**内部发布阻断测试标准**；对用户的表述只使用 §2 的措辞，并以 §0 的前提为条件。
> `[F-xx]`/`[V-xx]` 见 [RESEARCH_NOTES](RESEARCH_NOTES.md)；验证数据见 [SPIKE_REPORT](SPIKE_REPORT.md) §3。

---

## 0. 故障模型与前提

§1 的不变量与 §2 的承诺只在以下前提下成立。前提被打破时，MoriMeta 应尽量检测并拒绝写入，但不作保证。

| # | 前提 | 当前状态 |
|---|---|---|
| A-1 | 照片所在卷为**本地 NTFS**，或经 **SMB** 访问的网络共享 | NTFS：S2 验证；SMB：仅回环（`\\localhost`）验证，真实 NAS 未测 |
| A-2 | exFAT/FAT32（SD 卡）、云同步目录 | **未验证**：可移动介质默认禁止就地写入（§8.4，D-5）；云同步目录见 §8.3 |
| A-3 | 存储设备如实执行 `FlushFileBuffers` | 假设；断电测试（虚拟机）**尚未执行** |
| A-4 | 没有其他程序绕过共享模式写入文件（如以备份语义打开的驱动、磁盘级工具） | 假设；普通程序的写入会被锁句柄拒绝（SPIKE_REPORT §3.1） |
| A-5 | 备份卷在执行期间可用、空间充足 | 执行前预检（§6.2） |
| A-6 | MoriMeta 与打包的 ExifTool 未被篡改；同时只有一个 MoriMeta 实例 | 完整性校验（SECURITY_MODEL §5）；单实例锁 |
| A-7 | 进程可能在任何时刻被终止（崩溃、任务管理器、系统关机） | 已纳入故障模型：S2 固定崩溃点与随机终止测试 |

---

## 1. 安全不变量（内部测试标准）

| ID | 不变量 |
|---|---|
| **I-1** | 原文件**从不被原地写入**。唯一允许改变原文件路径内容的操作是：用**已验证的**新文件经 `ReplaceFileW` 提交，或经 Undo 用**已验证的**备份经同一方式提交。ExifTool 永远只写入新的临时文件。 |
| **I-2** | 对已存在文件提交之前，其逐字节备份已写入备份库、经哈希复核，且备份记录已持久化到 Journal。 |
| **I-3** | 任一时刻（含进程被终止），Operation 中每个文件**至少存在一份完整的执行前内容**，且其位置可从持久化 Journal 推导（原路径、登记过的 bak 名、或备份库）。 |
| **I-4** | 仅当目标文件与 Plan 记录的身份一致、且内容在锁定期间未被改变时才提交；否则标为 Conflict，不写入。 |
| **I-5** | 提交前，临时文件必须通过验证（§5）。 |
| **I-6** | 创建新文件（新 sidecar、导出副本、bak 名）**从不覆盖**已存在的文件。 |
| **I-7** | Undo/Restore 不销毁数据：文件当前内容不等于该 Operation 执行后的内容时，必须由用户显式确认，并先备份当前内容。 |
| **I-8** | 任何不可逆动作（提交、删除临时或 bak 文件）之前，描述该动作的 Journal 记录已持久化。 |
| **I-9** | MoriMeta 从不删除用户文件；只删除自己在 Journal 中登记过、且哈希核对无误的临时文件与 bak 文件；备份库只按用户可见的保留策略清理。 |
| **I-10** | MVP 中专有 RAW 只读，从不被修改。 |

测试：§12。S2 原型在 NTFS 与 SMB 回环上以不变量检查器验证了 I-3（恢复前）与"恢复后原路径只可能是 H0 或 H1，且不遗留临时文件与 bak 文件"（恢复后），0 违例。

---

## 2. 对用户的表述

界面与公开文档只使用以下措辞，不使用"事务性批处理""绝不会损坏""任何时候都能退回原样"等说法：

- **单个文件：** 恢复完成后，每个文件只会处于修改前或修改后两种状态之一，不会留下写了一半的文件。（前提见 §0。）
- **批次：** 批次不是全有或全无的；可能部分完成（失败、取消、崩溃）。Windows 上没有可用的多文件事务（TxF 已弃用），MoriMeta 也不假装提供。
- **取消：** 取消会停止后续文件；尚未提交的文件没有被修改；已提交的文件保留修改。
- **撤销：** 在备份有效期内、且文件没有被其他程序改动时，可以把已提交的修改撤销回原样；否则说明原因并提供选项（§7）。

---

## 3. 格式写入策略（FormatPolicy）

写入目标由格式决定，而非由用户规则决定（PRODUCT_SPEC C-01）。

| 格式 | MVP 目标 | 说明 |
|---|---|---|
| JPEG、TIFF | `Embedded` | 经 §4.1；TIFF 需补做 S2/S3 同类验证 |
| NEF、NRW | `Sidecar(<basename>.xmp)` | 与 Adobe/Capture One/Photo Mechanic 约定一致 [F-40]；RAW 只读 |
| XMP（孤立 sidecar） | `Embedded`（自身） | 经 §4.2 |
| DNG、HEIC/HEIF/AVIF、PNG、WebP、其他 RAW | `None`（只读） | 见 PRODUCT_SPEC §5 |

### 3.1 Sidecar 配对规则

| 情况 | 处理 |
|---|---|
| `DSC_0001.NEF` + `DSC_0001.xmp` | 配对；写入更新已存在 sidecar（§4.2）。 |
| 仅 `DSC_0001.NEF` | 新建 `DSC_0001.xmp`（§4.3），只包含 MoriMeta 写入的字段。第三方软件是否正确合并最小 sidecar 待 V-03。 |
| sidecar 扩展名大小写不同（`.XMP`） | 使用已存在的文件名，不新建第二个。 |
| 同目录同 basename 有多个 RAW | sidecar 归属歧义 → `Blocked(AmbiguousSidecar)`。 |
| `DSC_0001.NEF` + `DSC_0001.JPG` | 不冲突：JPEG 写入自身，`.xmp` 归属 NEF。 |
| `DSC_0001.NEF.xmp`（darktable） | 识别并只读；不写入 [F-43]。 |
| `DSC_0001.acr`（Lightroom Classic 15+） | 从不读写、移动或删除 [F-41]。 |
| sidecar 是符号链接 / 多硬链接 / 只读 | 同主文件规则（§8）。 |

更新已有 sidecar 时，全部 RDF 属性（含未知命名空间、结构、History）保留；**XML 注释与原有排版不保留**（SPIKE_REPORT §4）。对外只说"保留全部元数据属性"。

### 3.2 RAW Safe Mode

- MVP 中恒为开启，不提供关闭开关（PRODUCT_SPEC C-03）。
- UI 必须显示：写入目标为 sidecar；只读取文件内 EXIF 的软件（如 Windows 资源管理器）看不到这些修改；Lightroom Classic 需要 "Read Metadata from Files" 才会读入，且其之后写回 XMP 可能覆盖 MoriMeta 的修改（第三方行为待 V-03）。
- 之后的 "Direct RAW write"（仅 NEF）另行要求：机型/固件兼容性验证；强制备份；二次确认；验证包含 `ImageDataHash`（S3：3 个 NEF 仅改元数据后不变）与内嵌预览完整性。

---

## 4. 单文件事务

### 4.1 就地写入（Embedded：JPEG / TIFF）

```text
Planned ─► Locked ─► BackedUp ─► TempWritten ─► TempVerified ─► ReadyToCommit ─► Committed ─► Done
   └────────┴──────────┴────────────┴───────────────┴──── Failed/Skipped/Conflict（原文件未触碰）
```

| 步骤 | 动作 | 失败处理 | 持久化 |
|---|---|---|---|
| 0 Planned | Operation 开始时一次性登记全部文件：目标路径、临时名 `<stem>.mmtmp-<rand>.<ext>`、bak 名 `<stem>.mmbak-<rand>.<ext>`（`<rand>` ≥ 64 位随机）、备份文件名、Plan 指纹、**可执行 Plan 内容**（§9） | — | 单个事务（I-8） |
| 1 Lock | 以 `GENERIC_READ` + 共享 `READ\|DELETE` 打开原文件，持有到提交完成。其他程序此后无法以写方式打开；若已有程序以写方式打开，本步失败（共享冲突） | 共享冲突 → `Skipped(FileInUse)` | 否 |
| 2 Fingerprint | 经锁句柄核对 File ID、大小（与 Plan 一致）；检查只读属性、reparse point、硬链接数、云占位符 | 不一致 → `Conflict`；其他 → `Skipped(reason)` | 否 |
| 3 Backup | **经锁句柄**流式复制到备份库，同时计算 BLAKE3 = **H0**；刷盘；重新读取备份文件复核 H0 | 失败 → `Failed(BackupFailed)`，删除不完整备份 | 是（`BackedUp{H0}`，可与其他文件组提交） |
| 4 Write | ExifTool：`-o <temp> <备份文件>`，即以**已核对哈希的备份副本**为源（`-o` 不覆盖已存在文件 [F-14]）。以原路径为源同样通过了 S2 测试，作为备选 | ExifTool 错误/超时 → 删除临时文件，`Failed(Engine…)` | 否 |
| 5 Verify | §5 全部检查；刷盘临时文件；计算 **H1** | 任一失败 → 删除临时文件，`Failed(VerificationFailed)` | 否 |
| 6 ReadyToCommit | 写 Journal：`{H0, H1, temp, bak, backup}`，fsync | 写入失败 → 中止该文件 | **是**（I-2、I-8） |
| 7 Identity | 以只读属性方式打开原路径，核对其 File ID 仍等于锁句柄的 File ID（锁句柄无法阻止重命名，SPIKE_REPORT §3.1）；核对 bak 名处不存在文件 | 不一致 → 删除临时文件，`Conflict` | 否 |
| 8 Commit | `ReplaceFileW(原文件, temp, bak, 0)`（不使用 `IGNORE_MERGE_ERRORS`/`IGNORE_ACL_ERRORS`） | 见 §4.5 | — |
| 9 Committed | 写 Journal：`Committed{new_file_id}` | — | 是 |
| 10 Post-check | 关闭锁句柄；原路径内容 = H1；bak 内容 = H0 后删除 bak | 不一致 → `NeedsAttention` | 是（`Done`） |

说明：

- 锁句柄在提交后仍指向原内容（此时名为 bak）；S2 中锁句柄持有期间 `ReplaceFileW` 在 NTFS 与 SMB 上都成功。
- **`ReplaceFileW` 对进程终止不是原子的**：S2 的 300 次随机终止中有 14 次落在其执行期间，留下"原路径不存在、原内容只在 bak 名下"的状态，恢复程序全部正确还原（§10）。因此 bak 名必须事先登记在 Journal 中。
- **bak 名处若已有文件，`ReplaceFileW` 会静默覆盖它**（SPIKE_REPORT §3.4）。因此 bak 名使用 ≥ 64 位随机数，并在步骤 7 核对不存在（I-6）。
- 不采用"POSIX 语义重命名"单步替换：NTFS 上丢失 ADS、创建时间、文件属性；SMB 上返回错误 87（SPIKE_REPORT §3.2）。
- `REPLACEFILE_WRITE_THROUGH` 不受支持 [F-50]，因此提交前显式刷盘临时文件。
- 提交后 File ID 变为临时文件的 ID [F-50]，Journal 记录新 ID 供后续核对。
- ReplaceFileW 保留原文件的创建时间、ADS、属性等（S2 已核对前三项）；修改时间策略见 §8.9。

### 4.2 更新已存在的 sidecar

与 §4.1 相同，区别：源与目标均为 `.xmp`；验证包含"除写入目标外所有原有 XMP 属性完整保留"（V3，标签级比较，SPIKE_REPORT §4）；无 `ImageDataHash`。

### 4.3 新建 sidecar

```text
Planned ─► Prechecked(目标路径不存在) ─► TempWritten(-o <temp>.xmp，从零创建) ─► TempVerified ─► ReadyToCommit ─► Created ─► Done
```

- 提交使用**不覆盖**的重命名（`MoveFileExW` 不带 `MOVEFILE_REPLACE_EXISTING`）；目标此刻已被其他程序创建 → `Conflict`，删除临时文件（I-6）。
- 无需备份；Journal 记录"该文件由 Operation 创建"，Undo 时将其移入备份库而不是直接删除（I-9）。

### 4.4 Clean Export（若 D-15 纳入）

```text
Planned ─► Prechecked(输出路径不存在、输出卷空间) ─► TempWritten ─► TempVerified(§5 + V6) ─► Created(不覆盖重命名) ─► Done
```

- 源文件只读访问，不需要备份。
- 输出名冲突策略由用户选择：跳过 / 自动编号。永不覆盖。
- Undo = 将导出文件移入备份库（仅当其哈希仍等于 H1）。

### 4.5 ReplaceFileW 失败时的处理（S2 实测，NTFS 与 SMB 一致）

| 情况 | 返回 | 磁盘状态（实测） | 处理 |
|---|---|---|---|
| 原文件被其他程序打开且未共享删除 | 32 | 原文件 = H0，临时文件仍在，无 bak | 删除临时文件；退避重试 3 次（200 ms / 800 ms / 2 s）后 `Failed(FileInUse)` |
| 临时文件被其他程序打开 | 32 | 同上 | 同上 |
| 原文件只读 | 5 | 同上（只读属性保留） | 预检已跳过只读文件（§8.1）；若仍出现 → `Failed(AccessDenied)` |
| 临时文件与原文件不在同一卷 | 1176 | 同上 | 属于实现缺陷；`Failed(Internal)` |
| `ERROR_UNABLE_TO_MOVE_REPLACEMENT_2` 或进程在执行中被终止 | — | 原路径不存在，原内容在 bak 名下（S2 随机终止中出现 14 次） | 立即（或在恢复时）将 bak 不覆盖重命名回原路径；成功 → 文件视为未开始；失败 → `NeedsAttention`，原内容在 bak 与备份库中，UI 提供恢复入口 |
| 其他错误码 | — | 未实测 | 按 Journal 与磁盘实际哈希判定（§10） |

---

## 5. 提交前验证（Verification）

对临时输出文件执行（同一 ExifTool 命令中读取源与临时文件）：

| # | 检查 | 失败即中止 |
|---|---|---|
| V1 | 写入命令退出状态为 0；stderr 无 `Error`；`Warning` 只允许维护中的"良性警告白名单"中的条目（例如 IPTC 截断警告必须视为失败 [F-26]；MWG 写入时的 "could not be encoded" 必须视为失败） | 是 |
| V2 | **目标值**：每个 `TagOp` 在临时文件中的值等于期望值（数值按 `-n` 比较，文本精确比较，列表按类型规则）；删除操作对应标签不存在。S3 中它能拦截"转换 IPTC 时列表被追加"的错误 | 是 |
| V3 | **无附带变化**：源与临时文件的完整标签集合之差，只能落在 (a) 本次写入/删除的标签，(b) 维护中的"派生标签白名单"（如 `File:FileSize`、`XMP-x:XMPToolkit`、`Photoshop:IPTCDigest`、`Composite:*`、缩略图偏移类标签）之内。S3 中它能发现 `rdf:parseType="Literal"` 被改写 | 是 |
| V4 | **图像数据不变**：`ImageDataHash`（SHA-256）相等（JPEG/TIFF；将来 NEF/DNG 等 [F-19, F-35]）；sidecar 不适用 | 是 |
| V5 | 文件可被 ExifTool 完整重读，且没有新增 `Error` 级诊断 | 是 |
| V6 | （Clean Export）段白名单、标签白名单、预测移除集合 = 实际移除集合（METADATA_MODEL §10.1） | 是 |

白名单用语料确定，并随 ExifTool 升级复核。

---

## 6. 备份库

### 6.1 布局

```text
<backup_root>\                         默认 %LOCALAPPDATA%\MoriMeta\backups，可配置
└── <operation-id>\
    ├── manifest.json                  自描述：数据库丢失时也能恢复
    ├── 00000001.<ext>                 原文件逐字节副本；保留扩展名，因为 ExifTool 以它为写入源（§4.1 步骤 4）
    └── …
```

`manifest.json`（`manifest_version`）每个条目：原始绝对路径、卷序列号、File ID、大小、原修改/创建时间、H0、执行后 H1、备份文件名、条目角色（`PreImage` / `CreatedByOperation` / `PreImageBeforeForcedRestore`）。执行期间追加写入并 fsync；Operation 结束时写入最终版本。

### 6.2 规则

- 就地写入与更新 sidecar 时备份强制，不可关闭（PRODUCT_SPEC C-08）。
- 执行前预检空间：`Σ 需备份文件大小 × 1.05 + 1 GB` ≤ 备份卷可用空间；目标卷需 `N_workers × 最大文件大小 × 1.1`；不满足 → Preview 中阻止执行并说明所需空间。
- 备份位置更改只影响之后的 Operation。
- 备份库位于云同步目录时警告。

### 6.3 保留策略（D-7）

- 默认保留 30 天；总量上限默认为备份卷容量的 10%（可配置）。
- 永不自动清理：最近 10 个 Operation、处于未完成/NeedsAttention 状态的 Operation、用户标记"保留"的 Operation。
- 清理前在 UI 中告知将失去哪些 Operation 的撤销能力；清理后 History 保留记录并标注"备份已清理，不能撤销"。

---

## 7. Undo

### 7.1 模型

Undo 用备份逐字节恢复，不是"反向写入旧值"。字段级 before/after 仍记录在 Journal 中，用于 Inspect 与"备份已清理"时的说明。S2 原型：50 个文件执行后撤销，50/50 逐字节恢复。

### 7.2 流程

```text
op_undo_plan(op, scope)
  → 对 scope 中每个文件：
       若由该 Operation 创建：   current == H1 → MoveToBackupStore
       current == H1：          Restore(backup)
       current == H0：          NoChange
       文件不存在：              Restore(backup)（标注"文件已被删除或移动"）
       其他：                    Conflict（文件在之后被修改）
  → Preview（Conflict 默认排除；"强制恢复"先备份当前内容）
  → 执行：备份复制到同卷临时名 → 复核 == H0 → §4.1 的提交流程
```

- Undo 本身是一个 Operation，可以再次撤销。
- 后续 Operation 修改过同一文件时，前一个 Operation 的 Undo 会进入 Conflict，自然要求"先撤销后面的"；UI 指出是哪个 Operation 修改了它。
- 可以只撤销选定文件。

---

## 8. 环境风险与策略

| # | 风险 | 策略 |
|---|---|---|
| 8.1 | 只读属性（ExifTool 在目录可写时会改写只读文件 [F-13]；ReplaceFileW 对只读原文件返回 5） | 视为用户"锁定"意图：预检跳过并提示；不提供自动去掉只读 |
| 8.2 | 文件被占用 | 锁句柄打开失败即跳过；提交时共享冲突退避重试；最终提示"文件被其他程序占用" |
| 8.3 | 云占位符与同步目录 | 导入时检测 `RECALL_ON_DATA_ACCESS`/`OFFLINE` [V-12]；默认不扫描未下载文件；写入同步目录时提示可能产生同步冲突；**本环境未验证**（A-2） |
| 8.4 | 可移动介质（exFAT/FAT32 无日志） | 默认禁止就地写入，提示先复制到电脑（D-5）；**未验证**（A-2） |
| 8.5 | 网络驱动器（SMB/NAS） | 允许并提示；S2 回环测试通过，真实 NAS 未测；IO 并发降至 2 |
| 8.6 | 符号链接 / junction | 导入不跟随目录链接；文件本身是符号链接 → `Blocked(SymbolicLink)` |
| 8.7 | 硬链接（链接数 > 1） | `Blocked(HardLinked)`：S2 实测替换后其他链接仍指向旧内容 |
| 8.8 | 同一文件经不同路径导入 | 卷序列号 + File ID 去重 |
| 8.9 | 文件修改时间 | 默认不保留（让 mtime 更新），理由：增量备份工具依据大小 + 修改时间判断变化；可在设置中选择保留（`-P` [F-16]）并提示风险（D-6）。创建时间由 ReplaceFileW 保留（S2 已核对） |
| 8.10 | 长路径与特殊文件名 | 内部统一 `\\?\` 形式；ExifTool 13.59 在 334 字符中文路径与 emoji 文件名上读写正常（S0），`Blocked(UnsupportedFileName)` 规则取消，保留回归测试 |
| 8.11 | Lightroom / darktable 等的覆盖 | 无法技术阻止；在 RAW 相关 Preview 与帮助中说明 |
| 8.12 | C2PA 内容凭证 | 导入时检测 JUMBF（S3：可检测）；修改会使凭证失效（APP11 原样保留但签名不再匹配）；Preview 显著警告并默认排除 |
| 8.13 | 磁盘满 | 预检（§6.2）；执行中出现 `DiskFull` → 暂停 Operation |
| 8.14 | 系统睡眠 | 执行期间 `SetThreadExecutionState(ES_CONTINUOUS \| ES_SYSTEM_REQUIRED)` |
| 8.15 | 断电 | 提交前刷盘；恢复以磁盘实际哈希判定状态（§10）；**尚未做断电测试**（A-3） |
| 8.16 | 系统性故障 | 熔断：前 20 个文件中失败率 ≥ 50%，或连续 10 个验证失败 → 暂停并询问 |
| 8.17 | 两个 MoriMeta 实例 | 单实例锁 |
| 8.18 | Plan 生成后文件被外部修改 | 指纹与锁定期间的内容核对（I-4），`Conflict` |

---

## 9. Operation Journal

### 9.1 存储

SQLite（WAL，`synchronous=FULL`，`foreign_keys=ON`），位于 `db\morimeta.sqlite`；manifest.json 作为冗余（§6.1）。S2 原型以追加 JSON 行 + `FlushFileBuffers` 代替，协议相同。

### 9.2 Schema（概要）

```sql
operations(
  id TEXT PRIMARY KEY, kind TEXT,            -- Apply | Undo | CleanExport
  title TEXT, created_at, started_at, finished_at,
  status TEXT,                                -- Running | Completed | CompletedWithErrors | Cancelled | Interrupted | Recovered
  plan_summary_json TEXT, preset_snapshot_json TEXT,
  registry_version INT, exiftool_version TEXT, app_version TEXT,
  backup_root TEXT, backup_state TEXT,        -- Present | Pruned | Partial
  undo_of TEXT NULL
);
op_files(
  op_id, seq INT, asset_path TEXT, role TEXT, -- Embedded | SidecarUpdate | SidecarCreate | Export
  temp_name TEXT, bak_name TEXT, backup_file TEXT NULL,
  fp_size INT, fp_mtime INT, fp_file_id BLOB,
  plan_ops_json TEXT,                         -- 可执行内容：目标、TagOp 列表、期望值（用于"继续"）
  h0 BLOB NULL, h1 BLOB NULL, new_file_id BLOB NULL,
  state TEXT, error_code TEXT NULL, error_detail TEXT NULL,
  updated_at, PRIMARY KEY(op_id, seq)
);
op_field_changes(op_id, seq, field_id, before_json, after_json, kind);
```

### 9.3 持久化点

| 时机 | 内容 | 批量化 |
|---|---|---|
| Operation 开始 | `operations` 行 + 全部 `op_files`（含临时名、bak 名、可执行 Plan 内容） | 单事务 |
| 备份复核后 | `BackedUp{H0}` | 允许组提交 |
| 每个文件提交前 | `ReadyToCommit{H0, H1}` | 允许组提交（≤ 10 ms），但提交必须等待所在组持久化完成 |
| 每个文件提交后 | `Committed` / `Done` / `Failed` | 可延迟组提交（丢失可由恢复按磁盘哈希推断） |
| Operation 结束 | 状态与摘要 | 单事务 |

5,000 文件时持久化 Plan 内容的体积与写入耗时待 S4 测量。

---

## 10. 崩溃恢复

启动时（在允许任何新写操作之前）：

1. 查找 `status = Running` 的 Operation → 标为 `Interrupted`。
2. 对其中每个非终态文件，读取磁盘实际状态并判定：

| Journal 状态 | 磁盘观测 | 判定与自动动作 | 验证 |
|---|---|---|---|
| Planned / Locked / BackedUp | 原路径未变 | 未开始；删除登记的临时文件（若存在） | S2 固定崩溃点 |
| ReadyToCommit | 原路径 = H0，临时文件存在 | 未提交 → 未开始；删除临时文件 | S2 合成状态 S1 |
| ReadyToCommit | 原路径 = H1，bak = H0 | 已提交 → 删除 bak | 合成状态 S2；随机终止 |
| ReadyToCommit | 原路径不存在，bak = H0 | 替换中途 → bak 不覆盖重命名回原路径；删除临时文件；未开始 | 合成状态 S3；**随机终止实际出现 14 次** |
| Committed | 原路径 = H1 | 完成；删除残留 bak | 固定崩溃点 |
| Committed | 原路径 = H0 | 提交在断电中被回滚 → 未开始 | 合成状态 S4（断电本身未测） |
| 任意 | 原路径为其他内容 / 缺失且无 bak | NeedsAttention；不删除任何文件；备份库中的 H0 可供恢复 | 合成状态 S5、S6 |

3. 向用户展示恢复摘要（PRODUCT_SPEC §6.15）：继续剩余文件 / 撤销已完成部分 / 保持现状。
4. 任何删除只针对 Journal 登记的临时名与 bak 名，且删除前核对哈希（I-9）。

`mm-cli fsck` 可对任意 Operation 离线执行同样的判定。

---

## 11. 取消

- 协作式：设置标志后不再启动新文件。
- 处于步骤 1–7（提交前）的文件：中止，删除临时文件，结果为 `Cancelled`（原文件未被修改）。
- 处于步骤 8–10 的文件：完成提交与后检查，结果为已提交（可撤销）。
- 可以立即终止 ExifTool 进程：它只写临时文件（I-1）。
- 取消后可"继续剩余文件"（与崩溃后的"继续"相同，§9 的持久化 Plan），也可撤销已提交部分。

---

## 12. 安全测试

| 测试 | 内容 | 通过标准 | 当前状态 |
|---|---|---|---|
| 故障注入矩阵 | 在 §4 每个状态转换前后注入进程终止；随机时刻终止 | 恢复后不变量检查器 0 违例 | S2 原型：NTFS 固定点 60 例 + 随机 300 次；SMB 30 + 150 次，0 违例。产品实现需在 5,000 文件规模重做，并加入 IO 错误与磁盘满注入 |
| 断电模拟 | 虚拟机内执行中强制断电（≥ 50 次） | 同上 | **未执行**（需要虚拟机，D-13） |
| 往返测试 | Apply → Undo 后逐字节比较 | 100% 一致 | S2 原型 50/50 |
| 验证有效性 | 故意注入错误输出（损坏文件、丢失标签、改变图像数据、列表追加） | V1–V6 全部拦截 | S3/S7 中已观察到 V2、V3、V6 拦截真实问题；系统化注入测试待 Phase 1 |
| Conflict | Plan 后由外部程序修改文件；Undo 前修改文件 | 不覆盖，正确提示 | 合成状态 S5 通过；产品实现待测 |
| 环境 | 只读、占用、符号链接、硬链接、长路径、Unicode 文件名、云占位符、SD 卡、SMB | 行为符合 §8 | 只读/占用/硬链接/长路径/Unicode/SMB 回环：已测；符号链接、云占位符、SD 卡：未测 |
| 语料回归 | 每次升级 ExifTool：完整语料写入 + 验证 + 兼容性抽查 | 0 验证失败或每个失败都有已知原因 | 流程待建立 |
