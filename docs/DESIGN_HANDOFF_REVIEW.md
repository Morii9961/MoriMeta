# MoriMeta — 设计交接实施冲突核对

> 工程侧只读核对，2026-09-27。四份新设计文档仍由设计会话维护，本文件不修改它们。设计标注 `FROZEN` 是设计稿状态，不能替代 Morii 的人工屏幕验收，也不表示产品规格 v0.3 已批准。

## 实施前需对齐

| # | 设计稿位置 | 对照依据 | 实施阻碍与需对齐的语义 |
|---|---|---|---|
| H-1 单文件提交 | `INTERACTION_SPEC.md:85-86` 写“atomic swap → journal entry”，且称 swap 前失败时原路径不变 | `SAFETY_MODEL.md:100-115,145-154`；`PRODUCT_SPEC.md:60-61` | 顺序必须是备份及 `ReadyToCommit` Journal 持久化 **先于** `ReplaceFileW`，提交后再登记 `Committed`。S2 已观察到进程在 `ReplaceFileW` 内终止时原路径短暂缺失、原内容在预先登记的 bak 路径；恢复完成后才可作“两种完整状态”的承诺。界面错误态与恢复态不能假设原路径始终存在，也不能把提交过程称为严格原子。 |
| H-2 Retry | `INTERACTION_SPEC.md:95`：“opens Preview (skippable if nothing changed since)” | `DESIGN.md:23`；`PRODUCT_SPEC.md:60,102-104,290,296-298`；`ARCHITECTURE.md:154-158,174-180`；`SECURITY_MODEL.md:125-129` | Retry failed 产生新的 Plan；无论文件看起来是否变化，仍须展示该 Plan 的 Preview，并以该版本和确认令牌执行。可简化无变化项的阅读，但不能跳过 Preview/Plan 门禁。 |
| H-3 时间模式范围 | `SCREEN_SPEC.md:43-55` 写“8 radios”，列出 Change time zone、Sync from reference、Range、Random；`INTERACTION_SPEC.md:123-127` 把 Random/Range 作为现行模式；`DESIGN.md:56` 又写“6 capture-time modes” | `PRODUCT_SPEC.md:176-208`；`DEVELOPMENT_PLAN.md:86-87,202,211` | MVP 已定的仅 Absolute、Shift、Sequence、Preserve Relative Timing 四项。Change time zone 是否进入 MVP 取决于 D-18；独立的双机参照同步是 v1 候选；Range/Random 是 v1。需明确后四项在 1.0 是隐藏、禁用还是仅作说明，不能按八个可执行模式实现。 |
| H-4 版本与权威范围 | 四份设计文档首页 `FROZEN · v1.0`；`DESIGN.md:65-72` 只列设计文件和 mocks 的优先级；`SCREEN_SPEC.md:3,151` 声称三种分辨率已自动核验 | `PRODUCT_SPEC.md:3,10-21`；`DEVELOPMENT_PLAN.md:3,78-80,206-213`；`SAFETY_MODEL.md:3-4` | 应将“设计交付 v1.0”与“产品目标 1.0 / 产品规格及计划草案 v0.3”分开标识。设计内部优先级不覆盖产品范围、安全不变量或后端授权边界。三种分辨率的自动核验记录不等于 Morii 的人工屏幕验收；本仓库未含 `.dc.html` mocks，工程侧尚无法复核其画面。 |
| H-5 Creator 术语 | `SCREEN_SPEC.md:29` 把 Artist、Creator、Credit、Website、Email 并列为 Inspector 字段；`INTERACTION_SPEC.md:20` 以 Creator 作为一个字段的写入示例 | `PRODUCT_SPEC.md:97-103,168,216-221`；`METADATA_MODEL.md:203` | `creator` 是一个规范字段，EXIF Artist、XMP creator 和已有 IPTC By-line 是其底层标签/来源。若 Artist 与 Creator 是两个可编辑控件，会出现彼此覆盖的 Plan。需要标明哪些是规范字段，哪些只在来源详情中显示；Credit、Website、Email 不能因并列展示而自动进入 MVP 可写范围。 |
| H-6 待决功能的入口 | `DESIGN_SYSTEM.md:223` 工具栏显示 Clean export；`SCREEN_SPEC.md:30` 与 `INTERACTION_SPEC.md:59,106` 将其当作可选操作 | `PRODUCT_SPEC.md:223-250`；`DEVELOPMENT_PLAN.md:199,211` | Clean Export 是否纳入 MVP 仍由 D-15 决定。实现可保留视觉占位，但不能将入口做成 1.0 已承诺的可执行功能。 |
| H-7 只读属性改动 | `INTERACTION_SPEC.md:62,95`、`SCREEN_SPEC.md:144` 提供“Clear read-only attribute…”并让该文件随后 Retry | `ARCHITECTURE.md:152-158`；`SECURITY_MODEL.md:125-129`；`PRODUCT_SPEC.md:60` | 清除文件只读属性本身会改动用户文件的状态，需要定义 Plan、Preview、Journal、Undo 与后端授权方式；现有 FieldRegistry/Operation 范围没有这个动作。定义前只提供解释与系统外处理指引。 |

## 措辞需收窄

- `INTERACTION_SPEC.md:5` 的“Nothing touches disk before Apply”若指用户照片内容，是正确的产品意图；若按字面理解，则与 `ARCHITECTURE.md:222` 的 Plan 摘要落库及扫描/缓存行为冲突。实现时应明确为“不在 Apply 前修改源照片或 sidecar”。
- `DESIGN_SYSTEM.md:73` 将所有失败统一配“original unchanged”，但 `SAFETY_MODEL.md:145-154` 存在需要恢复的中间态，且提交后 Journal 写入失败可留下已提交文件。错误文案应基于 Journal 与磁盘状态核实后给出，不可无条件保证原路径内容未变。

## 当前工程边界

- 暂不修改或提交四份设计文档，暂不开始产品界面。
- H-1、H-2、安全错误文案以安全模型和后端门禁为实施下限；H-3、H-5、H-6、H-7 在 UI 前由产品/设计对齐。
- 当前核心规模验证不依赖上述设计决定。真正需要 Morii 后续提供的是 D-18、D-15 的产品选择，以及真实相机语料/第三方软件测试资源（D-13）；D-1 和仓库归属只在公开仓库阶段需要。
