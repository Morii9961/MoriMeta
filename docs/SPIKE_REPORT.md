# MoriMeta — Phase 0 Spike Report

> **Version:** 0.3 · **Status:** 草案（验证记录，非批准文件）· **Date:** 2026-09-26
> 本文记录 S0、S1、S2、S3、S7 的方法、结果与未覆盖范围。其他 v0.3 文档中的安全、隐私承诺均以本文为依据；本文没有覆盖的内容，其他文档不得写成已验证。
> 复现方式见 `research/README.md`；原始结果在 `research/results/`（JSON）。编号 `[X-nn]` 指本文条目，`[F-xx]`/`[V-xx]` 指 [RESEARCH_NOTES](RESEARCH_NOTES.md)。

---

## 0. 环境与边界

| 项 | 值 |
|---|---|
| 系统 | Windows 11 Home 10.0.26200，非管理员账户 |
| ExifTool | 13.59（Windows 64 位包，SHA-256 与官方 checksums.txt 一致；锁定在 `research/exiftool.lock.json`） |
| 原型语言 | Rust 1.98.1（`x86_64-pc-windows-gnu`）、Python 3.14（仅驱动实验） |
| 文件系统 | 本地 NTFS（E:、D:）；SMB 以 `\\localhost\E$` 回环方式测试 |
| 语料 | ExifTool 源码包 `t/images`（194 个文件，其中 49 个 JPEG）；raw.pixls.us CC0 NEF 3 个（Z8 HE、Z8 无损、D850，见 `research/corpus.lock.json`）；由它们派生的合成样本 |
| 未覆盖 | exFAT/FAT32（需管理员或可移动介质）、云同步目录（会把测试文件上传到用户网盘，未执行）、断电（需要虚拟机）、真实相机直出的 Nikon Z JPEG、真实 C2PA 样本、Lightroom/Capture One/NX Studio 等第三方软件的显示 |

所有原型代码位于 `research/`，不属于产品代码。

---

## 1. S0 — ExifTool 版本与既有实验复现

**版本选择：** 13.59 是当前最新版本，并被 history.html 标为 "Security update"；最新 production release 13.55 早于该安全更新。13.53 另有一次 "Security update (Windows only)"，v0.2 的 F-06 漏记。规则改为"锁定包含全部已知安全修复的最新版本"。

**复现结果**（`research/s0/repro.py`，两种调用方式各跑一次）：

| 条目 | v0.2 结论 | 13.59 复现 | 说明 |
|---|---|---|---|
| F-26 | IPTC 超长被截断且仍写入 | 复现 | |
| F-27 | lang-alt 不带语言后缀会删除其他语言 | 复现 | |
| F-28 | "只改 XMP 后出现 IPTCDigest is not current" | **不成立，已更正** | Photoshop.pm 比较的是已存 IPTCDigest 与当前 IPTC 块的摘要。只改 XMP 不触发警告；**改 IPTC 却不更新 IPTCDigest 才触发**。MWG 规则下，摘要不一致时读取方会忽略 XMP |
| F-30 | 包结构、Perl 5.32.1 | 复现 | 510 个文件、34.5 MB |
| F-31 | PERL5LIB/PERL5OPT 注入 | 复现；清空环境后被阻止 | 两种调用方式都受影响，必须清空环境 |
| F-32 | argfile 中相对文件名 `-o.jpg`/`#x.jpg` 被误解析 | 复现；绝对路径正常 | |
| F-33 | UTF-8 值与中文文件名 | 复现 | |
| F-11 | 代理对（emoji）文件名有问题 | **未复现** | 13.59 下 emoji 文件名读写均正常；v0.2 的 `Blocked(UnsupportedFileName)` 规则可取消，但保留回归测试 |
| F-34 | IPTC 写中文变成 `?` | 复现 | |
| F-35 | `-o` 写入后 ImageDataHash 不变；NEF 可计算 | 复现 | |
| F-36 | 截断 NEF 无 `-m` 时拒绝写入 | 复现 | |
| F-37 | `-o` 不覆盖已存在文件 | 复现 | |
| F-38 | 约 290–300 files/s | **与样本有关** | 同一简单 JPEG ×500：约 490 files/s（Python 驱动），约 715 files/s（Rust 会话）；ExifTool 测试 JPEG 混合（含厂商 MakerNotes）：约 35 files/s。真实性能需 S4 用真实语料测量 |
| F-39 | 冷启动约 0.15 s | 0.126 s | |

**绕过 launcher 直接调用 Perl（候选方案）**（`research/s0/perl_equiv.py`）：

- `perl.exe exiftool.pl` 与官方 launcher 相比：194/194 个文件的完整读取输出相同；8/8 个写入样本输出逐字节相同；334 字符的中文长路径读写均正常。
- launcher 在进程内加载 `perl532.dll`，不另起 `perl.exe` 子进程；终止 launcher 即终止解释器。
- 打包的 Perl：`usesitecustomize=undef`，`@INC` 只含包内 `lib`。
- 两种方式都会执行 `PERL5OPT` 注入，因此环境清空是必须的，与选哪种调用方式无关。
- **结论：** 技术上可行，行为等价。是否采用仍是待决事项（与 CC0 launcher 和签名路线相关，见 RELEASE_PLAN §4）。

---

## 2. S1 — stay_open 协议与参数编码（Rust 原型）

原型：`research/spikes/exiftool-session`。

### 2.1 argfile 行的实际解码规则（`research/s1/argfile_probe.py`，依据 exiftool.pl `FilterArgfileLine`）

| 情况 | 结果 |
|---|---|
| 普通行 | 删除行首空白、行尾 CR/LF；**`=` 后紧跟的一个空格被删除**；空行和 `#` 开头的行被忽略 |
| `#[CSTR]` 行 | `\n` `\r` `\t` `\\` `\"` 精确；**`$` 与 `@` 总会多出一个反斜杠**（无论是否转义），因此无法精确传递 |
| 控制字符（如 BEL）写入 XMP | 存为 `.`（静默改变） |
| 空值（`-TAG=`） | 删除该标签 |

### 2.2 选定的编码方式与往返测试

- **写入：** 命令带 `-ex`（ExifTool 写入时对值做 XML 反转义）；值中的 `& < > " '`、TAB/LF/CR 用字符引用表示，值开头的空格写成 `&#32;`；每个参数都是一行普通行，不使用 `#[CSTR]`。
- **读取：** `-json -api StructFormat=JSONQ`。默认 JSON 会把看起来像数字的字符串输出为数字、把 `true`/`false`（不区分大小写）输出为布尔值；Perl 的 `$` 还能匹配末尾换行之前的位置，因此 `"9\n"` 会被输出为数字 `9`。JSONQ 使所有值都带引号。
- **拒绝域：** NUL、除 TAB/LF/CR 外的 C0 控制字符、U+FFFE/U+FFFF。编码器直接拒绝，不产生任何行。

| 测试 | 结果 |
|---|---|
| 100,000 个随机值（ASCII、空白、XML 特殊字符、argfile 特殊字符、伪造终止标记、CJK、emoji、组合字符、RTL、不可见字符、C1、DEL）写入 → 读回 | launcher：100,000/100,000 精确；perl：100,000/100,000；换种子再跑：100,000/100,000 |
| 同样测试使用 `#[CSTR]` | 可表示的 84,627 个全部精确；15,373 个（含 `$`/`@`）无法表示 |
| 拒绝域约 19,450 个值 | 编码器 100% 拒绝 |

### 2.3 协议与进程监督

| 测试 | 结果 |
|---|---|
| 值注入（`x\n-o\n<path>`、`-execute`、`-stay_open False` 等） | 全部作为单个值精确存储；没有创建额外文件 |
| 敌意文件名（`-o.jpg`、`#x.jpg`、`=a.jpg`、`{ready1}.jpg`、前导空格、Unicode 减号、`%PATH%`、emoji） | 10/10 各得到一条结果 |
| 元数据中伪造的终止标记（`{ready1}`、`{ready<最大u64>}`、`{mm-end:…}`），包括 `-b` 二进制输出 | 输出逐字节精确；后续命令正常 |
| 命令执行中外部终止 ExifTool | 5 ms 内检测到崩溃；重新启动后正常 |
| 挂起（NtSuspendProcess） | 2 s 超时后终止并重启，正常 |
| 5,000 条 stderr 错误（约 0.46 MB） | 0.3 s 完成，5,000 条全部收到 |
| 父进程被杀 | ExifTool 随之终止（Job Object `KILL_ON_JOB_CLOSE`） |
| 命名管道路径 | ExifTool 直接报 "File not found"，不构成挂起（不作为挂起测试） |

**过程中发现并修复的原型缺陷：**

1. 只按"行首出现 `{ready…}`"判断终止会被文件内容伪造。改为每条命令使用新的随机 64 位 ID，只接受与当前 ID 相同的终止标记，其他一律视为数据。
2. `-b` 输出末尾没有换行，ExifTool 自己的 `{ready<ID>}` 会紧跟在数据后面，因此不能要求终止标记位于行首。

---

## 3. S2 — 单文件事务（Rust 原型）

原型：`research/spikes/fs-txn`。候选流程：登记计划 → 以"锁句柄"打开原文件 → 核对指纹 → 经锁句柄把原文件复制到备份库并计算 H0 → 重读备份核对 H0 → ExifTool 以**备份副本**（或原路径）为源写入同目录临时文件 → 验证 → 刷盘、计算 H1 → Journal 记录 ready（fsync）→ 核对路径的 File ID 仍等于锁句柄的 File ID → `ReplaceFileW(原文件, 临时文件, bak 名)` → 记录 committed → 关闭锁句柄 → 核对原路径内容 = H1 → 删除 bak → done。

### 3.1 锁句柄的共享模式（`share-matrix-{ntfs,smb}.json`）

ExifTool 以只读方式打开文件时使用 `GENERIC_READ` + 共享 `READ|WRITE`，不含 `FILE_SHARE_DELETE`（ExifTool.pm `Open`）。

| 锁句柄 | ExifTool 读 | ExifTool 从原路径写临时文件 | 其他程序写打开 | 其他程序重命名 | ReplaceFileW | POSIX 重命名替换 |
|---|---|---|---|---|---|---|
| 无锁 | ✓ | ✓ | 允许 | 允许 | ✓ | NTFS ✓ / SMB ✗(87) |
| **READ，共享 READ\|DELETE（选定）** | ✓ | ✓ | **拒绝(32)** | 允许 | **✓（锁句柄仍读到 H0）** | NTFS ✓ / SMB ✗(87) |
| READ\|DELETE，共享 READ\|DELETE | ✗ | ✗ | 拒绝 | 允许 | ✓ | — |
| READ，共享 READ | ✓ | ✓ | 拒绝 | 拒绝 | ✗(32) | ✗ |

NTFS 与 SMB 回环结果一致（POSIX 重命名除外）。锁句柄无法阻止其他程序重命名文件，因此提交前必须核对路径的 File ID 与锁句柄一致。

### 3.2 提交方式保留了什么（NTFS）

| 属性 | ReplaceFileW | POSIX 重命名 |
|---|---|---|
| 备用数据流（ADS） | 保留 | 丢失 |
| 创建时间 | 保留 | 丢失 |
| 文件属性（NOT_CONTENT_INDEXED） | 保留 | 丢失 |
| 提交后 File ID | 等于临时文件 | 等于临时文件 |

**结论：** 采用 ReplaceFileW；POSIX 重命名不采用（NTFS 上丢失属性，SMB 上不受支持）。

### 3.3 崩溃注入

| 测试 | 规模 | 不变量违例 |
|---|---|---|
| 固定崩溃点：10 个步骤 × 3 个文件位置，源 = 备份副本（NTFS） | 30 例 | 0 |
| 同上，源 = 原路径（NTFS） | 30 例 | 0 |
| 同上，源 = 备份副本（SMB 回环） | 30 例 | 0 |
| 随机时刻终止进程，每次 24 个文件（NTFS） | 300 次 | 0 |
| 随机时刻终止进程，每次 24 个文件（SMB 回环） | 150 次 | 0 |
| 执行 50 个文件后按备份撤销 | 50 个 | 50/50 逐字节恢复 |

不变量检查：崩溃后、恢复前，每个文件至少存在一份完整的执行前内容（原路径、bak 名或备份库之一）；恢复后原路径只能是 H0 或 H1，不残留临时文件与 bak 文件，原路径必须存在。

**关键发现：** 在 NTFS 的 300 次随机终止中，有 14 次恰好发生在 `ReplaceFileW` 执行期间，留下"原路径不存在、原内容只在 bak 名下"的状态，恢复程序全部正确还原。这说明 **ReplaceFileW 对进程终止不是原子的**，必须事先在 Journal 中登记 bak 名。

### 3.4 ReplaceFileW 失败时磁盘上的实际状态（NTFS 与 SMB 一致）

| 诱发条件 | 返回 | 原文件 | 临时文件 | bak |
|---|---|---|---|---|
| 原文件被其他程序以共享 READ 或 READ\|WRITE 打开（无 DELETE） | 32 | H0 | 仍在 | 无 |
| 临时文件被其他程序打开 | 32 | H0 | 仍在 | 无 |
| 原文件只读 | 5 | H0（只读属性保留） | 仍在 | 无 |
| 临时文件在另一卷 | 1176 | H0 | 仍在 | 无 |
| **bak 名处已有文件** | **成功** | H1 | — | **已有文件被静默覆盖** |
| 原文件有第二个硬链接 | 成功 | H1 | — | 另一个硬链接仍是 H0 |

"bak 名已存在会被覆盖"是新的数据丢失途径：bak 名必须使用足够长的随机名，并在提交前核对该路径不存在。

### 3.5 合成恢复状态（SAFETY_MODEL §10 表中崩溃难以直接制造的状态）

7 种状态（ready/committed × 原路径 H0/H1/缺失/被外部修改 × bak/临时文件是否存在）在 NTFS 和 SMB 上均按设计判定。外部修改过的文件与"原路径缺失且无 bak"的情况标为需要注意，不做任何删除。

### 3.6 未验证

exFAT/FAT32、云同步目录、断电（虚拟机硬重置）、真实杀毒软件或 Lightroom 占用、真实 NAS（非回环）。在这些环境得到验证之前，SAFETY_MODEL §0 不把它们列入保证范围。

---

## 4. S3 — 字段映射与 sidecar（ExifTool 侧）

脚本：`research/s3/fields.py`；结果：`research/results/s3/fields.json`。第三方软件部分未执行。

| 条目 | 结果 |
|---|---|
| 显式映射 vs MWG Composite（6 个 JPEG，作者"森 Morii"、版权"© Morii 2026"） | 无 IPTC、UTF-8 IPTC 的文件：两种方式写入的标签与值**完全相同** |
| 同上，Latin IPTC 文件 | **MWG 把 IPTC By-line 写成 `? Morii`，退出码 0，仅有警告，并把 IPTCDigest 更新为"已同步"**。显式映射先把 IPTC 整体转为 UTF-8，再写入正确值 |
| 转换 IPTC 与赋值放在同一条命令 | 未排除被赋值字段时，列表型 By-line 变成 `["Café", "森 Morii"]`（追加而非替换）；复制时排除被赋值字段（`--IPTC:By-line`）后结果正确。值级验证（V2）能拦截前一种错误 |
| Latin → UTF-8 转换 | 全部 IPTC 值保留；新增 `EnvelopeRecordVersion` 与 `CodedCharacterSet=UTF8`；无 IPTCDigest 警告 |
| IPTC 长度（By-line `string[0,32]`、CopyrightNotice `string[0,128]` 等，取自 IPTC.pm） | ExifTool 按**字节**截断，UTF-8 时会**切断多字节字符**（"森"×11 = 33 字节，存储结果不是原值的前缀）。必须在 Plan 阶段按编码后字节数预检 |
| Windows 属性系统（资源管理器/照片） | EXIF 中以 UTF-8 写入的 Artist/Copyright（无 XMP 时）显示正确；有 XMP 时也显示正确 |
| Windows 的拍摄时间 | 资源管理器显示 EXIF 钟面时间原样；属性值按本机时区换算为 UTC，**忽略 OffsetTimeOriginal** |
| 时间标签分布 | Nikon Z8/D850 NEF 与由其派生的 JPEG 在 **IFD0 中也有 DateTimeOriginal**（非标准位置）；相机内 XMP 含 `xmp:CreateDate`（带亚秒）。"已有位置全部更新"规则必须覆盖这些位置 |
| Absolute 写入（z8.jpg） | 只改变了指定标签；亚秒删除后，SubSecTime（对应 ModifyDate）与 IFD0:ModifyDate 保持不变 |
| 最小 sidecar（无源文件新建） | 只包含写入的字段与 `x:xmptk`，每个命名空间一个 `rdf:Description` |
| 更新合成的 LR 风格 sidecar（crs、lr、xmpMM:History、结构、lang-alt、未知命名空间） | 标签级：除 dc:creator 外**无任何变化** |
| sidecar 中的 XML 注释 | **被丢弃**（标签级无变化） |
| `rdf:parseType="Literal"` 内容 | **被改变**（出现警告与额外标签）；标签级比较（V3）能发现 |
| 第二个 `rdf:Description`，`rdf:about` 不同 | 属性保留，写入未报错；`rdf:about` 的差异是否保留未单独核对 |
| MakerNotes 序列号：`-MakerNotes:SerialNumber=` | 实际是**置为空字符串**，不是删除；D2Hs、D70 上其他标签未变 |
| MakerNotes 序列号改为其他值 | D2Hs 与 Z8 上**加密的镜头数据随之解码错误**（D2Hs LensID 变为 "Unknown"）；ExifTool 不重新加密 |
| 整体删除 MakerNotes | 允许；D70 的 Composite:LensID 随之丢失 |
| ExifIFD:SerialNumber / LensSerialNumber | 可干净删除 |
| JUMBF（合成的 c2pa 标签 APP11） | 可检测（`JUMBF:JUMDLabel = c2pa`）；写入其他标签后 APP11 字节原样保留（因此真实清单会保留但签名失效） |
| NEF ImageDataHash（V-02） | 3 个 NEF 多次计算稳定；仅改元数据（`-o`）后不变 |

---

## 5. S7 — Clean Export（JPEG）一致性

按 D-15 (c) 的方向进行验证，**不代表范围已获批准**。脚本：`research/s7/clean_export.py`；段解析器：`research/spikes/jpeg-segments`。

**方法：** 对每个源文件：完整读取（`-a -G0:1 -u -U`）+ 标记段解析 → 生成预览（保留/移除，逐项列出；无法识别的段和标签显示为"未识别 → 移除"）→ 导出（`-all= -tagsFromFile @ -ICC_Profile <保留标签> -XMP-x:XMPToolkit= -o`）→ 输出检查（段白名单、标签白名单、ImageDataHash）→ 一致性（预测移除集合 = 实际移除集合）。

| 项 | 结果 |
|---|---|
| 源文件 | 53 个：49 个 ExifTool JPEG 样本 + 语料 + 合成风险样本（缩略图、GPS、所有者、注释、xmpMM、人物、地名、Photoshop IRB、扩展 XMP、JUMBF、未知 APP5/APP15、APP14、ICC、EOI 后附加整幅 JPEG；MPF + FPXR + 十余种未知 APPn；多段 EXIF + MakerNotes） |
| 段解析 | 49 个 JPEG 样本 0 解析错误 |
| 输出通过检查 | 53/53 |
| **预测移除 = 实际移除** | 53/53（修正预览模型后，见下） |
| ImageDataHash 不变 | 53/53 |
| 负对照（只删 GPS 的导出器；在合格输出上追加尾部数据；插入 APP9；重新加入缩略图） | 4/4 被阻止 |

**过程中发现的问题：**

1. 最初的预览模型把 ExifTool 重建 EXIF 时自动写入的结构标签（ExifVersion、ComponentsConfiguration、YCbCrPositioning、InteropVersion）预测为"移除"，一致性检查因此拦截了 41 个文件。
2. 分辨率、FlashpixVersion、ExifImageWidth/Height 与 JFIF 段实际会被移除，而模型预测为保留。
3. YCbCrPositioning 与 ExifVersion 会被替换为 ExifTool 的默认值。

修正：这些结构字段从源文件复制回来（不含个人信息），其余结构字段明确列为"移除（结构性）"，并把"预测保留但被移除"也作为阻止条件。修正后 53/53 一致。

**验证后可以支持的表述（范围仅限本节语料）：** 对 JPEG 输入，导出文件只包含白名单中的标记段和标签；每个文件都在导出后逐个检查，不满足即不导出；预览中列出的移除项与实际移除一致。

**不能支持的表述：** 画面内容本身（可见文字、水印、隐写）；未在语料中出现的结构（真实相机的 C2PA 清单、HDR gain map、厂商私有段的新变体）；JPEG 以外的格式。

---

## 6. 对 v0.2 设计的修改（汇总）

| # | 修改 | 依据 |
|---|---|---|
| X-01 | ExifTool 锁定 13.59（规则：包含全部已知安全修复的最新版本） | §1 |
| X-02 | 参数编码改为 `-ex` + XML 字符引用；读取改用 `StructFormat=JSONQ`；不使用 `#[CSTR]` | §2 |
| X-03 | 终止标记使用每条命令随机的 64 位 ID，只接受当前 ID | §2.3 |
| X-04 | ExifTool 进程放入 Job Object（KILL_ON_JOB_CLOSE） | §2.3 |
| X-05 | 锁句柄：READ + 共享 READ\|DELETE，从备份开始持有到提交 | §3.1 |
| X-06 | ExifTool 以已核对哈希的备份副本为写入源（原路径方式同样通过测试，作为备选） | §3.3 |
| X-07 | 提交用 ReplaceFileW；bak 名必须登记、足够随机，且提交前核对不存在 | §3.2–3.4 |
| X-08 | 写入映射采用显式映射，不用 MWG Composite 写入 | §4 |
| X-09 | IPTC 按字节预检；转换 IPTC 时排除本次要赋值的字段 | §4 |
| X-10 | 时间字段的"已有位置"包含 IFD0:DateTimeOriginal 与相机内 XMP 日期 | §4 |
| X-11 | sidecar 更新：RDF 属性保留，XML 注释不保留，对外表述照此 | §4 |
| X-12 | Clean Export 的检查模型（段白名单 + 标签白名单 + 预测/实际一致）；结构字段从源文件复制 | §5 |
| X-13 | F-28、F-38、F-11 的表述更正；F-06 补充 13.53 | §1 |
