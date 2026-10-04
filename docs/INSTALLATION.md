# Installing MoriMeta

English is the authoritative version; a Chinese translation follows.

> MoriMeta is pre-release software. Work on copies of your photos until a stable version is out.

## Requirements

- Windows 11, or Windows 10 22H2; x64 (ARM64 devices run the x64 build through emulation).
- Microsoft Edge WebView2 Runtime. Windows 10 and 11 normally have it; the installer can fetch it if it is missing.
- About 60–80 MB for the program (ExifTool included), plus room for backups (see below).
- No administrator rights. MoriMeta refuses to write files when it runs as administrator.

## Install

1. Download `MoriMeta_<version>_x64-setup.exe` from the project's GitHub Releases page.
2. Optionally compare its SHA-256 with the one published on the release page:

   ```powershell
   Get-FileHash .\MoriMeta_<version>_x64-setup.exe -Algorithm SHA256
   ```

3. Run it. It installs for your user account only, in `%LOCALAPPDATA%\MoriMeta`, without asking for administrator rights.

### Windows SmartScreen

Early preview builds are not code-signed yet (DECISIONS D-2). Windows may then show *Windows protected your PC*. Check the file's SHA-256 first; then choose *More info* › *Run anyway*. Signed builds will carry the publisher's name.

## First launch

MoriMeta asks three things: the interface language, where backups go (the default is `%LOCALAPPDATA%\MoriMeta\backups`; it must be a local, writable folder), and whether to check for updates (nothing is pre-selected). All three can be changed later in Settings.

## Where your data is

| Location | Contents |
|---|---|
| `%LOCALAPPDATA%\MoriMeta` | The program (`morimeta.exe`, the `exiftool` package) and, beside it, History (the Journal, in `db`), settings, presets, logs and, by default, backups |
| The backup location you chose | Backups of every file before MoriMeta changed it |
| `%LOCALAPPDATA%\org.morimeta.app` | The window's WebView cache |

Backups need space: each operation keeps a full copy of every file it changes. Settings › Backup shows how much they use and lets you clean up old ones; nothing is deleted without your confirmation. See [PRIVACY.md](../PRIVACY.md) for what each file contains.

## Updating

If you allowed update checks, MoriMeta tells you when a new version exists. You decide when to download it and when to install it; it never installs while an operation runs. Updates are verified with the project's signing key before they are installed. You can also install a newer version over the old one by running its installer.

## Uninstalling

Use *Settings › Apps › Installed apps › MoriMeta › Uninstall* in Windows. The uninstaller removes only the program's own files. It keeps your data in `%LOCALAPPDATA%\MoriMeta` and your backup location, because they hold your History and the backups Undo needs; the checkbox *Delete the application data* only removes the WebView cache.

To remove everything, delete `%LOCALAPPDATA%\MoriMeta` and your backup location yourself after uninstalling. Earlier operations can then no longer be undone.

## Moving to another computer or reinstalling Windows

Copy `%LOCALAPPDATA%\MoriMeta` and your backup location. If only the backups survived, install MoriMeta and use *Settings › Advanced › Find history in a backup folder…*: operations are read back from the backups and can be undone again.

---

# 安装 MoriMeta

以英文版为准。

> MoriMeta 仍是预发布软件。在正式版发布之前，请只处理照片的副本。

## 系统要求

- Windows 11，或 Windows 10 22H2；x64（ARM64 设备通过仿真运行 x64 版本）。
- Microsoft Edge WebView2 运行时。Windows 10 和 11 通常已自带；如果缺失，安装程序可以下载。
- 程序本身约 60–80 MB（含 ExifTool），另需备份空间（见下文）。
- 不需要管理员权限。以管理员身份运行时，MoriMeta 拒绝写入文件。

## 安装

1. 从项目的 GitHub Releases 页面下载 `MoriMeta_<版本>_x64-setup.exe`。
2. 可选：将其 SHA-256 与发布页上公布的值比对：

   ```powershell
   Get-FileHash .\MoriMeta_<版本>_x64-setup.exe -Algorithm SHA256
   ```

3. 运行安装程序。它只为当前用户安装到 `%LOCALAPPDATA%\MoriMeta`，不需要管理员权限。

### Windows SmartScreen

早期预览版尚未进行代码签名（DECISIONS D-2）。Windows 可能显示“Windows 已保护你的电脑”。请先核对文件的 SHA-256，然后选择“更多信息” › “仍要运行”。签名版本会显示发布者名称。

## 首次启动

MoriMeta 会询问三件事：界面语言；备份保存位置（默认 `%LOCALAPPDATA%\MoriMeta\backups`，必须是本地可写的文件夹）；是否检查更新（默认不选）。三项都可以之后在设置中修改。

## 数据位置

| 位置 | 内容 |
|---|---|
| `%LOCALAPPDATA%\MoriMeta` | 程序（`morimeta.exe`、`exiftool` 包），以及与之并列的历史（Journal，在 `db` 中）、设置、预设、日志，默认还有备份 |
| 你选择的备份位置 | 每个文件被修改前的备份 |
| `%LOCALAPPDATA%\org.morimeta.app` | 窗口的 WebView 缓存 |

备份需要空间：每次操作都会保留被修改文件的完整副本。设置 › 备份显示占用情况，并可清理旧备份；未经你确认不会删除任何内容。各文件的内容见 [PRIVACY.md](../PRIVACY.md)。

## 更新

如果你允许检查更新，MoriMeta 会在有新版本时提示你。何时下载、何时安装由你决定；操作进行中不会安装。安装前会用项目的签名密钥验证更新。你也可以直接运行新版本的安装程序覆盖安装。

## 卸载

在 Windows 的“设置 › 应用 › 已安装的应用 › MoriMeta › 卸载”中卸载。卸载程序只删除程序自己的文件，保留 `%LOCALAPPDATA%\MoriMeta` 中的数据和你的备份位置，因为其中保存着历史以及撤销所需的备份；“删除应用程序数据”复选框只删除 WebView 缓存。

如需全部删除，请在卸载后手动删除 `%LOCALAPPDATA%\MoriMeta` 和你的备份位置。之后将无法撤销以前的操作。

## 迁移到其他电脑或重装 Windows

复制 `%LOCALAPPDATA%\MoriMeta` 和你的备份位置。如果只剩下备份，安装 MoriMeta 后使用“设置 › 高级 › 从备份文件夹找回历史…”：操作会从备份中读回，并可以再次撤销。
