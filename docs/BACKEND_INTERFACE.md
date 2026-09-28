# 后端接口对照（给 Tauri 适配层）

> 2026-09-28 · 工程文档。ARCHITECTURE §5.2 列出了界面需要的命令；本文把每个命令对应到已实现的 `mm-core` 函数、开发用 `mm-cli` 命令与测试，并标出尚缺的部分。适配层（`src-tauri`，尚未开始）只做转发、文件对话框与拖放、进度 Channel；业务逻辑都在下列函数中。所有写操作都经过 `service::OperationGate`（提权运行时拒绝）。

## 1. 会话与导入

| 命令（ARCHITECTURE §5.2） | mm-core | mm-cli | 说明 |
|---|---|---|---|
| `session_import_dialog` / `session_import_dropped` | `service::Session::import`、`import_folder` | —（`scan`） | 返回 `AssetId`；按卷 + File ID 去重；文件夹导入不跟随目录链接、不读取云占位符；非写入格式为只读资产 |
| `session_rows` | `Session::assets`、`changed_since_import` | — | 分页与排序在适配层；"导入后被改动"按导入时的指纹 |
| `asset_detail` | `inspect::asset_detail` | `inspect` | 字段有效值、来源、冲突、全部原始标签与 sidecar 标签 |
| `selection_aggregate` | `inspect::selection_aggregate` | `aggregate` | 每字段各值的文件数、空、冲突、不可读 |
| 会话摘要"需要注意" | `inspect::attention` | `attention` | 只读、链接、云占位符、darktable sidecar、C2PA、来源冲突、不可读 |

## 2. 规划与 Preview

| 命令 | mm-core | mm-cli | 说明 |
|---|---|---|---|
| `plan_create(selection, edits)` | `planner::plan_creator`、`plan_copyright`、`plan_gps`、`plan_capture_time` | `plan-creator` 等 | `PlanCtl`：进度、取消、并行读取会话数；值可用模板变量 |
| `plan_create(preset_id)` | `planner::plan_preset`（`presets::mark_untrusted`、`presets::used`） | `plan-preset` | 条件基于原始快照；多字段合并为一个条目 |
| 保存 Plan | `service::PlanBook::insert` | — | Plan 不可变，带来源（`PlanSource`） |
| `plan_page` | `PlanBook::page` | — | 按状态筛选，每页 200 行；摘要含警告数 |
| `plan_exclude` | `PlanBook::exclude` | — | 生成新版本；旧版本只读 |
| `plan_preflight` | `PlanBook::preflight`（`preflight::preflight`） | `preflight` | 需重扫的条目、备份位置、空间、ExifTool 版本 |
| 确认对话框 | `Plan::required_acks`、`PlanBook::confirm_with` | —（`apply --ack`） | 高风险 Plan 缺少必需确认时拒绝；确认项随 Operation 记录 |

## 3. 执行

| 命令 | mm-core | mm-cli | 说明 |
|---|---|---|---|
| `op_execute(plan_id, version, token)` | `PlanBook::execute`（`executor::start`） | `apply` | 一次性令牌；备份位置、空间与恢复状态检查；worker 数（即 ExifTool 会话数）用 `settings::workers` |
| 进度 Channel | `ExecOptions.progress`（`ExecProgress`） | — | 每个文件结算后回调；批量合并在适配层 |
| `op_cancel` | `ExecOptions.cancel` | — | 不再启动新文件；提交前的文件放弃；立即结束 ExifTool |
| 继续剩余文件 | `executor::resume` | `resume` | |

## 4. History、撤销、恢复

| 命令 | mm-core | mm-cli | 说明 |
|---|---|---|---|
| `history_list(page)` | `history::list` | `history` | 文件数、修改数、状态计数、撤销链接、保留/已清理、可撤销、回滚数、警告数 |
| `op_detail(id)` | `history::detail`、`now_vs_after` | `show`、`now` | 逐文件前后值；当前 vs 执行后（按页请求） |
| 导出日志 | `history::export_view`、`export_log` | `export-log` | 默认脱敏 |
| `op_undo_plan(id, scope)` | `undo::plan_undo`（`is_forced`） | `plan-undo` | 冲突文件默认排除，可强制恢复；选定文件 = `PlanBook::exclude` |
| 重试失败 | `history::retry_plan` | `plan-retry` | 仍为只读的文件保持排除 |
| 重新规划 | `history::replan` | `plan-again` | 按 Plan 来源对失败、跳过、冲突的文件重新读取 |
| 恢复备份到文件夹 | `history::restore_backups_to` | `restore-to` | 新文件，不覆盖，核对哈希 |
| 启动 | `service::open_data`（单实例锁 + Journal）、`service::startup` | —（`recover`） | 先取单实例锁（另一个 MoriMeta 或 `mm-cli` 占用同一数据目录时 `AnotherInstance`），再打开 Journal 并应用备份位置、程序日志与调试日志设置（适配层不必重复）；崩溃恢复、完成中断的清理、恢复摘要、提权状态与备份位置问题；提权运行时不做恢复（它也会写入） |
| `recovery_status` | `recovery::summary` | `recovery-status` | 恢复对话框的数据 |
| `recovery_resolve` | `recovery::resolve_keep`、`dismiss` | `resolve`、`dismiss` | 需要处理的文件保持现状；保持现状并关闭 |

## 5. Preset、设置、备份

| 命令 | mm-core | mm-cli | 说明 |
|---|---|---|---|
| `presets_*` | `presets::list`、`get`、`save`、`import`、`duplicate`、`delete`；`Preset::lint` | `presets`、`preset-*` | 内置 Preset 只读；导入受大小与数量限制，首次使用标为未信任 |
| `settings_*` | `settings::KEYS`、`get`、`set` | `settings`、`setting` | 已知键、默认值与校验；未知键拒绝 |
| `backup_usage` / `prune_plan` | `retention::usage`、`prune_plan`、`prune` | `backups`、`prune`、`keep` | 保留策略来自设置；未完成的 Operation 从不清理 |
| 清除只读属性 | `service::clear_read_only` | `clear-readonly` | 仅用户显式操作，写入日志 |
| 日志 | `log::init`、`log::set_debug_since` | `debug-log` | 每日文件，7 天 / 50 MB；调试日志 24 小时 |

## 6. 尚缺

- 更新（`update_check` / `update_download` / `update_install`）：依赖 D-2 / D-4 与 S6；`OperationGate::exclusive` 已就绪。
- 界面语言：消息码 + 参数，见 `DESIGN_REVIEW_ENGINEERING.md` §4。
- Clean Export：依赖 D-15。
- 时区修正与"从参考同步"：依赖 D-18。
- 可序列化的 IPC 类型与 TypeScript 类型生成：在适配层开始时一并做。
