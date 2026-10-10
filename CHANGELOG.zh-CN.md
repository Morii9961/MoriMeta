# 更新记录

与英文版 [CHANGELOG.md](CHANGELOG.md) 逐版本对应，以英文版为准。发布时，同一版本的中文段落会附在发布说明的英文内容之后。

## [Unreleased]

尚未发布任何版本。首个公开预览版不做代码签名（DECISIONS D-2），届时会在此列出包含的内容和尚未验证的方面（DECISIONS D-13）。

### 新增

- 安全写入元数据的核心：每次写入都先规划、预览、备份、校验（V1–V6），并记录在能承受崩溃的 Journal 中；中断的操作在下次启动时恢复；每次操作都能逐字节撤销。
- 字段：作者、版权、拍摄时间（绝对时间、平移、序列、保持相对间隔）、GPS（设置、移除）；模板变量；规则与预设，可导入导出。
- JPEG 就地写入；NEF/NRW 从不写入（修改写入 XMP sidecar）；TIFF、PNG、HEIC/HEIF、AVIF、WebP、DNG 及其他 RAW 格式只读。
- 干净导出：生成 JPEG 副本，按你的选择移除位置、序列号等隐私元数据，导出前逐项列出将移除的内容；原文件不变。
- 桌面应用（Tauri + React），英文与简体中文界面：资料库、检查器、批量编辑、时间工具、可排除与确认的预览、可安全取消的进度、可撤销/重试/恢复到文件夹的历史、恢复、设置、首次启动、备份管理、可选的签名更新。
- 设置 › 高级 › 从备份文件夹找回历史…，用于数据文件夹丢失的情况。
- 帮助 › 关于 MoriMeta：问题反馈所需的各项版本与第三方声明。
- PRIVACY.md、docs/INSTALLATION.md、Issue 模板；CI 中的依赖审计（cargo-deny、npm audit）。
- 发布工作流：推送版本标签后在 GitHub Actions 上构建安装包，并建立草稿 Release，附 SHA-256 校验和、CycloneDX SBOM、第三方声明、ExifTool 源码和 GitHub 构建证明。

### 已知限制

- 尚未验证：第三方软件的读取（Lightroom、Capture One、NX Studio）、断电、真实 SD 卡与 NAS、云同步客户端、5,000 个文件的相机语料（DECISIONS D-13）。
- 安装包与更新安装尚未在 Windows 10/11 真机上测试（S6）。
