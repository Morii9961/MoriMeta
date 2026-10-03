# 更新产物的离线准备

2026-10-03。此流程只准备本地产物；不会联网上传、修改 GitHub Release 或自动发布。

应用和 `update-manifest` 使用同一验签函数。它们接受 Minisign 默认的预哈希签名（`ED`），校验全局签名后，要求 trusted comment 中唯一的 `version:` 字段与清单版本一致，且比当前版本新。没有版本字段、旧版算法、篡改内容/注释、同版或降级均拒绝。

Minisign 的 trusted comment 会被签名；官方文档介绍了 `-t`、默认预哈希格式和验证方式：[Minisign](https://jedisct1.github.io/minisign/)。Tauri 清单结构见[官方更新文档](https://v2.tauri.app/plugin/updater/)。

## 1. 配置构建

正式签名密钥由发布者保管，不写入仓库，不作为命令行密码，也不打印到日志。此脚本不创建或读取私钥。应用公钥是公钥文本文件整体的 Base64；私钥与公钥须来自同一 Minisign 密钥对。

在构建应用之前设置：

```powershell
$env:MORIMETA_UPDATER_PUBLIC_KEY = [Convert]::ToBase64String([IO.File]::ReadAllBytes('release.pub'))
```

没有该变量的构建不发起更新请求。它与 Windows Authenticode 签名是两个独立用途：MVP 的签名身份与发布要求仍按 DECISIONS D-2 执行。先完成最终安装包（包括所需的 Windows 代码签名），再做下面的 Minisign 签名；签名后不能再改安装包。

应用版本须同时更新桌面 `Cargo.toml` 与 `tauri.conf.json`，与签名版本一致。安装包使用 ASCII 文件名，以 `.exe` 结尾，最多 128 MiB。

## 2. 签名最终安装包

发布者使用官方 Minisign，并在其交互提示中解锁密钥。示例版本须替换为实际版本：

```powershell
$comment = "timestamp:$([DateTimeOffset]::UtcNow.ToUnixTimeSeconds())`tversion:0.2.0"
minisign -S -m MoriMeta_0.2.0_x64-setup.exe -s release.key -x installer.minisig -t $comment
```

不能使用 `-l`，也不能只用默认的时间注释。签名密码不传给本工具。保存版本字段和签名，避免把真实旧安装包配上虚高的清单版本。

## 3. 验证并生成清单

在仓库根目录运行，参数依次是安装包、原始 Minisign 签名文本、公钥文本、目标版本、上一版版本、说明文件和输出文件：

```powershell
cargo run --manifest-path apps/desktop/src-tauri/Cargo.toml --bin update-manifest -- `
  MoriMeta_0.2.0_x64-setup.exe installer.minisig release.pub `
  0.2.0 0.1.0 release-notes.txt latest.json
```

输出包含 `windows-x86_64` 平台、验过的签名和本项目 HTTPS 下载地址。工具先完整写入并刷新 `.partial`，再以不覆盖方式取得最终名称。已有输出或 `.partial` 会拒绝；失败时不要将 `.partial` 发布。工具不加载私钥、不签名、不上传，也不改现有清单。

发布 URL 固定使用 `v<version>` 标签和安装包原文件名；实际 Release 必须使用这个标签并上传同名文件。清单不能先于安装包对外更新。上传、逐项核对、发布和 S6 真机更新/安装测试由发布流程单独执行。单元测试中的 `.exe` 只是合成文本载荷，绝不是安装验收。

## 4. 尚待发布验收

正式公钥与密钥保管、最终安装包的签名、Win10/Win11 上从上一 Stable/Beta 升级、网络失败/中断下载、操作进行中拒绝安装，以及实际安装后的数据保留均需验收。本地测试不证明这些结果。
