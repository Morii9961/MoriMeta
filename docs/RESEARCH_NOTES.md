# MoriMeta — Research Notes & Verification Register

> **Version:** 0.3 · **Status:** Phase 0 研究记录，2026-09-26（v0.3：S0/S1/S2/S3/S7 结果已回填，详见 [SPIKE_REPORT](SPIKE_REPORT.md)）
> **Purpose:** 记录本轮架构设计所依据的一手资料与本地实验结果；列出仍需在 Spike 中验证、不能凭经验假设的事项。
> 其他文档中出现的 `[V-xx]` 均指本文 §3 的验证项；`[F-xx]` 指本文 §1 的已核实事实。

---

## 1. 已核实事实（Verified Facts）

所有条目均来自官方文档或本地实验。日期为本次查阅日期（2026-09-26）。

### 1.1 ExifTool

| ID | 事实 | 来源 |
|---|---|---|
| F-01 | 当前版本 13.59（2026-05-27，标注 "Security update"）；最新 **production release 为 13.55**（2026-04-07），早于 13.59 的安全更新。MoriMeta 锁定 13.59（规则：包含全部已知安全修复的最新版本，`research/exiftool.lock.json`）。 | exiftool.org/history.html（2026-09-26 复核） |
| F-02 | 许可证："free software; you can redistribute it and/or modify it under the same terms as Perl itself"（即 Artistic License 或 GPL 任选）。 | exiftool.org 首页 License 节；exiftool_pod |
| F-03 | 读写支持：JPEG/TIFF/PNG/WebP/HEIC/AVIF/XMP 为 R/W/C；NEF/NRW/DNG/CR2/CR3/ARW/RAF 为 R/W。 | exiftool.org 支持格式表 |
| F-04 | 官方警告："there is the possibility that it will corrupt some files. Be sure to keep backups"；"It is not recommended to remove all metadata from RAW images"；删除全部元数据仅对 JPEG 较彻底，TIFF 可能在 IFD0 残留，RAW 不建议。 | 首页 Writer Limitations |
| F-05 | ExifTool 历史上发生过**版本级文件损坏 bug**：12.45–12.74 可损坏部分 Sony ARW；13.25 修复新 iPhone HEIC 损坏；13.28 修复 Panasonic RW2 损坏；13.49 修复 motion-photo HEIC 在 Google Photos 不显示；13.09 修复 HEIC gain map 问题。 | history.html / ancient_history.html / Known Problems |
| F-06 | 近期安全更新：13.50（CVE-2026-3102，macOS，`-n` + 复制到 FileCreateDate 时命令注入）、**13.53（"Security update (Windows only)"，v0.2 漏记）**、13.54（CVE-2026-7580，`-ee` 路径代码注入）、13.59 标注 "Security update"。历史：12.24（CVE-2021-22204 DjVu RCE）、12.38（CVE-2022-23935 文件名以 `|` 结尾被当作管道执行）。 | history.html；securelist.com；NVD |
| F-07 | 官方 Security Issues：`-config`、`-if`、`-p`、`-fileNUM`、`-api filter(w)` 及 `"-DSTTAG<STR"` 复制参数可执行 Perl 代码；调用方必须保证文件名不以 `-`（U+002D）或 Unicode 减号（U+2212）开头，或放在 `--` 之后。ExifTool 启动时会执行 ExifTool 目录、`EXIFTOOL_HOME`/`HOME`/`HOMEDRIVE+HOMEPATH`、**当前工作目录**中的 `.ExifTool_config`。 | 首页 Security Issues |
| F-08 | `-config ""` 可禁用默认配置文件，且必须是命令行第一个参数，作用于所有 `-execute` 命令。 | exiftool_pod `-config` |
| F-09 | `-stay_open True -@ -`：参数逐行写入 stdin；`-executeNUM` 触发执行并在 stdout 输出 `{readyNUM}`；`-echo3`/`-echo4` 在处理完成后向 stdout/stderr 输出文本，可用 `${status}` 取退出状态；管道方式无额外延迟。 | exiftool_pod `-stay_open`, `-echo` |
| F-10 | argfile：一行一个参数（不是一个选项）；空行与 `#` 开头行被忽略；`#[CSTR]` 开头的行按 C 字符串解析（可含 `\n`）；**行首空白被删除**。 | exiftool_pod `-@` |
| F-11 | Windows 文件名：命令行参数受代码页影响，推荐 **UTF-8 argfile + `-charset filename=utf8`**；该选项须在 `-@` 之前；代码点 > U+FFFF（代理对）的文件名仍有问题。**13.59 实测：emoji 文件名经 UTF-8 argfile 读写均正常（S0）。** | exiftool_pod "WINDOWS UNICODE FILE NAMES"；research/s0 |
| F-12 | Windows 长路径：旧版最大 246 字符；13.07 起默认启用 API `WindowsLongPath`（需 Win32::API，EXE 包内含）。 | Known Problems 2024-08-01；ExifTool.html |
| F-13 | 只读文件：只要目录可写，ExifTool 默认即可改写只读文件（rename 方式）。 | exiftool_pod "WRITING READ-ONLY FILES" |
| F-14 | `-o OUTFILE`：**不会覆盖已存在文件**；若目标为已存在目录或以 `/` 结尾则视为目录；可从零创建 XMP/EXIF/MIE 等。 | exiftool_pod `-o` |
| F-15 | `-overwrite_original` 通过"重命名临时文件替换原文件"实现；`-overwrite_original_in_place` 以 update 模式回写数据以保留属性/硬链接，速度更慢。默认行为是保留 `FILE_original`。 | exiftool_pod |
| F-16 | `-P` 保留 FileModifyDate；Windows 上 FileCreateDate 在 Win32API 可用时默认保留。 | exiftool_pod `-P` |
| F-17 | MakerNotes 标签为 "Permanent"：可编辑，**不可单独创建或删除**；删除 EXIF/IFD0 会连带删除 MakerNotes；删除 ExifIFD 也会删除 MakerNotes。 | exiftool_pod `-TAG=` notes 2 与分组删除说明 |
| F-18 | 伪标签可产生文件系统副作用：`FileName`/`Directory`（移动/重命名）、`HardLink`/`SymLink`（创建链接）、`FilePermissions`、`FileModifyDate`/`FileCreateDate`、`Geotag`（读取任意轨迹文件）。 | TagNames/Extra.html |
| F-19 | `ImageDataHash`（API `ImageHashType`=MD5/SHA256/SHA512）：对 JPEG、TIFF、PNG、CRW、CR3、MRW、RAF、X3F、IIQ、JP2、JXL、HEIC、AVIF、WEBP 等计算主图像数据哈希，不含 Thumbnail/Preview。 | TagNames/Extra.html |
| F-20 | 字符集：外部字符集默认 UTF-8；**IPTC 在 CodedCharacterSet 未定义时假定 Latin(cp1252)**；EXIF "ASCII" 默认不转换；MWG 建议 EXIF 字符串用 UTF-8，`-use MWG` 会设置 `-charset exif=utf8`；XMP 总是 UTF-8。官方建议新建 IPTC 时设置 `CodedCharacterSet=UTF8`，转换既有 IPTC 编码需重写全部 IPTC 标签。 | faq.html #10 |
| F-21 | MWG Composite：读取时按 MWG 规则调和，写入时同步 EXIF/IPTC/XMP；**仅当原文件已有 IPTC 时才写 IPTC**；自动维护 IPTCDigest；加载 MWG 会启用 strict 模式（忽略非标准位置的元数据）；EXIF:Artist 在 MWG 下变为以 `; ` 分隔的列表。覆盖字段：City、Copyright、Country、CreateDate、Creator、DateTimeOriginal、Description、Keywords、Location、ModifyDate、Orientation、Rating、State。 | TagNames/MWG.html |
| F-22 | 官方 sidecar 手册：`-o %d%f.xmp` 可批量创建但**不能修改已存在 XMP**；`-tagsfromfile @ -srcfile %d%f.xmp` 可创建或更新；`-all:all` 保留命名空间位置。 | exiftool.org/metafiles.html |
| F-23 | Windows EXE 包自 12.88 起改用 Oliver Betz 的 Strawberry Perl 打包；移动 exe 必须同时移动 `exiftool_files`；可执行文件名中括号内文本会被当作命令行选项（如 `exiftool(-k).exe`）。 | install.html；包内 windows_exiftool.txt |
| F-24 | "ExifTool NEVER makes an internet connection"。 | 首页 Security Issues |
| F-25 | 使用 `-ec`/`#[CSTR]`/`-E` 等方式写入含换行的值；读取时默认把控制字符转为 `.`，JSON 输出保留换行。 | faq.html #21 |

### 1.2 ExifTool Windows 包实测（13.59_64，SHA-256 `44b512b2…c2cba9ec` 与官方 checksums.txt 一致）

v0.2 中这些是作者本机实验。v0.3 已用 `research/s0/repro.py` 在锁定版本 13.59 上以两种调用方式复现（结果 `research/results/s0/`）：除 F-28（已更正）与 F-38（已修订）外全部复现。

| ID | 结果 |
|---|---|
| F-30 | 包结构：`exiftool(-k).exe` + `exiftool_files/`（35 MB，约 510 个文件），内含 `perl.exe`、`perl532.dll`（**Perl 5.32.1**）、`libgcc_s_seh-1.dll`、`libstdc++-6.dll`、`libwinpthread-1.dll`、`liblzma-5__.dll`、`LICENSE`（GPL-3 全文）、`Licenses_Strawberry_Perl.zip`（graphite2、harfbuzz、bzip2、BerkeleyDB、expat、freetype、gd、gmp 等组件许可证）、`readme_windows.txt`（launcher 为 CC0）。 |
| F-31 | **环境变量注入已复现**：设置 `PERL5LIB=<dir>` + `PERL5OPT=-Mevil` 后运行 `exiftool.exe -ver`，`evil.pm` 中的代码被执行。→ 子进程必须清空环境变量。 |
| F-32 | argfile 中 `-o.jpg` 这样的相对文件名被解析为选项（报 `Invalid TAG name`）；以 `#` 开头的相对文件名行被**静默当作注释跳过**（文件未被处理且无报错）。使用绝对路径时均正常。 |
| F-33 | 通过 stdin argfile 传入 UTF-8 值与 UTF-8 中文文件名，读写结果字节级正确；JSON 输出的 `SourceFile` 为正确 UTF-8。 |
| F-34 | 写 `IPTC:By-line=森` 时出现 `Warning: Some character(s) could not be encoded in Latin`，结果存为 `?`。→ **中文作者名写入 IPTC 会静默丢失**，除非处理 CodedCharacterSet。 |
| F-35 | `-o tmp` 写 JPEG 后，原文件与输出文件的 `ImageDataHash`（SHA256）一致；NEF 可计算 `ImageDataHash`（TIFF 系 RAW）。 |
| F-36 | ExifTool 自带测试 NEF 在写入时报 `[minor] Undersized IFD0 StripByteCounts` 并**拒绝写入**（未加 `-m`）。→ 默认不得使用 `-m` 写入。 |
| F-37 | `-o` 指向已存在文件时返回错误且不覆盖（exit 1）。 |
| F-38 | **（v0.3 修订）** 吞吐与样本关系很大：500 个相同的简单 JPEG 约 490 files/s（Python 驱动）/ 约 715 files/s（Rust 会话）；循环使用 25 个 ExifTool 测试 JPEG（含厂商 MakerNotes）约 35 files/s。v0.2 的 290–300 files/s 与"未并发读 stderr 降至 43 files/s"的实验方法未记录，不再引用；stdout/stderr 仍须并发读取（S1：5,000 条错误的 stderr 在并发读取下 0.3 s 完成）。真实文件吞吐见 V-06。 |
| F-39 | `exiftool.exe -ver` 冷启动约 0.15 s。 |
| F-26 | 写入超长 IPTC 值（By-line 40 字符）：ExifTool 输出 `Warning: [Minor] IPTC:By-line exceeds length limit (truncated)`，**仍然写入**，值被截断为 32 字节。→ minor 警告不会阻止写入；Preview 必须预先校验 IPTC 长度，执行后验证必须比对实际值。 |
| F-27 | 写 `-XMP-dc:Rights=值`（不带语言后缀）会**删除该 lang-alt 的所有其他语言项**（测试中 `de` 项丢失）；写 `-XMP-dc:Rights-x-default=值` 则保留其他语言。→ 所有 lang-alt 字段必须显式写 `-x-default`。 |
| F-28 | **（v0.3 更正）** v0.2 称"仅修改 XMP 后出现 IPTCDigest is not current"——**不成立**。Photoshop.pm 比较已存 IPTCDigest 与当前 IPTC 块的摘要：只改 XMP 不触发；**改 IPTC 却不更新 IPTCDigest 才触发**；同时写 `-Photoshop:IPTCDigest=new` 则不触发（S0 复现）。MWG 建议在摘要不一致时忽略 XMP。→ 写 IPTC 且已有摘要时必须同时更新摘要。 |

### 1.3 Adobe / 第三方软件的 Sidecar 行为

| ID | 事实 | 来源 |
|---|---|---|
| F-40 | Lightroom Classic：专有 RAW 的 XMP 写入 sidecar "to avoid file corruption"；**JPEG、TIFF、PSD、DNG 的 XMP 写入文件内部**。 | helpx.adobe.com Lightroom Classic "Metadata basics and actions"（2025-06-17） |
| F-41 | Lightroom Classic 15.0（2025-10）起，重编辑时额外生成 **`.acr` sidecar**，与 `.xmp` 并存，并由 LR 管理其移动/重命名/删除生命周期。 | helpx "Save metadata to external sidecar files"（2025-10-27） |
| F-42 | Lightroom 修改 capture time 写回 RAW 需在 Catalog Settings 中开启；否则仅在 catalog/XMP。修改 capture time 改变 DateTimeOriginal 与 DateTimeDigitized，不改变 DateTime（ModifyDate）。 | 同 F-40 |
| F-43 | darktable sidecar 命名为 `<basename>.<ext>.xmp`；导入后数据库优先，外部修改在下次同步时被覆盖（可配置启动时检查）。 | docs.darktable.org sidecar 页 |

### 1.4 Windows 平台

| ID | 事实 | 来源 |
|---|---|---|
| F-50 | `ReplaceFileW`：保留被替换文件的创建时间、短文件名、Object ID、DACL、加密、压缩、命名流；三个文件必须位于**同一卷**；`REPLACEFILE_WRITE_THROUGH` **不受支持**；未提供 backup 名时若返回 `ERROR_UNABLE_TO_MOVE_REPLACEMENT`，**原文件已不存在、替换文件仍在原名下**；提供 backup 名时两者保留原名。结果文件的 File ID 等于替换文件。 | learn.microsoft.com ReplaceFileW |
| F-51 | 代码签名：MSIX 经 Microsoft Store 发布由 Store 免费重签名、无 SmartScreen 警告；Store 的 MSI/EXE 路径需自行 Authenticode 签名；Azure Artifact Signing ≈ $9.99/月，**个人仅限美国/加拿大**；OV 证书 $150–300/年、私钥须在 HSM（2023-06 起）；**EV 自 2024 年起不再立即绕过 SmartScreen**；SignPath Foundation 为合格开源项目免费签名。 | learn.microsoft.com "Code signing options"（2026-08-29） |

### 1.5 Tauri

| ID | 事实 | 来源 |
|---|---|---|
| F-60 | 最新：tauri 2.11.5（2026-07-01）、tauri-cli 2.11.4、plugin-updater 2.10.1、plugin-dialog 2.7.2、plugin-fs 2.5.1。 | v2.tauri.app/release |
| F-61 | Updater：签名**强制且不可关闭**；公钥写在 tauri.conf.json；生产环境强制 HTTPS；支持 NSIS 与 MSI；Windows 安装更新时**应用会被自动退出**；installMode：passive/basicUi/quiet。 | v2.tauri.app/plugin/updater |
| F-62 | Sidecar（externalBin）要求单个可执行文件 + `-$TARGET_TRIPLE` 后缀；JS 端调用需 `shell:allow-execute/spawn` 权限。→ ExifTool Windows 包（exe + 文件夹）不适合 externalBin，应作为 `bundle.resources` 打包并由 Rust 直接启动。 | v2.tauri.app/develop/sidecar |
| F-63 | 安全模型：WebView 只能通过 IPC 访问显式暴露的资源；Capabilities/Permissions/Scopes、CSP、Isolation Pattern、Runtime Authority。 | v2.tauri.app/security |
| F-64 | Windows 安装器：NSIS 默认 per-user（`%LOCALAPPDATA%`，无需管理员）；MSI 只能在 Windows 上构建；WebView2 模式 downloadBootstrapper（默认）/embedBootstrapper(+1.8MB)/offlineInstaller(+127MB)/fixedVersion(+180MB)；Windows 10+ WebView2 随系统分发。 | v2.tauri.app/distribute/windows-installer |
| F-65 | Tauri 无原生 MSIX 输出；社区工具 `tauri-windows-bundle` 可生成 MSIX/msixbundle。 | GitHub tauri-apps/tauri #8548；Choochmeque/tauri-windows-bundle |

### 1.6 测试素材

| ID | 事实 | 来源 |
|---|---|---|
| F-70 | raw.pixls.us 样本以 CC0 发布，按厂商/机型组织，darktable/RawTherapee 以其测试。 | raw.pixls.us |
| F-71 | ExifTool 源码包 `t/images/` 含 `Nikon.nef`、`DNG.dng`、多种 JPEG 等测试样本（样本 NEF 为截断文件，不能用于写入测试）。 | Image-ExifTool-13.59.tar.gz |

---

### 1.7 Phase 0 Spike 新增事实（v0.3）

| ID | 事实 | 来源 |
|---|---|---|
| F-80 | exiftool.pl `FilterArgfileLine`：普通行删除行首空白、行尾 CR/LF，并删除 `=` 后紧跟的一个空格；`#[CSTR]` 行精确解码 `\n \r \t \\ \"`，但 `$` 与 `@` 总会多出一个反斜杠。 | exiftool.pl 源码；research/results/s1/argfile-probe.json |
| F-81 | 写入命令带 `-ex` 时，值中的 XML 字符引用在写入前被反转义；以此编码 100,000 个随机值往返 0 差异（两种调用方式）。 | S1；XMP.pm `UnescapeXML` |
| F-82 | 默认 `-json` 把看起来像数字的字符串输出为数字、把 `true`/`false`（不区分大小写）输出为布尔值；Perl 的 `$` 匹配末尾换行之前，因此 `"9\n"` 被输出为数字 `9`。`-api StructFormat=JSONQ` 使所有值带引号。 | exiftool.pl `EscapeJSON`；S1 |
| F-83 | 控制字符（如 BEL）写入 XMP 时被存为 `.`；空值 `-TAG=` 删除标签。 | S1 |
| F-84 | `-b` 输出末尾没有换行，`{ready<ID>}` 直接跟在数据之后。 | S1 |
| F-85 | Windows 启动器在进程内运行 Perl（加载 `perl532.dll`），不另起 `perl.exe`；直接 `perl.exe exiftool.pl` 与之行为等价（194/194 读取、8/8 写入字节相同）；打包 Perl `usesitecustomize=undef`，`@INC` 仅含包内 lib。 | S0 |
| F-86 | ExifTool 以只读方式打开文件时使用 `GENERIC_READ` + 共享 `READ\|WRITE`（不含 `FILE_SHARE_DELETE`）；`Exists` 检查使用共享 `READ`。 | ExifTool.pm `Open`/`Exists` |
| F-87 | 持有 `GENERIC_READ` + 共享 `READ\|DELETE` 的句柄时：ExifTool 可读、可以原路径为源写临时文件；其他程序写打开返回 32；其他程序可重命名；`ReplaceFileW` 成功，句柄随后读到的是原内容（此时位于 bak 名下）。NTFS 与 SMB 回环一致。 | S2 |
| F-88 | `SetFileInformationByHandle(FileRenameInfoEx, REPLACE_IF_EXISTS\|POSIX_SEMANTICS)` 在 NTFS 上成功但不保留 ADS、创建时间、文件属性；在 SMB 回环上返回 87。`ReplaceFileW` 保留这三者。 | S2 |
| F-89 | `ReplaceFileW` 对进程终止不是原子的：随机终止中出现"原路径不存在、原内容在 bak 名下"的状态（NTFS，300 次中 14 次）。 | S2 |
| F-90 | `ReplaceFileW` 失败码实测：原文件或临时文件被其他程序占用 → 32；原文件只读 → 5；临时文件在另一卷 → 1176；三种情况磁盘均未改变。bak 名处已有文件时**成功并覆盖该文件**。原文件有第二个硬链接时成功，另一链接保留旧内容。 | S2 |
| F-91 | MWG Composite 写入：Latin IPTC 中写入无法用 cp1252 表示的字符时，IPTC 值变为 `?`，退出码 0，仅有警告，IPTCDigest 被更新。 | S3 |
| F-92 | 同一命令中 `-tagsFromFile @ -IPTC:all` 与对列表型 IPTC 标签赋值同时使用时，值被追加而非替换；排除被赋值标签（`--IPTC:By-line`）后正确。 | S3 |
| F-93 | IPTC 超长时 ExifTool 按字节截断并仍写入；UTF-8 下会切断多字节字符。IPTC 长度上限取自 IPTC.pm（By-line `string[0,32]`、CopyrightNotice `string[0,128]`、ObjectName 64、City 32、Keywords 64）。 | S3；IPTC.pm |
| F-94 | Nikon Z8、D850 NEF 在 IFD0 中也有 DateTimeOriginal；相机写入 OffsetTime*；NEF 内含相机写入的 XMP（`xmp:CreateDate` 带亚秒、`crd:*`）；序列号同时在 ExifIFD 与 MakerNotes。 | S3（raw.pixls.us CC0 样本） |
| F-95 | `-MakerNotes:SerialNumber=` 实际把序列号置为空字符串；把它改为其他值会使 Nikon 加密镜头数据解码错误（D2Hs LensID 变为 Unknown；Z8 多个标签改变）；整体删除 MakerNotes 允许，但 D70 的 Composite:LensID 随之丢失。ExifIFD:SerialNumber/LensSerialNumber 可干净删除。 | S3 |
| F-96 | 更新已有 XMP sidecar：全部 RDF 属性（未知命名空间、结构、History、lang-alt）保留；XML 注释丢弃；`rdf:parseType="Literal"` 内容被改写（出现警告与额外标签）。 | S3 |
| F-97 | Windows 属性系统：EXIF 中以 UTF-8 写入的 Artist/Copyright 显示正确；"拍摄日期"按钟面时间显示，属性值按本机时区换算，不使用 OffsetTimeOriginal。 | S3（Windows 11） |
| F-98 | 合成的 c2pa JUMBF（APP11）可被检测（`JUMBF:JUMDLabel=c2pa`）；写入其他标签后 APP11 原样保留。 | S3 |
| F-99 | 3 个 NEF 的 ImageDataHash 多次计算稳定，仅改元数据（`-o`）后不变。 | S3 |
| F-100 | `-all=` 会删除 JFIF 段、ExtendedXMP、Photoshop IRB、COM、未知 APPn、EOI 之后的数据；保留 APP14 Adobe；重建 EXIF 时 ExifTool 自动写入 ExifVersion、ComponentsConfiguration、YCbCrPositioning 等结构标签（值可能不同于源文件）。 | S7 |
| F-101 | SignPath Foundation 条款：项目 "must already be released in the form that should be signed"；可执行程序需 "a certain verifiable reputation"；所有组件须为 OSI 认可许可证、无商业双许可；允许附带上游开源项目的未签名二进制；Authors/Reviewers/Approvers 三种角色（未说明能否兼任）；全员 MFA；二进制须可验证地从源码构建；向用户未指定的系统传数据须有隐私政策、安装时展示、提供关闭选项；证书发给 SignPath Foundation，由其作为发布者。 | signpath.org/terms.html（2026-09-26） |
| F-102 | CC0 未获 OSI 批准（委员会未达成共识，Creative Commons 撤回申请）；OSI 不建议用 CC0 发布软件。 | opensource.org/faq（2026-09-26） |
| F-103 | CVE-2026-43893（GHSA-cw26-7653-2rp5）：npm `exiftool-vendored` ≤ 35.18.0 在把标签名、文件名/路径、`imageHashType`、`retain`/`numericTags` 条目等输入写成 stdin 参数行时未拒绝换行等行分隔符，可导致读写任意路径；传给 `ExifTool#write` 的**值**不受影响，因为其已把空白编码（`\n` → `&#10;`）；35.19.0 修复（逐输入校验 + 渲染层过滤 `\r \n \0`）；不是 ExifTool 本体漏洞。 | github.com/advisories/GHSA-cw26-7653-2rp5（2026-09-26） |
| F-104 | ExifTool 13.59 解码未知 protobuf 字段（如 Google HDR+ MakerNote，`-u`）时，把 `IsProtobuf` 标志记在**全局**标签表的动态标签上，且会由 1 改为 0（`Protobuf.pm` 164–180 行）；同一进程内第二次读取同一数据时递归更少、字段更少，`-stay_open` 的 `-execute` 之间同样保留。读过一次后状态不再变化（标志单调，第二次访问的记录是第一次的子集）。影响：写入前后两次读取的 V3 比较出现假差异（`t/images/Google.jpg` 每次写入都被拒绝）。处理：V3 报出差异时在同一进程内重读备份与临时文件再比较；真实差异仍会出现（e2e 在 Google.jpg 上注入多余标签验证）。可向上游报告 | 本机复现（2026-09-28，scratch 副本） |
| F-105 | ExifTool 13.59 Windows 包**本身**所在的文件夹名含系统 ANSI 代码页以外的字符时，两种调用方式都无法启动：官方 launcher 报 "Could not find …\exiftool_files\perl5*.dll"，`perl.exe exiftool.pl` 报 "Can't open perl script"，路径中的这些字符变成 `?`（二者都经 ANSI API 得到自身位置）。本机代码页 936 实测（2026-09-29，均为临时副本）：ASCII、空格、中文、日文假名正常；韩文、`Łukasz Żółć`、emoji 失败。照片与数据文件夹路径不受影响（经 UTF-8 参数文件传给 ExifTool；数据文件夹含 emoji 时全链路正常）。影响：按用户安装在 `%LOCALAPPDATA%\Programs` 下，用户名超出系统代码页时（如英文 Windows 上的中文用户名）ExifTool 无法启动。对策：路径不能按代码页精确表示时改用 8.3 短名（两种方式实测均正常）；卷不保留短名时拒绝启动并说明。D-17 不受影响（两种方式表现相同） | 本机 | `mm-core::engine::ansi_path`；e2e `exiftool_package_in_a_folder_outside_the_code_page` |

## 2. 从事实推出的设计约束（摘要）

| 约束 | 依据 |
|---|---|
| ExifTool 只写入**新临时文件**（`-o`），原文件在验证通过前不被触碰；提交用 `ReplaceFileW` 并总是提供 backup 名。 | F-04, F-05, F-14, F-50 |
| 所有路径以**绝对路径**经 **UTF-8 stdin argfile** 传递；不经命令行传任何不可信数据；值以 `-ex` + XML 字符引用编码，读取用 JSONQ。 | F-07, F-11, F-32, F-33, F-80–F-82 |
| 子进程：清空环境变量、`-config ""` 为第一个参数、工作目录为应用私有空目录、可执行文件名固定为 `exiftool.exe`。 | F-07, F-08, F-23, F-31 |
| 只允许注册表白名单中的标签；永久禁止文件系统伪标签；禁止 `<` 复制语法接收用户字符串；禁用 `-ee`、`-if`、`-p`、`-api filter`、`-geotag`。 | F-06, F-07, F-18 |
| 写入默认不加 `-m`；minor error 视为失败并如实展示。 | F-36 |
| IPTC 写入需显式处理字符集与字节长度；不存在 IPTC 时不新建（遵循 MWG 惯例）；写入 IPTC 时同步更新已有的 IPTCDigest；不使用 MWG Composite 写入。 | F-20, F-21, F-28, F-34, F-91–F-93 |
| IPTC 长度必须在 Plan 阶段按字节预检；minor 警告不能作为"写入成功"的依据，执行后必须重读比对。 | F-26, F-36 |
| lang-alt 字段一律写 `-x-default` 后缀。 | F-27 |
| 不能对 RAW 原片做就地隐私删除；MakerNotes 中的标识不能单独删除（只能置空或整体删除，均有副作用）。隐私功能的形式由 D-15 决定。 | F-04, F-17, F-95 |
| 只读属性必须由 MoriMeta 自行检查并尊重。 | F-13 |
| ExifTool 锁定"包含全部已知安全修复的最新版本"（当前 13.59），安全更新有 SLA；每次升级跑完整回归语料。 | F-01, F-05, F-06 |
| RAW 默认写 `<basename>.xmp`；JPEG/TIFF/DNG 写入文件内部（与 Adobe 一致）；不写 darktable 的 `.ext.xmp`；不碰 `.acr`。 | F-40, F-41, F-43 |
| 提交用 `ReplaceFileW`，bak 名预先登记、足够随机且提交前核对不存在；锁句柄 READ + 共享 READ\|DELETE；不用 POSIX 重命名替换。 | F-86–F-90 |

---

## 3. 验证登记表（Verification Register）

以下事项**未经验证，不得在实现中当作事实**。每项在对应 Spike 中关闭，结论回填本表。

| ID | 待验证事项 | 方法 | 阻塞 | Spike |
|---|---|---|---|---|
| V-01 | Rust 实现的 stay_open 协议：`{readyN}` + `-echo4` 标记、`${status}` 取值、stderr 并发消费、`#[CSTR]` 对 `\r \n \t \\ "`、首尾空白、NUL、超长值的往返正确性；进程崩溃/挂起检测。 | 单元 + 模糊测试 | MVP | S1 |
| V-02 | `ImageDataHash` 对 Nikon Z8/Z9（HE/HE*）、Z6III、D850 NEF、DNG、HEIC、JPEG（含 MPF/多图）的稳定性：同一文件多次计算一致、仅改元数据后不变。 | 真实语料 | MVP | S2 |
| V-03 | NEF + 最小化 `<basename>.xmp` 在 Lightroom Classic 15.x（含 `.acr` 并存）、ACR/Bridge、Capture One、NX Studio、darktable、digiKam、Photo Mechanic 中的可见性与冲突行为；哪些字段被读取。 | 兼容性实验室 | MVP | S3 |
| V-04 | `ReplaceFileW` 在 NTFS、exFAT、FAT32、SMB（NAS）、OneDrive/Dropbox 同步目录上的行为；目标文件被 Lightroom/资源管理器预览/杀软占用时的错误码；创建时间与 ADS 保留。 | 平台测试 | MVP | S2 |
| V-05 | 写入策略：MWG Composite vs 显式标签映射（IPTCDigest、IPTC 仅在存在时写、EXIF UTF-8）在 LR、Windows 资源管理器、Photos 中的显示效果。 | 兼容性实验室 | MVP | S3 |
| V-06 | 真实文件（JPEG 10–40 MB、NEF 25–80 MB）在 NVMe/SATA SSD/HDD/NAS 上的扫描吞吐与最佳 worker 数；5000 文件扫描与写入总耗时。 | 基准测试 | MVP | S4 |
| V-07 | NEF sidecar 中哪些时间字段会被 LR/C1 当作拍摄时间（`exif:DateTimeOriginal` / `photoshop:DateCreated` / `xmp:CreateDate`）；时区偏移的解释。 | 兼容性实验室 | MVP | S3 |
| V-08 | HEIC/AVIF/PNG/WebP 写入后在 Windows Photos、Apple Photos/Preview、Google Photos、浏览器中的兼容性（含 motion photo、gain map）。 | 兼容性实验室 | v1 写入 | 后续 |
| V-09 | Tauri + WebView2：5000 行 × 20 列虚拟表格滚动/排序性能、IME（中文输入）、屏幕阅读器（Narrator/NVDA）可用性；IPC 传输 5000×30 字段的耗时；Channel 进度事件频率。 | 原型 | MVP | S5 |
| V-10 | NSIS per-user 安装 35 MB/510 文件的 `exiftool_files` 资源；升级/卸载/回滚；Windows Defender 对打包 perl.exe 的行为；Updater 在"任务进行中"时的门控。 | 打包演练 | MVP | S6 |
| V-11 | ExifTool Windows 包（Strawberry Perl 各组件、`exiftool_files/LICENSE` 为 GPL-3）的再分发义务：需要附带哪些许可证文本、是否需要提供对应源码及方式。 | 许可证审查（建议法律意见） | 公开发布 | S6 |
| V-12 | 云占位符（OneDrive Files On-Demand 等）检测：`FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS`/`OFFLINE` 属性在各同步客户端中的表现；读取/写入是否触发下载与上传。 | 平台测试 | MVP | S2 |
| V-13 | 只读属性 + ReplaceFileW：确认替换行为，以及 MoriMeta 显式检查后的 UX。 | 平台测试 | MVP | S2 |
| V-14 | C2PA/JUMBF：Nikon/Leica/Sony 带 Content Credentials 的文件在 ExifTool 写入后的清单状态（保留但失效/被删除）；检测方法。 | 真实样本 | MVP（检测+警告） | S3 |
| V-15 | 用 `-o tmp.xmp existing.xmp` 改写已存在 sidecar 时，未知命名空间（crs、lr、自定义 schema、xmpMM）是否完整保留。 | 语料对比 | MVP | S3 |
| V-16 | SignPath Foundation 对"捆绑 ExifTool + Strawberry Perl"项目的资格认定。 | 咨询 | 公开发布 | S6 |
| V-17 | MSIX（社区工具）+ Store 路径可行性，包括对 `exiftool_files` 的文件系统访问、LocalAppData 虚拟化对备份库的影响。 | 打包演练 | 可选 | 后续 |
| V-18 | Rust→TS 类型生成（tauri-specta / ts-rs）对 Tauri 2.11 的支持成熟度。 | 原型 | MVP | S5 |
| V-19 | Nikon MakerNotes 中的序列号、所有者等标签在 JPEG 中能否"编辑为空"而不删除 MakerNotes；删除整个 MakerNotes 对 LR/NX Studio 显示镜头信息的影响。 | 真实样本 | MVP（隐私导出） | S3 |
| V-20 | 5000 文件批处理期间的内存占用（Rust 索引 + 前端数据）与 SQLite 日志写入延迟（`synchronous=FULL`）。 | 基准测试 | MVP | S4 |

---

### 3.1 验证状态（v0.3）

| ID | 状态 | 依据 / 剩余工作 |
|---|---|---|
| V-01 | **关闭** | S1：100,000/100,000；注入、伪造终止标记、崩溃、挂起通过 |
| V-02 | 部分 | JPEG、3 个 NEF（Z8 HE、Z8 无损、D850）稳定且改元数据后不变；Z6III、DNG、HEIC、MPF JPEG 未测 |
| V-03 | 未开始 | 需要第三方软件（D-13） |
| V-04 | 部分 | NTFS、SMB 回环完成；exFAT/FAT32、真实 NAS、云同步目录未做 |
| V-05 | 部分 | 显式映射已定（F-91）；Windows 资源管理器显示已核对（F-97）；LR/Photos 未做 |
| V-06 | 未开始 | S4 |
| V-07 | 未开始 | 需要第三方软件 |
| V-08 | 未开始 | v1 |
| V-09 | 未开始 | S5 |
| V-10 | 未开始 | S6 |
| V-11 | 未开始 | S6 + 法律意见 |
| V-12 | 未开始 | 需要可安全使用的云同步测试目录（D-13） |
| V-13 | **关闭** | ReplaceFileW 对只读原文件返回 5 且不改变磁盘；MoriMeta 预检跳过只读文件 |
| V-14 | 部分 | 合成 JUMBF 可检测、写入后原样保留；真实 C2PA 样本未测 |
| V-15 | 部分 | 合成 LR 风格 sidecar：RDF 属性全部保留，XML 注释丢弃（F-96）；真实 LR 15 sidecar 未测 |
| V-16 | 改写 | 由 V-21、V-22 取代 |
| V-17 | 未开始 | 可选 |
| V-18 | 未开始 | S5 |
| V-19 | 部分 | F-95（ExifTool 侧）；NX Studio/LR 对置空序列号的反应未测 |
| V-20 | 未开始 | S4 |
| V-21 | 新增，未开始 | SignPath 对 CC0 launcher 的态度（或采用调用方式 B） |
| V-22 | 新增，未开始 | SignPath 对单人兼任三种角色的态度；"已发布"是否接受以其他方式签名的首发 |
| V-23 | 新增，未开始 | 断电测试（虚拟机硬重置） |
| V-24 | 新增，部分 | Clean Export 真实样本扩充：真实 C2PA、HDR gain map、各品牌相机直出 JPEG（S7 已完成 53 个源） |
| V-25 | 新增，未开始 | 5,000 文件时持久化可执行 Plan 的体积与写入耗时 |
| V-26 | 新增，未开始 | 后端 updater 门禁与 Windows 安装时退出流程 |

## 4. 实验记录

- v0.2 阶段的实验在 `%TEMP%` 中完成，未记录脚本。
- v0.3 起所有实验都有可复现脚本与原型：`research/`（说明见 `research/README.md`），结果在 `research/results/`，汇总见 SPIKE_REPORT。
- 注意：本机 Git Bash / 工具链会对命令行中的非 ASCII 字符与反斜杠做转换，曾导致两次误判（v0.2 的中文值、v0.3 的 `#[CSTR]` 换行"拆分"——后者实为测试脚本本身传入了真实换行）。这再次说明：不可信或非 ASCII 数据不能经命令行或 shell 传递；实验脚本应以文件形式编写。

## 5. 参考链接

- ExifTool 主页 / 文档：https://exiftool.org/ ，https://exiftool.org/exiftool_pod.html ，https://exiftool.org/metafiles.html ，https://exiftool.org/faq.html ，https://exiftool.org/TagNames/MWG.html ，https://exiftool.org/TagNames/Extra.html ，https://exiftool.org/history.html ，https://exiftool.org/install.html
- CVE：https://nvd.nist.gov/vuln/detail/CVE-2022-23935 ，https://www.cvedetails.com/cve/CVE-2021-22204/ ，https://securelist.com/exiftool-compromise-mac/119866/ ，https://github.com/advisories/GHSA-qjrf-c4wp-pgxx
- Adobe：https://helpx.adobe.com/lightroom-classic/help/metadata-basics-actions.html ，https://helpx.adobe.com/lightroom-classic/help/create-xmp-acr-files.html
- darktable：https://docs.darktable.org/usermanual/development/en/overview/sidecar-files/sidecar/
- Microsoft：https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew ，https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options
- Tauri：https://v2.tauri.app/security/ ，https://v2.tauri.app/plugin/updater/ ，https://v2.tauri.app/develop/sidecar/ ，https://v2.tauri.app/distribute/windows-installer/ ，https://v2.tauri.app/distribute/sign/windows/ ，https://v2.tauri.app/release/
- 测试素材：https://raw.pixls.us/
- SignPath：https://signpath.org/terms.html ；OSI：https://opensource.org/faq ；CVE-2026-43893：https://github.com/advisories/GHSA-cw26-7653-2rp5
