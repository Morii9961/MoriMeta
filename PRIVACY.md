# Privacy

English is the authoritative version; a Chinese translation follows.

## In short

MoriMeta runs on your computer only. It has no account, no telemetry, no analytics and no crash reporting. Nothing about your photos, their metadata or your use of the app is sent anywhere.

## Network requests

MoriMeta makes exactly one kind of network request, and only if you allow it:

- **Update check** (Settings › Updates, asked once at first launch with nothing pre-selected). When you choose weekly checks or click *Check now*, MoriMeta requests `https://github.com/Morii9961/MoriMeta/releases/latest/download/latest.json`. The request carries no information about you, your photos or your installation; its user agent names only the update library (`tauri-plugin-updater/<version>`). If you then choose to download an update, the installer is fetched from the same GitHub release. GitHub receives your IP address as with any web request; see GitHub's privacy statement.
- A build made without an update signing key never makes any request.

The installer downloads Microsoft's WebView2 Runtime, which draws the app's window, only if Windows does not have it yet. The runtime is a Microsoft component and follows the [Microsoft Privacy Statement](https://privacy.microsoft.com/privacystatement) and your Windows diagnostic-data settings; MoriMeta gives it no information about your photos.

Opening a folder on a network drive or a cloud-synced folder makes Windows (or the sync client) transfer those files; that is your storage, not a service of MoriMeta.

## What MoriMeta stores on your computer

Everything lives in `%LOCALAPPDATA%\MoriMeta` unless you choose another backup location:

| Folder / file | Contents |
|---|---|
| `db\` | The Journal: History of every operation, with file paths and the metadata values before and after; your settings and presets |
| `backups\` (or your backup location) | A full copy of every file before MoriMeta changed it, so each operation can be undone. Backups are never deleted without you: Settings › Backup proposes a clean-up by the retention policy (older than 30 days, or beyond 10 % of the drive; the 10 most recent operations and those you keep are protected) and you confirm it |
| `logs\` | Daily program logs, kept 7 days / 50 MB. Files appear as `asset#n.ext`; no path, file name, user name or metadata value is written. The optional debug log (Settings › Advanced) ends by itself after 24 hours and shows ExifTool commands with values cut to 12 characters |
| `backup-locations.txt` | The backup locations used, so History can be rebuilt if the database is lost |
| `run\` | Temporary working files and the lock that keeps a second MoriMeta from using the same data |

The WebView (the app's window) keeps its own cache in `%LOCALAPPDATA%\org.morimeta.app`.

## Sharing a log

*Export log…* in History writes a report of one operation. Paths become `asset#n.ext` and metadata values are left out unless you tick the options to include them. Check the file before you attach it to a bug report.

## Deleting your data

Uninstalling MoriMeta keeps `%LOCALAPPDATA%\MoriMeta` and any backup location you chose, because the backups are what Undo restores from. To remove everything, uninstall MoriMeta and delete those folders yourself. Deleting the backups makes earlier operations impossible to undo.

---

# 隐私说明

以英文版为准。

## 概要

MoriMeta 只在你的电脑上运行。没有账户、没有遥测、没有统计分析、没有崩溃上报。照片、元数据以及你的使用情况都不会发送到任何地方。

## 网络请求

MoriMeta 只会发出一种网络请求，而且只有在你允许时才会发出：

- **检查更新**（设置 › 更新；首次启动时询问一次，默认不勾选任何选项）。选择每周检查或点击“立即检查”后，MoriMeta 会请求 `https://github.com/Morii9961/MoriMeta/releases/latest/download/latest.json`。请求中不包含任何关于你、你的照片或你的安装的信息；User-Agent 只写明更新组件（`tauri-plugin-updater/<版本>`）。如果你随后选择下载更新，安装包同样从该 GitHub Release 下载。和任何网页请求一样，GitHub 会看到你的 IP 地址，详见 GitHub 的隐私声明。
- 未配置更新签名密钥的构建不会发出任何请求。

只有在 Windows 尚未安装时，安装程序才会下载用于绘制应用窗口的 Microsoft WebView2 运行时。该运行时是 Microsoft 的组件，适用 [Microsoft 隐私声明](https://privacy.microsoft.com/privacystatement)和你的 Windows 诊断数据设置；MoriMeta 不向它提供任何关于你照片的信息。

打开网络驱动器或云同步文件夹中的照片时，文件传输由 Windows 或同步客户端完成，属于你自己的存储，不是 MoriMeta 的服务。

## MoriMeta 在本机保存的内容

除非你另选备份位置，所有内容都在 `%LOCALAPPDATA%\MoriMeta`：

| 文件夹 / 文件 | 内容 |
|---|---|
| `db\` | Journal：每次操作的历史，包括文件路径以及修改前后的元数据值；你的设置和预设 |
| `backups\`（或你选择的备份位置） | 每个文件被修改前的完整副本，用于撤销。备份不会在你不知情时被删除：设置 › 备份会按保留策略提出清理建议（超过 30 天，或超出所在磁盘的 10%；最近 10 次操作和你标记保留的操作受保护），由你确认后才清理 |
| `logs\` | 每日程序日志，保留 7 天 / 50 MB。文件以 `asset#n.ext` 表示；不写入路径、文件名、用户名或元数据值。可选的调试日志（设置 › 高级）24 小时后自动关闭，其中 ExifTool 命令的值截断为 12 个字符 |
| `backup-locations.txt` | 使用过的备份位置，数据库丢失时用于重建历史 |
| `run\` | 临时工作文件，以及防止两个 MoriMeta 同时使用同一数据的锁 |

应用窗口（WebView）的缓存保存在 `%LOCALAPPDATA%\org.morimeta.app`。

## 分享日志

历史中的“导出日志…”为一次操作生成报告。路径会变成 `asset#n.ext`，元数据值默认不包含，除非你勾选相应选项。附到问题反馈之前，请先检查文件内容。

## 删除数据

卸载 MoriMeta 时会保留 `%LOCALAPPDATA%\MoriMeta` 和你选择的备份位置，因为撤销需要用到这些备份。如需全部删除，请先卸载 MoriMeta，再手动删除这些文件夹。删除备份后，之前的操作将无法撤销。
