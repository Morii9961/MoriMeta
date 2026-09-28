# MoriMeta — Security Model & Threat Model

> **Version:** 0.3 · **Status:** 草案（未批准）· **Date:** 2026-09-26
> v0.3 的修改依据 [SPIKE_REPORT](SPIKE_REPORT.md) §1–§2（S0/S1）。
> 数据完整性（不损坏照片）见 [SAFETY_MODEL](SAFETY_MODEL.md)；本文处理**恶意输入与攻击者**。
> `[F-xx]` / `[V-xx]` 引用 [RESEARCH_NOTES](RESEARCH_NOTES.md)。

---

## 1. 范围与资产

| 资产 | 需要保护的属性 |
|---|---|
| 用户照片与 sidecar | 完整性（不被篡改、覆盖、删除）、可用性 |
| 用户隐私（GPS、身份、路径、照片内容） | 机密性（不外泄、不写入日志/报告） |
| 用户系统 | 不因处理不可信文件而执行任意代码 |
| MoriMeta 分发渠道 | 用户安装/更新到的是真实的 MoriMeta |
| 备份库与 Journal | 完整性（恢复不被利用来写任意位置） |

不在范围：已取得用户账户权限的本地恶意软件（它可以直接修改照片）；物理访问攻击。但我们不应让 MoriMeta 成为其提权或持久化的跳板。

---

## 2. 信任边界

```text
 [不可信] 照片文件内容（元数据、MakerNotes、嵌入对象）   ──┐
 [不可信] 文件名与目录结构                                ──┤
 [不可信] 导入的 Preset JSON                              ──┤
 [半可信] ExifTool 输出（解析不可信数据的产物）            ──┤──► Rust core（可信）──► 文件系统写入
 [半可信] WebView 中运行的前端（可能因 XSS 被控制）        ──┘         │
 [可信，需验证] 更新服务器返回的更新包（签名验证）                    └──► exiftool.exe（运行在同一用户权限下）
```

关键判断：

- **ExifTool 处理的是攻击者可控的数据**，它有过 RCE 与命令注入历史 [F-06]。ExifTool 进程与 MoriMeta 同权限运行，因此"让 ExifTool 不被利用"与"及时更新 ExifTool"都是安全要求。
- **WebView 被视为半可信**：元数据字符串由攻击者控制并在 UI 中显示；即使出现 XSS，攻击者也只能调用暴露的命令，而这些命令无法在未经后端生成的 Plan 与用户确认的情况下写文件（§6）。

---

## 3. 攻击者与入口

| 入口 | 例子 |
|---|---|
| 恶意图片 | 从网络下载、客户交付、比赛投稿的图片中构造的元数据：超长字符串、控制字符、HTML/脚本、畸形结构、触发 ExifTool 解析器漏洞的 payload |
| 恶意文件名 | `-o.jpg`、`#x.jpg`、`x.jpg|`、含换行（非 Windows）、Unicode 减号、`..`、保留名（`CON`）、超长 |
| 恶意目录结构 | junction 循环、指向系统目录的符号链接、硬链接到系统文件 |
| 恶意 Preset | 社交渠道分享的 Preset JSON，试图注入标签名、选项或路径 |
| 更新渠道 | DNS/网络劫持、发布账号被盗 |
| 供应链 | 被投毒的 crate/npm 包、被篡改的 ExifTool 下载 |
| 本地环境 | `PERL5OPT`、`PERL5LIB`、`.ExifTool_config`、`EXIFTOOL_HOME` 等环境与配置注入 [F-07, F-31] |

---

## 4. ExifTool 调用加固（强制规则）

### 4.1 进程

| 规则 | 依据 |
|---|---|
| 从不经 shell 启动；参数使用 `Command::arg` 数组，启动参数固定为 `-config "" -charset filename=utf8 -stay_open True -@ -`（调用方式 B 时，`exiftool.pl` 为第一个参数） | F-07, F-08, F-11 |
| 清空环境变量，仅保留 `SystemRoot`、`TEMP`/`TMP`（应用私有目录） | F-31 实测注入 |
| 工作目录为应用私有空目录 | F-07（cwd 中的 `.ExifTool_config` 会被执行） |
| 可执行文件位于安装目录内，以绝对路径启动，不使用 PATH 搜索；方式 A 的 launcher 重命名为 `exiftool.exe` | F-23（文件名括号内容被当作选项） |
| ExifTool 进程放入 Job Object（`KILL_ON_JOB_CLOSE`），MoriMeta 退出或崩溃时随之终止，不留下持有文件句柄的孤儿进程 | S1 R8 |
| 启动后校验 `-ver` 等于构建锁定版本；启动前校验打包文件完整性（§5） | 防篡改、防版本漂移 |
| MoriMeta 与 ExifTool 从不以管理员权限运行；检测到提权运行时显示警告并禁用写操作 | 降低被利用后的影响 |

### 4.2 参数与数据

| 规则 | 依据 |
|---|---|
| 所有不可信数据只经 **stdin UTF-8 argfile** 传递，从不出现在命令行 | F-11, F-33 |
| 文件参数一律为规范化**绝对路径**（不以 `-`、`−`、`#` 开头）；拒绝含 `\r`、`\n`、`\0`、`\|` 的路径 | F-07, F-32；S1 R2 |
| 标签名只能来自 FieldRegistry 或 Advanced 白名单；类型系统保证不接受字符串拼接 | 防选项/标签注入 |
| 值只出现在 `-GROUP:TAG=` 之后；写入命令带 `-ex`，值以 XML 字符引用编码，任何一行都不含换行；NUL、C0 控制字符、U+FFFE/U+FFFF 由编码器拒绝 | S1：100,000 次随机往返 0 差异；注入用例全部作为单个值存储 |
| 渲染层（写入 stdin 之前）再次断言每一行不含 `\r`、`\n`、`\0` | 纵深防御；参照 CVE-2026-43893 的修复方式 |
| 每条命令使用新的随机 64 位 ID 作为 `-execute`/`-echo4` 终止标记，只接受当前 ID；文件内容中的 `{ready…}` 被当作数据 | S1 R3 |
| **永不使用** `<`/`<=` 复制与重定向语法承载任何用户数据（Perl 表达式插值） | F-07 |
| **永不使用** 的选项：`-if`、`-p`、`-fileNUM`、`-api filter`/`filterw`、`-userParam`、`-ee`、`-geotag`/`-geosync`/`-geotime`（v1 GPX 同步需单独设计）、`-overwrite_original*`、`-srcfile` 指向计划外文件、`-config` 非空 | F-06（CVE-2026-7580 经 `-ee`）、F-07 |
| 读取 JSON 输出时使用 `-api StructFormat=JSONQ`，严格解析并设大小上限（单文件输出 > 16 MB 视为异常） | DoS；默认 JSON 会把字符串转成数字/布尔值（S1） |

### 4.3 永久禁止写入的标签（即使在 Advanced 模式）

| 类别 | 标签 | 原因 |
|---|---|---|
| 文件系统伪标签 | `FileName`、`Directory`、`TestName`、`HardLink`、`SymLink`、`FilePermissions`、`FileAttributes`、`FileModifyDate`、`FileCreateDate`、`FileAccessDate`、`FileInodeChangeDate`、`FileUserID`/`FileGroupID` 等 System 组可写标签 | 可移动/重命名文件、创建链接、改变权限 [F-18]；macOS 上 FileCreateDate 曾导致命令注入 [F-06] |
| 轨迹/外部文件 | `Geotag`、`Geosync`、`Geotime` | 读取任意路径文件 [F-18] |
| MakerNotes | `MakerNotes:*` | 永久标签、兼容性风险 [F-17] |
| 结构性 | `ICC_Profile`、`Orientation`（字段层之外）、JUMBF 写入、`ThumbnailImage`/`PreviewImage`/`JpgFromRaw` 写入 | 影响图像呈现或完整性 |
| 整组删除 | `-all=`（仅 Clean Export 的受控模板可用）、`-EXIF:all=` 等 | 批量破坏 |

白名单在构建时由脚本对照 ExifTool `-listw` 生成并与禁止表求差，结果入库并在 ExifTool 升级时复核。

---

## 5. ExifTool 打包与供应链

- 构建脚本从 exiftool.org / SourceForge 下载固定版本，并比对**已固定在仓库中的 SHA-256**（取自官方 `checksums.txt`，见 `research/exiftool.lock.json`）；不一致 → 构建失败。
- 打包时生成 `exiftool.manifest`：每个文件的相对路径 + BLAKE3。应用启动时校验关键文件（`exiftool.exe`、`perl.exe`、`perl*.dll`、`lib/Image/ExifTool.pm`）；完整校验在后台进行。不一致 → 禁用写操作并提示重新安装。
- 版本策略：锁定**包含全部已知安全修复的最新版本**（当前 13.59）；**安全更新 SLA**：ExifTool 发布安全更新后 14 天内发布 MoriMeta 补丁版本（需通过语料回归）。2026 年已有 13.50（macOS）、13.53（Windows）、13.54、13.59 四次安全更新 [F-06]，这是持续性义务。
- 调用方式（ARCHITECTURE ADR-03，D-17）：官方 launcher（CC0）或直接调用包内 `perl.exe`（S0 验证行为等价）。两种方式对环境变量注入同样敏感，环境清空不可省略。
- Perl 运行时：Windows 包内为 Perl 5.32.1 [F-30]，该主版本已不在 Perl 上游支持周期内。风险：ExifTool 解析不可信数据时依赖该运行时（如正则引擎）。选项：(a) 使用官方包（广泛测试，维护成本低）；(b) 自行以当前 Strawberry Perl 构建包（运行时更新，但需自行承担测试与兼容性）。**MVP 建议 (a)**，并跟踪上游是否更新运行时；列为残余风险 R-3。
- 自定义 ExifTool 路径（v1 Advanced）：显示风险、校验版本 ≥ 锁定版本、不做完整性校验（用户自担），默认关闭。

---

## 6. WebView 与 IPC

### 6.1 前端加固

- CSP：`default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' asset: blob: data:; connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; frame-ancestors 'none'`（最终以 Tauri 2 的 IPC 协议要求校准）。
- 不加载任何远程内容；不嵌入第三方脚本、字体 CDN 或统计。
- 元数据一律以文本节点渲染；禁用 `dangerouslySetInnerHTML`（ESLint 规则强制）；不把元数据作为 URL 或 CSS 值使用。
- 预览图以受限的自定义协议或 blob 提供，只能访问后端生成的缓存条目，不能按路径读取任意文件。
- 考虑启用 Tauri Isolation Pattern（额外拦截与校验 IPC 消息）；在 S5 评估性能与复杂度后决定。

### 6.2 Capability 最小化

- 不启用 `shell`、`fs`、`http`、`process` 插件给前端；`dialog` 插件仅在 Rust 侧调用。
- Capability 仅包含 MoriMeta 自定义命令与必要的窗口/事件权限；CI 检查 capability 文件的差异需代码所有者审核。
- **前端不获得任何 updater 插件权限。** 检查、下载、安装只通过后端自定义命令进行，并经 OperationGate：安装需要独占许可，且不存在运行中或待恢复的 Operation（ARCHITECTURE §5.1a）。这样前端（即使被控制）无法绕过写操作门禁直接安装。

### 6.3 命令授权模型（Handle 模型）

- 前端无法提交任意路径用于写入；所有写入必须引用后端生成的 `PlanId + version`（ARCHITECTURE §5.1）。
- 执行需要 `confirm_token`：由后端在 Preview 返回 Plan 摘要时签发、与 Plan 版本绑定、一次性有效、短期过期。它不是抵御完全被控制的前端的密码学措施（被控前端也能请求令牌），但确保"执行"只能发生在一个真实存在、完整生成、未过期的 Plan 上，且后端在执行前重新做全部预检。
- 即使前端被完全控制，攻击者能造成的最大影响受限于：对**用户已导入**的文件执行 FieldRegistry 允许的写入，且全部有备份、可撤销。它无法写入任意路径、无法调用 ExifTool 的危险选项、无法删除文件。

---

## 7. 文件系统威胁

| 威胁 | 缓解 |
|---|---|
| 任意文件覆盖 | 写入只发生在：(1) Plan 中、指纹匹配的已导入文件；(2) 在目标目录中以随机名创建的新临时文件（ExifTool `-o` 不覆盖 [F-14]）；(3) 不覆盖重命名创建的新 sidecar/导出文件（I-6）。 |
| 路径穿越 | MVP 不从元数据或模板生成路径；导出文件名 = 源文件名（仅做冲突编号）；v2 Rename Engine 需独立的文件名净化与"目标必须位于源目录内"规则。 |
| 符号链接 / junction / TOCTOU | 导入不跟随目录链接；写入目标为链接 → Blocked；提交前以句柄重新获取 File ID 并比对（SAFETY_MODEL §4.1 步骤 6）；临时文件以随机名创建于目标目录，攻击者预测困难且 `-o` 拒绝已存在目标。 |
| 硬链接到敏感文件 | 链接数 > 1 → Blocked（SAFETY_MODEL §8.7）。 |
| 恶意 manifest / 备份恢复被利用 | 恢复只使用本机数据库中的记录；manifest 仅在数据库丢失时作为冗余读取，并要求 manifest 位于 MoriMeta 备份根目录、路径与卷序列号匹配、恢复前再次 Preview；不支持"导入他人的备份"。 |
| 保留设备名/非法文件名 | 由 `mm-fs` 在导出与临时文件命名时拒绝 `CON`、`NUL`、尾随点/空格等。 |
| 临时文件泄露隐私 | 临时文件位于原目录（同卷原子替换所需），由 Journal 登记并在完成/恢复时清理。 |

---

## 8. 隐私与日志

- 无遥测、无崩溃上报、无第三方 SDK。
- 网络：唯一请求为更新检查（HTTPS，GitHub Releases 或自有静态端点），请求内容仅含应用版本与平台（Tauri updater 默认行为，需在 S6 确认请求头）；首次启动询问是否自动检查。
- 日志默认不含：元数据值、GPS、完整路径、用户名（路径中 `C:\Users\<name>` 被替换）；文件以 `asset#n.ext` 表示。
- 调试日志需显式开启（24 小时自动关闭），界面提示可能包含隐私数据。
- "导出日志 / 报告问题"：展示将导出的内容、默认脱敏、Issue 模板提醒检查私人路径与元数据（沿用 v0.1 §28.8）。
- 实现状态（2026-09-28）：History 的导出日志（`history::export_view` / `export_log`）默认把路径换成 `asset#n.ext`、不导出字段值、错误文本中的路径与用户名被替换（`mm-core::privacy`）；用户可选择包含路径或值。e2e 断言默认导出中没有值、文件夹路径与用户名（T-14 的导出部分）。运行日志文件尚未实现。
- 预览图缓存位于应用缓存目录，可一键清除；卸载时询问是否删除。

---

## 9. Preset 导入

- JSON Schema 严格校验（`deny_unknown_fields`）；大小上限 1 MB；规则数、条件深度上限。
- FieldId 必须存在于当前注册表；模板变量必须在允许列表内；不支持任何表达式求值。
- 导入后作为"未信任"显示，首次应用时 Preview 中标注"来自导入的 Preset"。

---

## 10. 更新与分发安全

- Authenticode 签名安装包与 MoriMeta 自身的可执行文件（RELEASE_PLAN §4）。
- Tauri updater 签名强制 [F-61]：私钥离线保存 + CI 秘密存储；至少两人知晓恢复流程；私钥泄露预案：发布经旧密钥签名的过渡版本，内置新公钥。
- 更新端点仅 HTTPS [F-61]；不开启 `dangerousInsecureTransportProtocol`。
- 更新期间应用会被安装程序关闭 [F-61] → 仅在无运行中/待恢复 Operation 时允许安装。
- 发布产物附带 SHA-256 与 SBOM（CycloneDX）。

---

## 11. 构建与依赖供应链

- `Cargo.lock` 与 `package-lock.json`（或 pnpm lock）入库；CI 使用 `--locked`。
- `cargo-deny`（许可证、漏洞公告、重复依赖、禁止的 crate）、`cargo-audit`、`npm audit`（或 osv-scanner）作为必过检查。
- GitHub Actions 以 commit SHA 固定第三方 action；最小权限 `permissions:`；发布工作流与签名密钥隔离在受保护环境中（需审批）。
- Dependabot/Renovate 每周更新，安全更新即时。
- 前端依赖保持精简；新增依赖需在 PR 中说明理由与许可证。

---

## 12. 威胁清单

| # | 威胁 | 影响 | 缓解 | 验证 |
|---|---|---|---|---|
| T-01 | 文件名被 ExifTool 解释为选项（`-o.jpg`、`−x`） | 执行任意选项/代码 | 绝对路径；§4.2 | 模糊测试（F-32 已复现风险） |
| T-02 | 文件名以 `#` 开头被当注释跳过 | 文件被静默漏处理，Preview 与执行不一致 | 绝对路径；执行后逐文件结果核对（每个请求文件必须有结果） | F-32 已复现；回归测试 |
| T-03 | 文件名以 `\|` 结尾被当管道（CVE-2022-23935） | 命令执行 | ExifTool ≥ 12.38；绝对路径 | 版本校验 |
| T-04 | 值、标签名或路径包含换行/控制字符，拆出新的 argfile 行 | 选项/标签注入、读写任意路径 | 值以 XML 字符引用编码（`-ex`）；标签名来自注册表；路径拒绝 `\r\n\0\|`；渲染层再断言 | S1：100,000 次往返与注入用例通过。类比：CVE-2026-43893（npm `exiftool-vendored` ≤ 35.18.0 封装层未拒绝标签名、路径等输入中的行分隔符；其写入的**值**已按 `&#10;` 编码，不受影响；不是 ExifTool 本体漏洞；35.19.0 修复） |
| T-05 | 恶意文件触发 ExifTool 解析器漏洞（如 CVE-2021-22204、CVE-2026-7580） | 以用户权限执行代码 | 及时更新（SLA）；不使用 `-ee`；非管理员运行；（v2 研究）低完整性/AppContainer 沙箱运行只读扫描进程 | 版本跟踪 |
| T-06 | `PERL5OPT`/`PERL5LIB`/`.ExifTool_config` 注入 | 代码执行、持久化 | 清空环境、`-config ""`、私有 cwd | F-31 已复现；集成测试断言 |
| T-07 | 打包的 ExifTool/Perl 被替换 | 代码执行 | 完整性清单校验；per-machine 安装可选（v1） | 启动测试 |
| T-08 | 元数据中的 HTML/JS 在 WebView 中执行（XSS） | 调用 IPC 命令 | React 文本渲染、CSP、禁止 innerHTML、Handle 模型 | 恶意元数据语料 E2E 测试 |
| T-09 | 被控前端直接调用写命令 | 修改照片 | 只能执行后端生成的 Plan；FieldRegistry 限制；备份可撤销 | IPC 单元测试 |
| T-10 | 伪标签导致文件移动/链接/权限变化 | 任意文件操作 | §4.3 永久禁止表 | 白名单生成测试 |
| T-11 | 符号链接/硬链接诱导写入其他文件 | 覆盖系统或他人文件 | §7 | 环境测试 |
| T-12 | 恶意 Preset | 注入标签/选项 | §9 | Schema 测试 |
| T-13 | 更新劫持 | 安装恶意版本 | 签名 + HTTPS | 发布演练 |
| T-14 | 日志/报告泄露隐私 | 位置、身份泄露 | §8 | 日志快照测试（断言无路径/值） |
| T-15 | 超大/畸形文件导致挂起或内存耗尽 | DoS | 命令超时、输出大小上限、进程重启、熔断 | 畸形语料 |
| T-16 | 依赖投毒 | 构建产物被植入 | §11 | CI 必过检查 |
| T-17 | 恶意 C2PA/JUMBF 或巨型 XMP 拖慢扫描 | DoS | 扫描时排除不需要的 XMP 命名空间；超时 | 基准测试 |
| T-18 | 文件内容伪造协议终止标记（`{ready…}`、`{mm-end:…}`） | 输出截断、结果错配 | 每条命令随机 64 位 ID，只接受当前 ID；不依赖行首位置 | S1 R3（含 `-b` 二进制输出） |
| T-19 | MoriMeta 崩溃后遗留 ExifTool 进程，持有文件句柄或继续执行 | 文件被锁、行为不可控 | Job Object `KILL_ON_JOB_CLOSE` | S1 R8 |
| T-20 | ReplaceFileW 的 bak 名处已有文件（被预测或巧合） | 用户文件被静默覆盖 | bak 名 ≥ 64 位随机；提交前核对不存在；登记在 Journal | S2 F5 实测会覆盖 |
| T-21 | 读取时字符串被转为数字/布尔值 | Preview 与实际值不一致 | `StructFormat=JSONQ` | S1 |

---

## 13. 漏洞响应（SECURITY.md 要点）

- 私下报告渠道（GitHub Security Advisories / 专用邮箱）；承诺 72 小时内确认。
- 支持版本：最新 minor 版本。
- 修复发布后公开 Advisory；ExifTool 上游安全更新单独在 Release Notes 中标注。

---

## 14. 残余风险

| # | 风险 | 说明 |
|---|---|---|
| R-1 | ExifTool 零日漏洞 | 同权限运行；v2 研究沙箱化只读扫描 |
| R-2 | WebView2 漏洞 | 依赖系统更新（Evergreen） |
| R-3 | 打包的 Perl 5.32 运行时不再受上游支持 | 见 §5；持续跟踪；`usesitecustomize` 未启用，`@INC` 仅含包内 lib（S0） |
| R-4 | per-user 安装目录可被同用户进程修改 | 与所有 per-user 应用相同；完整性校验只能发现非恶意损坏或粗糙篡改 |
| R-5 | 其他软件（LR 等）覆盖 MoriMeta 写入的 sidecar | 非安全问题，属兼容性；文档说明 |
