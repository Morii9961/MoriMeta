# Code signing policy

English is the authoritative version; a Chinese translation follows.

**Status (2026-10-10): MoriMeta's builds are not code-signed yet.** The first public previews are published unsigned, with SHA-256 checksums and GitHub build attestations (DECISIONS D-2). The project intends to apply to [SignPath Foundation](https://signpath.org) for free code signing of open-source software. Until a certificate is granted, nothing on this page should be read as saying that a MoriMeta file is signed by SignPath Foundation.

Once the application is approved, signed releases will carry this attribution, here and on the release pages:

> Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org).

## What is signed

- The installer `MoriMeta_<version>_x64-setup.exe` and the program `morimeta.exe` it installs, both built from this repository by the [release workflow](../.github/workflows/release.yml) on GitHub Actions from a `v<version>` tag. Nothing built on a personal computer is signed.
- Not signed with MoriMeta's identity: the ExifTool package (`perl.exe`, its DLLs and scripts) is shipped unmodified from the official ExifTool Windows package and protected by a SHA-256 manifest that MoriMeta checks before running it (SECURITY_MODEL §5, RELEASE_PLAN §4.3).
- Update packages are additionally signed with the project's own Minisign key, which the app checks before installing an update ([docs/UPDATER_RELEASE.md](UPDATER_RELEASE.md)).

## Team roles

MoriMeta is maintained by one person. The roles that SignPath Foundation requires are held by:

| Role | Who | Responsibility |
|---|---|---|
| Committers and reviewers | [Morii9961](https://github.com/Morii9961) | Change the source code; review every pull request from anyone else before it is merged |
| Approvers | [Morii9961](https://github.com/Morii9961) | Approve each signing request, release by release |

Whether one maintainer may hold all roles is a question the project asks SignPath Foundation in its application (DECISIONS D-16); this table will change if the answer requires it. Every team member must use multi-factor authentication for GitHub and for SignPath; the maintainer confirms this before the application is sent.

## How a release is signed

1. A `v<version>` tag on `main`, where every required check has passed, starts the release workflow. It builds the installer from that commit, checks that the program uses and verifies its bundled ExifTool, and creates a draft release with the installer, the SBOM, the third-party notices, the ExifTool source and SHA-256 checksums.
2. Once signing is set up, the workflow submits that build to SignPath and an approver approves the signing request by hand; no release is signed automatically. The checksums and attestations are then made for the signed files that are published.
3. The release is published by hand after the checks in RELEASE_PLAN §11.

Changes to build scripts and CI configuration (`.github/`, `apps/desktop/scripts/`, `tools/`, `apps/desktop/src-tauri/tauri*.json`, `build.rs`) are reviewed with particular care, because they decide what is built and signed.

## Privacy

This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.

In detail ([PRIVACY.md](../PRIVACY.md)): MoriMeta has no telemetry, accounts or crash reporting. Its only network request is the optional update check, which is off until the user turns it on at first launch or in Settings. The installer downloads Microsoft's WebView2 Runtime only if it is missing. The WebView2 Runtime is a Microsoft component governed by the [Microsoft Privacy Statement](https://privacy.microsoft.com/privacystatement); update checks and downloads go to GitHub, governed by the [GitHub Privacy Statement](https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement).

## Licenses

MoriMeta is GPL-3.0-or-later. Every component it ships is under an OSI-approved open-source license, checked in CI by cargo-deny and listed in `THIRD_PARTY_NOTICES.md` with every release. The CC0-licensed ExifTool launcher is not shipped (DECISIONS D-17).

---

# 代码签名政策

以英文版为准。

**状态（2026-10-10）：MoriMeta 的构建尚未进行代码签名。** 首批公开预览版不签名发布，附 SHA-256 校验和与 GitHub 构建证明（DECISIONS D-2）。项目计划向 [SignPath Foundation](https://signpath.org) 申请面向开源软件的免费代码签名。在获得证书之前，本页任何内容都不表示某个 MoriMeta 文件已由 SignPath Foundation 签名。

申请获批后，签名版本将在本页和发布页标注：

> Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org).

## 签名范围

- 安装包 `MoriMeta_<版本>_x64-setup.exe` 及其安装的 `morimeta.exe`，均由 GitHub Actions 上的[发布工作流](../.github/workflows/release.yml)从 `v<版本>` 标签对应的本仓库源码构建。个人电脑上构建的文件一律不签名。
- 不以 MoriMeta 身份签名：ExifTool 包（`perl.exe`、其 DLL 与脚本）原样取自 ExifTool 官方 Windows 包，由 MoriMeta 在运行前核对的 SHA-256 清单保护（SECURITY_MODEL §5，RELEASE_PLAN §4.3）。
- 更新包另外使用项目自己的 Minisign 密钥签名，应用在安装更新前验证（[docs/UPDATER_RELEASE.md](UPDATER_RELEASE.md)）。

## 团队角色

MoriMeta 由一人维护。SignPath Foundation 要求的角色由以下人员担任：

| 角色 | 人员 | 职责 |
|---|---|---|
| 提交者与审核者 | [Morii9961](https://github.com/Morii9961) | 修改源码；合并他人的拉取请求之前逐一审核 |
| 批准者 | [Morii9961](https://github.com/Morii9961) | 逐个版本批准签名请求 |

一名维护者能否兼任所有角色，项目会在申请时向 SignPath Foundation 书面询问（DECISIONS D-16）；如答复要求，本表将相应调整。每位团队成员的 GitHub 与 SignPath 账户都必须启用多因素认证；维护者在提交申请前确认这一点。

## 签名流程

1. 在所有必过检查均已通过的 `main` 上打 `v<版本>` 标签，触发发布工作流：从该提交构建安装包，确认程序使用并校验了随附的 ExifTool，然后建立包含安装包、SBOM、第三方声明、ExifTool 源码和 SHA-256 校验和的草稿 Release。
2. 签名接入后，工作流把该构建提交给 SignPath，由批准者手动批准签名请求；任何版本都不会自动签名。校验和与构建证明随后针对最终发布的已签名文件生成。
3. 按 RELEASE_PLAN §11 检查后手动发布。

构建脚本与 CI 配置（`.github/`、`apps/desktop/scripts/`、`tools/`、`apps/desktop/src-tauri/tauri*.json`、`build.rs`）决定了构建和签名的内容，审核时格外仔细。

## 隐私

This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.（除非用户或安装、操作本程序的人明确要求，本程序不会向其他网络系统传输任何信息。）

详见 [PRIVACY.md](../PRIVACY.md)：MoriMeta 没有遥测、账户或崩溃上报。唯一的网络请求是可选的检查更新，在用户于首次启动或设置中开启之前一直关闭。只有在系统缺少 Microsoft WebView2 运行时时，安装程序才会下载它。WebView2 运行时是 Microsoft 的组件，适用 [Microsoft 隐私声明](https://privacy.microsoft.com/privacystatement)；检查更新与下载访问 GitHub，适用 [GitHub 隐私声明](https://docs.github.com/site-policy/privacy-policies/github-general-privacy-statement)。

## 许可证

MoriMeta 采用 GPL-3.0-or-later。随附的所有组件均采用 OSI 认可的开源许可证，由 CI 中的 cargo-deny 检查，并在每个版本附带的 `THIRD_PARTY_NOTICES.md` 中列出。不随附采用 CC0 的 ExifTool 启动器（DECISIONS D-17）。
