# MoriMeta user guide

English is the authoritative version; a Chinese translation follows.

> MoriMeta is pre-release software. Work on copies of your photos until a stable version is out. Installation: [INSTALLATION.md](INSTALLATION.md). Privacy: [PRIVACY.md](../PRIVACY.md).

## What MoriMeta does

MoriMeta edits four fields of your photos' metadata in batches: **Creator**, **Copyright**, **capture time** and **GPS position**. Every change is shown in a Preview before anything is written; every file is backed up first, checked after writing, and can be undone.

| Format | What happens |
|---|---|
| JPEG | Written in the file |
| NEF, NRW (Nikon RAW) | Never written. Edits go to an XMP sidecar next to the RAW (`DSC_0001.NEF` → `DSC_0001.xmp`), created when it does not exist |
| XMP picked on its own | Written in the file, with a warning when no photo of that name is next to it |
| TIFF, PNG, HEIC/HEIF, AVIF, WebP, DNG, other RAW formats | Read-only: shown in the Inspector, listed as Unsupported in a Plan |

## The workflow

1. **Add photos.** *Add › Add folder…* (Ctrl Shift O), *Add files…* (Ctrl O), or drag them onto the window. Nothing is written by adding; the Library reads each file's metadata.
2. **Find and select.** Sort by clicking a header (Shift-click adds a level), filter with the facets on the left, the search field, or *+ Add condition* above the table; save useful conditions as smart filters. *Columns…* and a right-click on a header change the table, including grouping rows by a column.
3. **Look at a file.** With one file selected, the Inspector shows each field's value and where it comes from (EXIF, XMP, IPTC, sidecar). *Effective* is what MoriMeta and most programs use; *In file* and *Sidecar* show each source.
4. **Stage edits.** With several files selected, the batch editor shows each field as *Leave*, *Set* or *Clear*. Time tools (T) change capture times; Presets apply saved rules. Nothing is written yet.
5. **Preview.** *Preview* (Ctrl Enter) shows every file and change. Leave out a file, one change or a whole edit; review Warnings, Unsupported and Removals; the pre-flight checks the backup location, free space and whether a file changed since it was read.
6. **Apply.** MoriMeta backs up each file, writes a temporary copy, reads it back and compares it with the plan; only a copy that matches replaces the file, so a file that fails the check stays as it was. *Cancel* stops after the current file.
7. **Check and undo.** The completion summary lists what was done. *History* keeps every operation: *Undo…* (all or selected files, conflicts shown first), *Retry failed*, *Restore backup to folder…*, *Export log…*.

## Fields

- **Creator**: one field written to EXIF Artist, XMP creator and an existing IPTC By-line together. Several names are separated by `; `.
- **Copyright**: EXIF Copyright, XMP rights and an existing IPTC copyright notice. Templates can use `{year}`, `{month}`, `{day}`, `{camera}`, `{lens}`, `{filename}`, `{folder}` and `{creator}`; `{creator|Morii}` gives a default when the file has none. A file where a variable has no value and no default is blocked, not guessed.
- **Capture time**: *Absolute* (one time for all; their order by time is lost, and the Preview warns), *Shift* (add or subtract), *Sequence* (start + step in time or name order; a RAW and its JPG share a position), *Preserve relative timing* (set one reference file, the others keep their spacing). A file's UTC offset is never changed, and GPS time is never moved.
- **GPS**: *Set* accepts decimal degrees (`35.6586, 139.7454[, altitude]`) or degrees, minutes and seconds (`35°39′31″N 139°44′43″E`). *Remove* deletes the GPS position; for a RAW it cannot be removed through the sidecar and shows as Unsupported (use Clean Export for a copy without it).

## Presets and rules

A preset is a list of rules: *IF* conditions (all must hold) *THEN* actions. Rules run top to bottom against each file as it is now; when two rules set the same field, the later one wins. The rule builder's *Dry run* tests the preset you are editing against some files without writing or keeping a plan. *Apply…* lets you choose the selected files, the files the filter shows, or the whole session, and opens the Preview. An imported preset is marked until its first use.

## Clean Export

*Clean export…* makes JPEG copies in a folder you choose, without the metadata you choose to remove (location, serial numbers, maker notes and more). Every removed tag and segment is listed before export; the originals are never changed.

## Backups, undo and recovery

- Backups are full copies of each file before it was changed, in `%LOCALAPPDATA%\MoriMeta\backups` or the location you choose (it must be local and writable). They cannot be switched off.
- Settings › Backup shows their size. Cleaning up is proposed by the retention policy (30 days, 10 % of the drive; the 10 most recent operations and any you keep are protected) and happens only after you confirm. Removing a backup means that operation can no longer be undone.
- If MoriMeta or Windows stops during an operation, the next launch finishes or rolls back each file from the record and asks how to continue: *Continue remaining*, *Undo the completed…*, or *Keep as is*.
- If the data folder is lost but the backups survived, *Settings › Advanced › Find history in a backup folder…* brings the operations back so they can be undone.

## RAW files and other programs

MoriMeta never writes inside a RAW file. Programs that read an XMP sidecar named like the RAW show the edits; programs that read only the RAW file (for example the camera) do not. Which programs read it has not been verified yet. A darktable sidecar (`DSC_0001.NEF.xmp`) is read only and never written.

## When a file is not written

The Preview gives the reason for every file it will not write. The common ones:

- the read-only attribute is set (the Inspector offers *Clear read-only attribute…*);
- the file is a cloud placeholder or in a cloud-synced folder, on removable media, or on a drive that is not NTFS;
- the file is a link, or has C2PA Content Credentials;
- the file has more than one IPTC record, or its content is not what its extension says;
- a template variable has no value;
- MoriMeta runs as administrator (it never writes then; start it normally).

## Keyboard

Ctrl O / Ctrl Shift O add files / a folder · Ctrl A select all · arrows, Shift and Ctrl to move and extend the selection · T time tools · Ctrl Enter Preview · Esc back · I inspector · Ctrl B sidebar · Ctrl F search · Ctrl S save a preset · Alt ↑/↓ move a rule · Ctrl , Settings.

---

# MoriMeta 使用指南

以英文版为准。

> MoriMeta 仍是预发布软件。在正式版发布之前，请只处理照片的副本。安装见 [INSTALLATION.md](INSTALLATION.md)，隐私见 [PRIVACY.md](../PRIVACY.md)。

## MoriMeta 做什么

MoriMeta 批量编辑照片元数据中的四个字段：**作者**、**版权**、**拍摄时间**和 **GPS 位置**。写入任何内容之前都会在预览中展示每一处修改；每个文件写入前先备份，写入后核验，并且可以撤销。

| 格式 | 处理方式 |
|---|---|
| JPEG | 在文件内写入 |
| NEF、NRW（尼康 RAW） | 从不写入。修改写入 RAW 旁边的 XMP sidecar（`DSC_0001.NEF` → `DSC_0001.xmp`），没有时新建 |
| 单独选择的 XMP | 在文件内写入；旁边没有同名照片时给出警告 |
| TIFF、PNG、HEIC/HEIF、AVIF、WebP、DNG 及其他 RAW 格式 | 只读：可在检查器中查看，在计划中列为不支持 |

## 工作流程

1. **添加照片。** “添加 › 添加文件夹…”（Ctrl Shift O）、“添加文件…”（Ctrl O），或直接拖入窗口。添加不会写入任何内容，资料库只读取每个文件的元数据。
2. **查找与选择。** 点击表头排序（Shift 点击添加次级排序），用左侧分面、搜索框或表格上方的“+ 添加条件”筛选，常用条件可保存为智能筛选。“列…”和表头右键菜单可调整表格，包括按某一列分组。
3. **查看文件。** 选中一个文件时，检查器显示每个字段的值及其来源（EXIF、XMP、IPTC、sidecar）。“生效值”是 MoriMeta 和大多数软件采用的值，“文件内”和“Sidecar”分别显示各来源。
4. **暂存修改。** 选中多个文件时，批量编辑器中每个字段可选“保持”“设置”或“清除”。时间工具（T）修改拍摄时间，预设应用已保存的规则。此时仍未写入任何内容。
5. **预览。** “预览”（Ctrl Enter）列出每个文件和每处修改。可以排除某个文件、某一处修改或整项编辑；逐类检查警告、不支持和移除；预检会核对备份位置、可用空间，以及文件在读取后是否被改动。
6. **应用。** MoriMeta 先备份每个文件，写入临时副本，读回并与计划比对；只有一致的副本才会替换原文件，未通过核验的文件保持原样。“取消”会在当前文件完成后停止。
7. **检查与撤销。** 完成摘要列出结果。“历史”保留每一次操作：“撤销…”（全部或所选文件，先列出冲突）、“重试失败项”、“将备份恢复到文件夹…”、“导出日志…”。

## 字段

- **作者**：一个字段，同时写入 EXIF Artist、XMP creator 和已有的 IPTC By-line。多个名字用 `; ` 分隔。
- **版权**：EXIF Copyright、XMP rights 和已有的 IPTC 版权声明。模板可使用 `{year}`、`{month}`、`{day}`、`{camera}`、`{lens}`、`{filename}`、`{folder}` 和 `{creator}`；`{creator|Morii}` 表示文件没有值时使用默认值。变量没有值也没有默认值时，该文件会被阻止，而不是猜测。
- **拍摄时间**：“绝对时间”（所有文件设为同一时间，按时间的顺序会丢失，预览中会警告）、“平移”（加上或减去）、“序列”（起始时间 + 步长，按时间或文件名排序；RAW 与同名 JPG 共用一个位置）、“保持相对间隔”（设定一个参照文件，其他文件保持间隔）。文件的 UTC 偏移从不修改，GPS 时间也从不移动。
- **GPS**：“设置”接受十进制度数（`35.6586, 139.7454[, 海拔]`）或度分秒（`35°39′31″N 139°44′43″E`）。“移除”删除 GPS 位置；RAW 文件内的 GPS 无法通过 sidecar 移除，会显示为不支持（可用干净导出生成不含位置的副本）。

## 预设与规则

预设由若干规则组成：“如果”条件（全部满足）“则”动作。规则自上而下按每个文件当前的内容执行；两条规则设置同一字段时，以后者为准。规则构建器的“试运行”会用正在编辑的预设测试部分文件，不写入、也不保留计划。“应用…”可选择所选文件、筛选显示的文件或整个会话，然后打开预览。导入的预设在首次使用前会被标记。

## 干净导出

“干净导出…”在你选择的文件夹中生成 JPEG 副本，去除你选择移除的元数据（位置、序列号、厂商注释等）。导出前会列出每个被移除的标签和数据段；原文件从不改动。

## 备份、撤销与恢复

- 备份是每个文件修改前的完整副本，保存在 `%LOCALAPPDATA%\MoriMeta\backups` 或你选择的位置（必须是本地可写的文件夹）。备份无法关闭。
- 设置 › 备份显示占用的空间。清理由保留策略提出建议（30 天、所在磁盘的 10%；最近 10 次操作及你标记保留的操作受保护），经你确认后才执行。删除某次操作的备份后，该操作将无法撤销。
- 如果操作过程中 MoriMeta 或 Windows 停止运行，下次启动时会根据记录完成或回滚每个文件，并询问如何继续：“继续剩余部分”“撤销已完成的…”或“保持现状”。
- 如果数据文件夹丢失但备份仍在，可在“设置 › 高级 › 从备份文件夹找回历史…”中找回操作，并可再次撤销。

## RAW 文件与其他软件

MoriMeta 从不写入 RAW 文件本身。读取与 RAW 同名的 XMP sidecar 的软件会显示这些修改；只读取 RAW 文件本身的设备或软件（例如相机）不会显示。哪些软件会读取尚未验证。darktable 的 sidecar（`DSC_0001.NEF.xmp`）只读，从不写入。

## 文件未被写入时

预览会说明每个不写入的文件的原因。常见原因：

- 文件设置了只读属性（检查器提供“清除只读属性…”）；
- 文件是云端占位符或位于云同步文件夹、位于可移动介质，或所在磁盘不是 NTFS；
- 文件是链接，或带有 C2PA 内容凭据；
- 文件包含多条 IPTC 记录，或其内容与扩展名不符；
- 模板变量没有值；
- MoriMeta 以管理员身份运行（此时从不写入，请正常启动）。

## 键盘

Ctrl O / Ctrl Shift O 添加文件 / 文件夹 · Ctrl A 全选 · 方向键配合 Shift、Ctrl 移动和扩展选择 · T 时间工具 · Ctrl Enter 预览 · Esc 返回 · I 检查器 · Ctrl B 侧栏 · Ctrl F 搜索 · Ctrl S 保存预设 · Alt ↑/↓ 移动规则 · Ctrl , 设置。
