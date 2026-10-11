# MoriMeta 项目状态

> 快照时间 2026-10-11T22:35:00+08:00 · 依据 `main` @ `e6c6906` · **本次没有重新运行任何测试**（`tests_rerun: false`），测试结论都来自已有记录 · 机器可读版本：[`project-status.json`](project-status.json)（Project Status Protocol v1，供手动导入 Moriium Dashboard）· Morii 的验收记录：[`ACCEPTANCE.md`](ACCEPTANCE.md)

本文与 `project-status.json` 内容一致；两者只在 Morii 说出「生成项目状态快照」时一起刷新（见文末）。

## 概况

- **阶段**：`acceptance`（验收）。开发已到验收入口，但验收还没有开始。
- **摘要**：MVP 功能已在代码中完成：桌面应用能导入照片、预览并应用修改、撤销、中断后恢复和干净导出，历史自动化测试与 CI 均通过。至今没有任何功能经 Morii 亲自验收；安装包未签名，更新密钥和真实环境验证尚缺。下一步由 Morii 用照片副本按手动试用清单试用。
- **总体进度**：`null`。仓库里没有可以折算成百分比的依据（计划没有按工作量拆分的任务表），所以不写数字，用下面的里程碑代替：14 项中完成 9、进行中 2、受阻 2、未开始 1；**Morii 验收通过 0 项**。

## 三种证据，分开算

| 证据等级 | 意思 |
|---|---|
| `repository` | 代码或文档在仓库里（"代码已写好"），没有测试记录证明它整体可用 |
| `existing_test` | 以前的自动化测试、CI 或脚本化冒烟测试有通过记录（"历史测试通过"），本次没有重跑 |
| `morii_accepted` | Morii 亲自用过并在 `ACCEPTANCE.md` 登记"验收通过"（目前 0 项） |
| `self_report` / `unknown` | 只有 Agent 的说法 / 无从判断（本次没有用到） |

Morii 验收一栏：`pending` 待验收（已经可以试用）· `accepted` 验收通过 · `rejected` 发现问题 · `not_required` 工程或发布事项，不需要作为用户验收 · `unknown` 还不到能验收的程度。`accepted` 和 `rejected` 只来自 `ACCEPTANCE.md` 中 Morii 的记录。

## 里程碑

| 编号 | 里程碑 | 状态 | 证据等级 | 证据 | Morii 验收 |
|---|---|---|---|---|---|
| M01 | 资料库：添加文件夹、排序筛选、检查器（只读查看） | done | existing_test | research/MANUAL_TEST_PLAN.md §2；research/PROGRESS.md 2026-10-04 资料库与检查器补全；research/PROGRESS.md 2026-10-10 真实应用 UI 冒烟测试 PASS；GitHub Actions ci 38104520043（e6c6906，success） | pending |
| M02 | 批量编辑作者与版权 → 预览 → 应用 | done | existing_test | research/MANUAL_TEST_PLAN.md §3；research/PROGRESS.md 2026-10-01 桌面应用第一版（UI 冒烟测试：应用后逐个核对磁盘上的文件）；GitHub Actions ci 38104520043（e6c6906，success） | pending |
| M03 | 拍摄时间工具（平移、序列、绝对、保持间隔）与 GPS 设置/移除 | done | existing_test | research/MANUAL_TEST_PLAN.md §4；research/PROGRESS.md 2026-10-10 时间工具与 GPS 输入的属性测试；crates/mm-domain/tests/properties.rs；GitHub Actions nightly 38127870408（e6c6906，success） | pending |
| M04 | 历史与撤销、恢复备份到文件夹、导出日志 | done | existing_test | research/MANUAL_TEST_PLAN.md §5；research/PROGRESS.md 2026-10-03 历史撤销选择和重新规划；research/PROGRESS.md 2026-10-04 真实应用 UI 冒烟测试（撤销后 12 个文件逐字节还原） | pending |
| M05 | 中断后恢复（继续剩余 / 撤销已完成 / 保持现状） | done | existing_test | research/MANUAL_TEST_PLAN.md §6；docs/PHASE1B_FAULT_MATRIX.md；README.md Safety, stated with its conditions；GitHub Actions ci 38104520043（e6c6906，success） | pending |
| M06 | 预设与规则（规则构建器、试运行、导入导出） | done | existing_test | research/MANUAL_TEST_PLAN.md §7.1–7.3；research/PROGRESS.md 2026-10-01 Presets and Rules screens；research/PROGRESS.md 2026-10-10 界面规格补齐 | pending |
| M07 | 干净导出（JPEG 副本去除位置、序列号等） | done | existing_test | research/MANUAL_TEST_PLAN.md §7.4；research/PROGRESS.md 2026-10-01 Clean Export；docs/DECISIONS.md D-15 | pending |
| M08 | 设置、备份管理、中英文界面 | done | existing_test | research/MANUAL_TEST_PLAN.md §8；research/PROGRESS.md 2026-10-03 备份管理界面；research/PROGRESS.md 2026-10-11 备份位置拒绝网络共享（T-22）；commit c5cfef2 | pending |
| M09 | 安装包、首次启动、卸载（未签名 NSIS） | done | existing_test | research/MANUAL_TEST_PLAN.md §1、§9；research/PROGRESS.md 2026-10-10 发布流水线；GitHub Actions release 38040422715（e35f3a0，success）；docs/DECISIONS.md U-1 | pending |
| M10 | 应用内更新（S6：生产密钥与真机安装验证） | in_progress | repository | docs/UPDATER_RELEASE.md；research/PROGRESS.md 2026-10-03 Offline updater release preparation；CHANGELOG.md Known limits | unknown |
| M11 | 发布流水线（首个版本尚未发布） | in_progress | existing_test | .github/workflows/release.yml；docs/RELEASE_PLAN.md §11；GitHub Actions release 38040422715（e35f3a0，success） | not_required |
| M12 | 代码签名（SignPath Foundation 申请） | blocked | repository | docs/CODE_SIGNING.md；docs/DECISIONS.md D-2、D-16 | not_required |
| M13 | 真实环境验证（D-13：真实语料、第三方软件读取、断电、SD 卡与 NAS、云同步） | blocked | repository | docs/DECISIONS.md D-13；research/PROGRESS.md Waiting / needs the user；CHANGELOG.md Known limits | not_required |
| M14 | Moriium Gallery 集成（非 v1 必需） | not_started | repository | MoriMeta_Project_Spec_v0.1.md §27；docs/PRODUCT_SPEC.md C-13；docs/DEVELOPMENT_PLAN.md §7 v1.1 | not_required |

各项"已有测试"覆盖到哪里为止：

- M01–M02、M04 的主流程由 `apps/desktop/scripts/ui-smoke.mjs` 在真实应用里走过（导入 → 批量作者 → 预览 → 应用 → 核对磁盘 → 撤销 → 历史），但"恢复到文件夹""导出日志"弹出的是系统对话框，冒烟测试没有点过。
- M03 的时间与 GPS 界面只在模拟后端中检查过；真实写入由后端端到端测试与属性测试覆盖。
- M05 的崩溃恢复由 CI 故障注入覆盖（核心层）；**在桌面应用里强制结束再启动，还没有任何记录**。断电没有测过。
- M09 的安装包在 CI 上构建并通过 `release-check.mjs`，但**从没有在真实机器上安装过**。
- M10 只有合成签名的测试；没有生产密钥，没有真实的下载与安装。M11 手动运行一次通过，打标签才执行的构建证明与草稿发布两步没有实际跑过。
- 可访问性（键盘之外）：代码中有 ARIA 标注，没有专门的验证记录。

## 现在能用什么

以下判断只来自已有记录，本次没有新测试。

- 可以亲手试的：`research/MANUAL_TEST_PLAN.md` 的全部 9 节（安装、只读查看、批量编辑、时间与 GPS、历史与撤销、中断恢复、预设与干净导出、设置、卸载），约 1–2 小时。**只用照片副本**，不要放在云同步文件夹或 U 盘里（那里会被有意拒绝写入）。
- 安装包的两种来源：本机构建（清单第 0 节，约 10 分钟）；或 release 工作流产物——现有一份构建于 2026-10-10（e35f3a0），早于 T-22 备份位置修复，2026-10-24 过期，想用最新代码就在 GitHub Actions 里重新运行一次。
- 还不能用的：应用内更新（没有生产密钥）；签名安装包（SmartScreen 会拦，需"仍要运行"）。

## Morii 最值得先体验的 3 个场景

1. **给一批照片写作者和版权，再整批撤销**（清单 §3、§5）：日常最常用的一条路，同时能看到"先预览、再写入、可撤销"是不是清楚可信。
2. **修正相机时钟：对 NEF+JPG 整体平移时间**（清单 §4.1–4.2，场景 A）：确认 NEF 本身一个字节都不动、改动写进 `.xmp`，同名 JPG 与 NEF 用同一个时间。
3. **应用到一半强制结束，再启动**（清单 §6，场景 E）：最重要的安全承诺，也是自动化测试在真实应用里唯一没走过的环节。

## 尚未验收的功能、问题与反馈

- 可以试用的 M01–M09 全部是 `pending`；没有任何一项经 Morii 验收。
- Morii 的反馈：`ACCEPTANCE.md` 没有记录；GitHub 上没有未关闭的 issue。
- 仓库中没有记录未关闭的数据安全缺陷（这不等于没有缺陷，只说明没有发现记录）。工程侧已知、未处理的事项见 `research/PROGRESS.md` "Next engineering steps"：重负载下偶发写入超时（原文件不变，超时值待 S4 校准）、S4 性能未在真实数据上测、扫描只读必要标签的取舍待定。

## v1 发布前真正必要的事

计划中的路线是 Private Alpha（5–10 名摄影师，用副本）→ Public Preview/Beta → 1.0（`docs/DEVELOPMENT_PLAN.md` Phase 4）。按 `docs/RELEASE_PLAN.md` §11 与 `docs/DECISIONS.md`，1.0 之前必须有：

1. Morii 验收，并修掉验收中发现的问题（M01–M09）。
2. 首个公开预览 v0.1.0（未签名，D-2），使打标签的发布路径实际走通（M11）。
3. 签名：公开 Beta 起所有安装包签名（D-2，M12）。
4. 更新：生产密钥；安装、升级、卸载在 Windows 10 与 11 上实测（S6，M10）。
5. 真实环境验证（D-13，M13）；做不到的项目在发布说明中写为"未验证"并保持默认限制，不拿合成结果冒充。

不属于 1.0 必需：Moriium Gallery 集成、Metadata JSON/CSV 导出（v1.1）、DNG 写入（v1.2）、时区修正（v1.3）。

## 与 Moriium Gallery 的集成

- **进展：未开始。是否 v1 必需：否。**
- 依据：v0.1 项目规格 §27 只把它写成"未来可形成"的流水线（Z8 RAW → MoriMeta → 元数据清理 → 衍生图 → Moriium Gallery），并明确"不应成为 MoriMeta 的硬依赖，公共版本必须保持通用性"；`docs/PRODUCT_SPEC.md` C-13 把 Moriium 等个人预设降为文档示例；规格 §27 里给其他系统读取的结构化 JSON 导出排在 v1.1。
- 已经有的相关部分：干净导出（M07）就是这条流水线里"元数据清理"那一步，可以单独试用。

## 阻塞

| 阻塞 | 影响 |
|---|---|
| 尚无 Morii 亲自试用的结论 | 全部功能只有代码与自动化测试证据，好不好用、有没有看漏的问题都还不知道，不宜发布公开预览。 |
| 代码签名需要 Morii 申请，且须先有公开版本 | D-2 规定公开 Beta 起安装包必须签名；未签名的安装包会被 SmartScreen 拦截。 |
| 更新功能缺生产签名密钥，真机安装未验证（S6） | 不设密钥构建的版本永不检查更新；安装、升级、卸载未在 Windows 10/11 上实测。 |
| 1.0 前的真实环境资源缺失（D-13） | 真实相机语料、Lightroom/NX Studio 读取、断电、SD 卡与 NAS、云同步均未验证，只能在发布说明中写为未验证并保持默认限制。 |

## 下一步（最多 3 项）

| 优先级 | 动作 | 结果 |
|---|---|---|
| P0 | Morii 按 research/MANUAL_TEST_PLAN.md 用照片副本完成第一次手动试用，并在 docs/status/ACCEPTANCE.md 登记结论 | M01–M09 各有 Morii 的结论：验收通过或发现问题。 |
| P1 | 工程修复试用中发现的问题；Morii 复验后决定是否推送 v0.1.0 标签，发布未签名公开预览 | 首个公开版本；release.yml 的构建证明与草稿发布两步首次实际运行。 |
| P2 | v0.1.0 发布后，Morii 申请 SignPath Foundation 代码签名（8 周未获批则改用 OV 证书） | 公开 Beta 起的安装包可以签名。 |

## 需要 Morii 决定

1. 何时开始第一次手动试用；安装包用本机构建，还是重新运行 release 工作流（现有产物构建于 2026-10-10，早于 T-22 修复，2026-10-24 过期）。
2. 试用并修复后，是否推送 v0.1.0 标签发布未签名公开预览。
3. 首个版本前是否设置更新公钥（仓库变量 MORIMETA_UPDATER_PUBLIC_KEY）；不设置，该版本永不检查更新。
4. SignPath Foundation 申请与 D-16 角色询问；F-104 报告是否发到 ExifTool 论坛（草稿在 research/reports/）。
5. D-13 资源能提供哪些（真实语料、Lightroom/Capture One/NX Studio、SD 卡、NAS、专用云同步目录、断电虚拟机）；其余在发布说明中写为未验证。
6. Dependabot PR：工程建议合并 #2、#5，关闭 #3、#4；#6 已被提交 730a728 取代，可直接关闭。

## 刷新与验收登记

**刷新口令：Morii 在对话中输入「生成项目状态快照」。** 没有这句话，Agent 不刷新这两个文件；没有定时任务，不联网同步，不自动跑测试。刷新时 Agent：

1. 只读取证据：git 提交、`research/PROGRESS.md` 的新条目、`ACCEPTANCE.md`、CHANGELOG、计划文档，以及 GitHub 上已有的 CI 结论（只读查询）。不运行测试、构建或照片处理；`tests_rerun` 保持 `false`。
2. 同时重写本文与 `project-status.json`，两者逐项一致；里程碑编号不变，新增的接着编。`source.commit_sha` 写证据所依据的提交。
3. `human_acceptance` 只按 `ACCEPTANCE.md` 中该里程碑的最后一行：待验收 → `pending`，验收通过 → `accepted`，发现问题 → `rejected`；没有记录时按上文含义填 `pending`、`unknown` 或 `not_required`。只有 Morii 登记的"验收通过"才能把证据等级写成 `morii_accepted`。
4. 不写真实照片路径、照片中的元数据值、个人信息或任何凭据。
5. 只提交这两个文件；刷新本身不改 `ACCEPTANCE.md`。

**验收登记**：Morii 在 [`ACCEPTANCE.md`](ACCEPTANCE.md) 末尾加一行（格式见该文件）。也可以直接在对话里说，例如"M02 验收通过""M05 发现问题：……"，由 Agent 原话转录并注明。
