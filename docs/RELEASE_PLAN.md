# MoriMeta — Release Plan (Windows first)

> **Version:** 0.3 · **Status:** 草案（未批准）· **Date:** 2026-09-26
> 已确定：MoriMeta 公开开源。待决：具体许可证（D-1）、签名路线（D-2）、发布者身份（D-3）、SignPath 团队角色（D-16）、ExifTool 调用方式（D-17）。本文不做任何公开发布、签名申请或证书购买。
> 本文定义公开发布所需的安装、签名、更新、版本、许可证与发布流程。
> `[F-xx]` / `[V-xx]` 引用 [RESEARCH_NOTES](RESEARCH_NOTES.md)。

---

## 1. 目标平台

| 项目 | 1.0 | 说明 |
|---|---|---|
| 操作系统 | Windows 11；Windows 10 22H2 | Win10 已结束主流支持，但仍有大量用户；以实际测试矩阵为准（D-11） |
| 架构 | x64 | ARM64 设备通过仿真运行 x64 构建；ExifTool 官方包为 32/64 位 x86 [F-23] |
| WebView2 | Evergreen（Win10/11 随系统分发 [F-64]） | 安装器嵌入 bootstrapper 兜底 |
| 磁盘 | 安装约 60–80 MB（其中 ExifTool 约 35 MB [F-30]） + 备份空间 | 备份空间由用户数据量决定 |
| 权限 | 普通用户；不需要管理员 | 以管理员运行时禁用写操作（SECURITY_MODEL §4.1） |

---

## 2. 安装器

- **NSIS**（Tauri 默认），`installMode: currentUser`：安装到 `%LOCALAPPDATA%`，无需管理员 [F-64]。
- MSI（WiX）在 1.x 视企业需求提供（仅能在 Windows 上构建 [F-64]）。
- WebView2：`embedBootstrapper`（+1.8 MB），在极少数缺失 WebView2 的系统上可安装；不使用 offline/fixed（体积 127–180 MB，且 fixed 版本需要我们自己追安全更新）。
- 资源：`resources/exiftool/`（`exiftool_files/` + `exiftool.manifest` + 许可证文件；采用调用方式 A 时另含重命名后的 `exiftool.exe`，方式 B 时不含 launcher，见 ARCHITECTURE ADR-03）。以 `bundle.resources` 打包，**不使用 externalBin** [F-62]。
- 若更新检查属于"把数据传到用户未指定的系统"，某些签名渠道要求安装程序展示隐私说明并提供关闭选项（§4.2）；安装程序为此预留一页。
- 安装器行为：检测正在运行的 MoriMeta 并要求关闭；不修改 PATH；不注册文件关联（v1 可选"用 MoriMeta 打开文件夹"右键菜单，需单独评估）。
- 需验证：510 个资源文件对安装/升级时长的影响、Windows Defender 对打包 `perl.exe` 的反应 [V-10]。

---

## 3. 卸载

- 删除程序文件与开始菜单项。
- 用户数据（History、Presets、设置、**备份库**、日志）默认保留；卸载器提供复选框"同时删除 MoriMeta 数据"，并显示备份库路径与大小，提示删除后无法撤销历史操作。
- 若备份库位于自定义位置，卸载器只提示位置，不删除（避免误删用户选择的目录中的其他内容）。

---

## 4. 代码签名（候选路线，待 D-2）

### 4.1 选项（依据 Microsoft 2026-08 官方指南 [F-51] 与 SignPath Foundation 条款，2026-09-26 查阅）

| 路线 | 成本 | 首个签名版本的前提 | 显示的发布者 | SmartScreen | 主要不确定性 |
|---|---|---|---|---|---|
| **SignPath Foundation** | 免费 | 条款："The project must already be released in the form that should be signed"；对可执行程序要求 "a certain verifiable reputation"（无具体门槛，不保证批准） | **SignPath Foundation**（条款："SignPath Foundation is the publisher of the OSS project"） | 需积累信誉 | 是否及何时批准；CC0 launcher（见下）；单人维护能否兼任三种角色 |
| **OV 证书（CA 云 HSM）** | $150–300/年 [F-51] | 身份验证通过即可签名 | 个人法定姓名或注册主体 | 需积累信誉 | CA 对个人的身份验证流程 |
| **Microsoft Store（MSIX）** | 免费（Store 重签名） | 提交审核通过 | 开发者账户 | 无警告 [F-51] | Tauri 需社区工具生成 MSIX [F-65]；对备份库位置的影响（V-17） |
| Azure Artifact Signing | ≈ $9.99/月 | 组织：美/加/欧盟/英；**个人仅美/加** [F-51] | 个人或组织 | 需积累信誉 | 地域限制 |
| 不签名 + SHA-256 | 0 | — | 无 | 强烈警告，部分企业直接拦截 | 影响用户信任 |

EV 证书自 2024 年起不再立即绕过 SmartScreen [F-51]，不值得为此购买。

**SignPath 条款中与 MoriMeta 相关的其他要求（原文要点）：**

- 所有组件使用 OSI 认可的许可证，且无商业双许可；允许在签名的安装包中附带上游开源项目未签名的二进制（如 DLL）。
- 条款定义 Authors、Reviewers、Approvers 三种角色，**未说明一人能否兼任**（D-16，需向 SignPath 书面确认）。
- 所有团队成员对 SignPath 与代码仓库开启多因素认证。
- 二进制必须以可验证的方式从源码构建（CI）。
- 若软件把数据传到用户未指定的系统：隐私政策中说明，安装时展示，并提供关闭选项。
- ExifTool Windows 包中的 launcher 以 **CC0** 发布；OSI FAQ 说明 CC0 未获批准（Creative Commons 撤回了申请），且 OSI 不建议用 CC0 发布软件。该 launcher 是否构成问题，条款没有直接回答。处理方式（均为候选）：向 SignPath 书面确认；采用调用方式 B（直接调用 Perl，S0 已验证等价）而不打包 launcher；或自建 Perl 运行时（维护成本高）。

**关于"首次发布"：** SignPath 要求项目"已经以需要签名的形式发布过"，条款没有要求那次发布必须未签名。可行的顺序因此不止一种：先发布未签名的公开预览版，或先用 OV 证书/Microsoft Store 签名发布，再申请 SignPath；后者能否满足其"已发布"与"信誉"条件，需向 SignPath 确认。

### 4.2 候选组合（由 D-2 决定，本文不替用户选择）

| 组合 | 首发签名 | 之后 | 需要用户接受的事项 |
|---|---|---|---|
| ① 未签名公开预览 → SignPath | 否 | SignPath 批准后改为签名发布 | 首发有 SmartScreen 警告；发布者显示为 SignPath Foundation |
| ② OV → （可选）SignPath | 是 | 可继续用 OV，或改用 SignPath | 证书费用；更换签名身份会重新积累 SmartScreen 信誉 |
| ③ Microsoft Store → 其他渠道 | 是（仅 Store 渠道） | GitHub Releases 另需签名路线 | MSIX 可行性（V-17） |
| ④ 以上组合 | — | — | — |

无论选哪条：Public Beta 起所有公开安装包都应签名，并使用**同一签名身份**持续签名以积累信誉 [F-51]；后备方案与切换条件（例如申请后若干周未获批准）随 D-2 一并决定。

### 4.3 签名范围

- 签名：安装器、`MoriMeta.exe`、MoriMeta 自有的其他 PE 文件、updater 包。
- 第三方二进制（`perl.exe`、DLL、方式 A 的 launcher）：默认**不以 MoriMeta 身份重签名**；以完整性清单保护（SECURITY_MODEL §5）。SignPath 条款允许附带上游开源项目的未签名二进制。

---

## 5. 自动更新

| 项目 | 设计 |
|---|---|
| 机制 | `tauri-plugin-updater`（2.10.x [F-60]），签名强制 [F-61] |
| 端点 | GitHub Releases 上的静态 `latest.json`（Stable / Beta 各一）；HTTPS [F-61] |
| 渠道 | Stable（默认）、Beta（设置中选择）；不提供公开 Nightly（PRODUCT_SPEC C-14） |
| 频率 | 首次启动询问是否自动检查（D-4）；自动检查最多每周一次，手动随时 |
| 行为 | 发现更新 → 显示版本与 Release Notes 摘要 → 用户确认下载 → 经 OperationGate 取得独占许可（无运行中/待恢复 Operation）→ 关闭 ExifTool 进程池、Journal 落盘 → `passive` 模式安装（Windows 下应用会被自动退出 [F-61]）。前端没有 updater 插件权限，全部经后端命令（ARCHITECTURE §5.1a）；流程待 S6 验证 |
| 安全更新 | Release Notes 中标注；UI 中以非打扰方式提示"包含安全更新" |
| 回退 | 不支持自动降级；每个版本的安装包长期保留在 GitHub Releases；数据库/配置迁移只向前（ARCHITECTURE §11），降级安装会进入只读保护模式 |
| 密钥 | Tauri updater 私钥离线保管 + CI 受保护环境；泄露预案见 SECURITY_MODEL §10 |

---

## 6. 版本与兼容性

- **SemVer**：`MAJOR.MINOR.PATCH`；预发布 `1.0.0-beta.3`。
- `PATCH`：缺陷修复、ExifTool 安全更新（注册表不变）。
- `MINOR`：新功能、新字段（注册表版本递增，向后兼容）。
- `MAJOR`：Preset schema 或备份 manifest 的不兼容变更（尽量避免）。
- 兼容承诺：
  - 新版本总能读取旧版本的 Preset、设置、数据库、备份 manifest。
  - 旧版本遇到新版本数据 → 只读并提示升级，不损坏数据。
  - 任何版本都能对自己之前版本创建的 Operation 执行 Undo（只要备份仍在）。
- About 页面显示：MoriMeta 版本、ExifTool 版本、注册表版本、WebView2 版本（用于 Bug 报告）。

---

## 7. 许可证与第三方声明

### 7.1 MoriMeta 自身（已确定开源；具体许可证待 D-1）

仓库根目录 `LICENSE` 在 D-1 决定后创建；在此之前不发布任何构建，依赖只选用同时兼容下列两种许可证的宽松许可（MIT、Apache-2.0、BSD、Zlib、Unicode 等）。

| 维度 | Apache-2.0 | GPL-3.0-or-later |
|---|---|---|
| 衍生版本 | 任何人都可以做闭源修改版并分发，须保留许可证与声明、标注修改过的文件 | 分发修改版时须以 GPL 发布并提供对应源码；不能闭源分发 |
| 他人复用 MoriMeta 的代码 | 可被任何项目使用（含闭源、GPL-3.0 项目） | 只能被 GPL-3.0 兼容的项目使用 |
| MoriMeta 使用他人的代码 | 可用 MIT/Apache/BSD；不能引入 GPL 代码，除非整体改为 GPL | 可用 MIT/Apache/BSD/GPL-3.0 |
| 专利 | 明确的贡献者专利授权与专利诉讼终止条款（§3） | 明确的专利授权（§11），另有针对"用户产品"的安装信息要求（§6） |
| 贡献流程 | 通常 DCO，无需 CLA | 同左；多人贡献后更换许可证需全体同意（除非有 CLA）；"or-later" 允许升级到 GPL 后续版本 |
| 与 ExifTool 的关系 | 以独立进程调用，属于聚合，与选哪种无关 | 同左 |
| SignPath 条件 | 满足（OSI 认可） | 满足（OSI 认可） |

也可以组合：应用本体用一种、可复用的核心库（如 `mm-exiftool`、`mm-domain`）用另一种。选择由用户决定。

### 7.2 ExifTool Windows 包 [F-02, F-30]

| 组件 | 许可 | 义务（初步，需 V-11 法律确认） |
|---|---|---|
| ExifTool（Phil Harvey） | 与 Perl 相同（Artistic License 或 GPL，二选一） | 附带许可证文本；保留版权声明；以独立程序形式调用（进程间通信，不链接）通常视为"聚合"，不影响 MoriMeta 自身许可证 |
| Strawberry Perl 及其组件 | Perl 许可 + 各组件许可（`Licenses_Strawberry_Perl.zip` 中 graphite2、harfbuzz、bzip2、BerkeleyDB、expat、freetype、gd、gmp 等） | 原样附带 `Licenses_Strawberry_Perl.zip`；GPL 类组件需提供对应源代码的获取方式 |
| `exiftool_files/LICENSE` | GPL-3.0 全文 | 原样保留；确认其适用范围 |
| Oliver Betz launcher | CC0 | 无强制义务，保留说明 |
| GCC 运行时 DLL（libgcc/libstdc++/libwinpthread） | GPL + GCC Runtime Library Exception 等 | 附带许可证 |

做法：

- 包内文件**原样**再分发，不修改（修改会带来额外义务并破坏完整性校验）。
- 在每个 GitHub Release 中附带对应版本的 `Image-ExifTool-<ver>.tar.gz` 源码与 Strawberry Perl 源码获取说明（或镜像），满足源码提供义务（最终方式以法律意见为准）。
- `THIRD_PARTY_NOTICES.md` 由脚本生成：Rust（`cargo-about`）、npm（许可证报告工具）、ExifTool 包、字体、图标。CI 拒绝未知或不兼容许可证（`cargo-deny` 许可证白名单）。
- 应用内 About → Licenses 显示完整第三方声明。
- 字体与图标：仅使用 OFL / MIT / Apache 等可再分发许可的资源，并记录来源。

---

## 8. 分发渠道

| 渠道 | 时间 | 说明 |
|---|---|---|
| GitHub Releases | Beta 起 | 主渠道；安装包 + updater 清单 + SHA-256 + SBOM + 源码链接 |
| 项目网站 | 1.0 | 下载页清楚说明功能与隐私（若采用 SignPath，其条款另有要求，见 §4.1） |
| winget | 1.0 后 | 提交清单 |
| Microsoft Store | 1.x 评估 | 见 §4.1 |

---

## 9. 公开仓库文档

| 文件 | 内容 | 时间 |
|---|---|---|
| `README.md` | 定位、截图、功能、安全模型摘要、下载、系统要求 | Beta 前 |
| `docs/INSTALLATION.md` | 安装、SmartScreen 说明、卸载与数据位置 | Beta 前 |
| `docs/USER_GUIDE.md` | 工作流、RAW/sidecar 与 Lightroom 等软件的关系、Undo 与备份、FAQ | Beta 前 |
| `PRIVACY.md` | 本地处理、无遥测、更新检查的网络请求内容、日志位置与内容、如何删除数据 | Beta 前 |
| `SECURITY.md` | 漏洞报告渠道、支持版本、ExifTool 安全更新策略 | Beta 前 |
| `CONTRIBUTING.md` | 开发环境、测试要求（尤其安全测试）、PR 规则、DCO/CLA（取决于 D-1） | 首次公开前 |
| 代码签名政策页 | 若采用 SignPath：团队角色（Authors/Reviewers/Approvers）、签名流程、隐私说明 | 申请签名前 |
| `CHANGELOG.md` | Keep a Changelog 格式 | 首个版本起 |
| `THIRD_PARTY_NOTICES.md` | 生成 | 每次发布 |
| `.github/ISSUE_TEMPLATE/` | Bug（版本、OS、格式、ExifTool 版本、复现步骤、期望/实际、脱敏日志 + 隐私提醒）、Feature、Compatibility report | Beta 前 |

---

## 10. ExifTool 更新流程

1. 监控 exiftool.org history/RSS（自动任务，发现新版本开 Issue）。
2. 判断：标注 "Security update"（含 "Windows only"）→ 启动 SLA（14 天），即使它不是 production release；其他版本 → 纳入下一个计划版本（锁定规则：包含全部已知安全修复的最新版本）。
3. 更新 `tools/fetch-exiftool` 中的版本与 SHA-256 → CI 运行完整语料回归（写入 + V1–V6 验证 + 派生标签白名单差异报告）。
4. 抽查兼容性实验室关键项。
5. 发布 PATCH 版本，Release Notes 注明 ExifTool 版本与原因。

---

## 11. 发布检查清单（每个版本）

- [ ] 全部 CI 必过检查通过（含故障注入矩阵与语料回归）。
- [ ] 无未关闭的 P0/P1 数据安全缺陷。
- [ ] ExifTool 版本与 SHA-256 已记录；完整性清单已生成。
- [ ] 数据库/配置/Preset/manifest 迁移在"从上一 Stable 升级"与"从上一 Beta 升级"两条路径上测试。
- [ ] 安装、升级、卸载（保留/删除数据）在 Win10 与 Win11 上验证。
- [ ] 签名验证（安装包、主程序、updater 包）。
- [ ] THIRD_PARTY_NOTICES、SBOM、SHA-256 已生成并附在 Release。
- [ ] Release Notes（含用户可见的安全/兼容性说明）中英文。
- [ ] `latest.json` 仅在所有产物上传并验证后更新。

---

## 12. macOS / Linux（v2）

- macOS：Developer ID 签名 + 公证；ExifTool 需打包 Perl 发行版（系统 Perl 已被 Apple 标为弃用）；注意 macOS 特有伪标签与 CVE-2026-3102 类问题 [F-06]。
- Linux：AppImage / Flatpak / deb 评估；WebKitGTK 与 WebView2 的差异需单独做 UI 验证。
- 跨平台前提：`mm-fs` 的 POSIX 实现与对应的故障注入测试。
