# 设计稿工程评审（对照架构与安全模型）

> 2026-09-28 · 工程侧评审设计会话冻结的 v1.0 设计稿（`DESIGN.md`、`DESIGN_SYSTEM.md`、`INTERACTION_SPEC.md`、`SCREEN_SPEC.md`；这些文件由设计会话维护，本文不修改它们）。
> 目的：列出与 `SAFETY_MODEL.md` / `ARCHITECTURE.md` 及已实现后端不一致之处、后端需要补的能力，以及取决于未决决定的项，供设计会话与 Morii 确认。标注"已补"的项已在后端实现并有测试。

## 1. 与安全模型或已实现行为不一致（需要设计会话确认）

| # | 设计稿 | 现状 / 安全模型 | 建议 |
|---|---|---|---|
| R-1 | INTERACTION §13：原文件完好、旁边有已验证的临时副本时，该文件"需要决定"，在 History 中选择"使用副本 / 丢弃副本"。 | SAFETY_MODEL §10 恢复表：Ready 状态、原路径 = H0、临时文件存在 → 未提交，**自动删除临时文件**，文件记为未开始；"继续剩余文件"会重新生成同样的结果。真正需要人决定的是原路径内容未知的情况（NeedsAttention），后端提供"保持现状"，并可经撤销 Plan 用备份强制恢复。 | 采用恢复表：不向用户提出"使用副本"。设计中的"需要决定"对应 NeedsAttention（内容与 Journal 不符），选项为"保持现状" / "用备份恢复原内容"（强制恢复，先备份当前内容）。 |
| R-2 | INTERACTION §10：取消对话框写"Stop after the current file?"，进行中的文件"从临时副本回滚"。 | 实现：取消后不再启动新文件；进行中的文件若尚未提交即放弃（删除临时文件，原文件未被修改），已提交的照常完成。两类未完成的文件都记为 `cancelled`，可继续。 | 文案改为"停止：进行中的文件若尚未写入则放弃"。完成摘要的三类计数后端已给出：History 摘要中 `states.done`、`rolled_back`（写入中途被停止、临时输出已丢弃）与其余 `cancelled`（未开始）。 |
| R-3 | INTERACTION §16：删除 Preset 会把文件移到回收站。 | Preset 存在数据库中，不是单独文件。 | 删除改为"删除（可先导出）"，或由后端提供软删除与撤销。 |
| R-4 | INTERACTION §15：空间不足时，"超出限额或可用空间"都会阻止 Apply。 | SAFETY_MODEL §6.3：超出容量上限时按保留策略清理旧备份，而不是阻止；预检只检查可用空间（§6.2）。 | 超出上限时提示将清理的 Operation（它们会失去撤销能力），由用户确认后清理或取消；只有可用空间不足才阻止。 |
| R-5 | SCREEN §4：Absolute 可"设置偏移"；列出"更改时区""从参考同步"。 | Absolute 保留各位置已有的偏移；时区修正取决于 D-18，尚未实现。 | 1.0 中这两项按 D-18 决定显示或隐藏；Absolute 的"设置偏移"同样取决于 D-18。 |
| R-6 | INTERACTION §14："orphan sidecars never edited"（孤立 sidecar 从不编辑）。 | SAFETY_MODEL §3：孤立 XMP 以自身为写入目标（`Embedded`），用户单独选中或文件夹导入时单独列出，可以写入；已实现并测试（PRODUCT_SPEC §6.1 只要求单独列出）。 | 二选一：保持可写（在 Preview 中说明它没有对应的主文件），或改为只读（导入为只读资产，Plan 中为 Unsupported）。决定前后端保持现状。（2026-09-30 补） |

## 2. 设计需要、后端已补

| # | 设计稿 | 后端 |
|---|---|---|
| B-1 | History"当前 vs 执行后"（= 未变 / ≠ 被其他程序改动） | 已补：`history::now_vs_after`，按页请求，逐文件哈希比较 |
| B-2 | History"把备份恢复到文件夹…" | 已补：`history::restore_backups_to`，新文件、不覆盖、核对哈希 |
| B-3 | 备份位置可设置；不可用时一切写入被阻止 | 已补：设置 `backup.root`；执行与继续前检查，不可用 → `BackupUnavailable` |
| B-4 | 只读格式（HEIC、DNG、CR3…）留在选择集中并显示为 Unsupported | 已补：只读资产进入会话，Plan 中为 Unsupported |
| B-5 | 导出日志默认匿名化路径；调试日志有提示 | 已补：导出默认脱敏；调试日志 24 小时自动关闭 |
| B-6 | Library 会话摘要"需要注意"（只读、冲突、导入后被改动、云占位符、darktable sidecar、C2PA） | 已补：`inspect::attention`（`mm-cli attention`）与 `Session::changed_since_import` |

## 3. 设计需要、后端待补（按优先级）

| # | 设计稿 | 需要的后端能力 |
|---|---|---|
| G-1 | INTERACTION §3：Preview 打开期间文件被改动 → 行标 RESCAN，Apply 被阻止直到重新扫描；§4 预检（备份可写、空间、ExifTool、无需重扫） | **已补**：`preflight::preflight` / `PlanBook::preflight`（`mm-cli preflight`），返回需重扫的条目、备份位置、空间与 ExifTool 问题 |
| G-2 | INTERACTION §17：RAW+JPG 成对的文件共享同一时间戳 | **已补**：Sequence 把同目录同名（不区分大小写）的文件视为一个位置（1.x 的 Range / Random 同样适用） |
| G-3 | INTERACTION §15：备份位置必须是本地可写磁盘（拒绝网络盘与可移动介质） | **已补**：执行、继续与预检时按卷类型拒绝（网络盘、可移动介质 → BackupUnavailable；可移动介质无测试介质，未实测） |
| G-4 | INTERACTION §5：确认（如"移除 64 个 JPG 的 GPS"）记录在 Operation 日志中 | **已补**：`Plan::required_acks`（`remove:<字段>`、`unsupported`、`large` >1,000 文件）；`PlanBook::confirm_with` 缺少必需确认时拒绝；确认项写入 Journal（schema v5）与 manifest.jsonl，重建后保留，随导出日志输出 |
| G-5 | PREVIEW 的 6 种类别中有 Warnings（如 EXIF ≠ XMP：两处都会被设置） | **已补**：备注以 `warning: ` 标记为警告（来源不一致、首次使用的导入 Preset），`PlanEntry::warnings()`，摘要 `warnings` 计数（有警告的就绪条目） |
| G-6 | INTERACTION §1：规则构建器在模板变量依赖另一规则所设字段时警告 | **已补**：`Preset::lint`（`{creator}` ↔ Creator，`{year}`/`{month}`/`{day}` ↔ 拍摄时间），`mm-cli presets` 输出 `warnings` |
| G-7 | INTERACTION §7 / SCREEN §14："清除只读属性…"是单独、记录在案的操作 | **已补**：`service::clear_read_only`（经写入闸门、写入日志，提权时拒绝），`mm-cli clear-readonly`；从不自动清除 |
| G-8 | INTERACTION §9：排除单个改动（某文件的某字段）与整项编辑；"排除拍摄时间会一并去掉三个日期标签" | **已补**（2026-09-29，此前评审遗漏）：`PlanBook::exclude_field` / `Plan::set_field_excluded`；多字段条目保留每个字段的写入与校验期望（`PlanEntry::parts`），排除后由其余字段重新合并，因此写入、V2 校验、执行前核对、摘要计数与所需确认都只看仍会写入的改动；被排除的改动在 `excluded_changes` 中供界面划掉显示；`mm-cli plan-exclude`；e2e `a_change_or_a_whole_edit_is_left_out_in_preview` |
| G-9 | INTERACTION §17："Absolute 产生相同时间戳时警告按时间排序会丢失" | **已补**（2026-09-29，此前评审遗漏）：Absolute 作用于多于一个位置（RAW 与同名 JPG 算一个）时，每个将写入的条目带警告（计入 Preview 的 Warnings 类别）；单个位置不警告；e2e `time_tools_apply_and_undo` |
| G-10 | SCREEN 1#large / 1#filters / 1#sort：Library 表格边扫描边显示（进度、取消），按相机、GPS、版权等筛选与排序 | **已补**（2026-09-29，此前评审遗漏；ARCHITECTURE §6.1 已设计但核心未实现，只有 `mm-cli scan`）：`inspect::scan_rows` 按批读取并推送行（字段有效值、相机、镜头、写入去向、未下载、错误原因）；筛选、排序与分组由适配层在行数据上完成；e2e `library_rows_are_read_in_streamed_batches` |

## 4. 需要共同决定

- 界面语言：后端给出的原因与备注目前是英文句子（约 130 处）。中英文界面需要"消息码 + 参数"，由界面按语言渲染；消息码的清单与界面文案宜由设计会话与工程一起定，确定后后端统一改造。现有文本的完整清单（160 条，按 Preview 状态与备注、逐文件结果、错误分类，并标出带有系统或 ExifTool 原文的条目）见 [MESSAGE_INVENTORY.md](MESSAGE_INVENTORY.md)，由 `tools/message_inventory.py` 生成。

## 5. 取决于未决决定

- D-15（隐私范围）：多处建议"改用 Clean export"移除 RAW 内 GPS——Clean Export 是否进入 1.0 未定。
- D-18（时区修正）：见 R-5。
- D-2 / 更新：Settings › Updates 与 First Launch 的更新检查选项依赖 D-4。
