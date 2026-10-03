# 后端接口对照（给 Tauri 适配层）

> 2026-10-02 · 工程文档。ARCHITECTURE §5.2 列出了界面需要的命令；本文把每个命令对应到已实现的 `mm-core` 函数、开发用 `mm-cli` 命令与测试，并标出尚缺的部分。适配层已在 `apps/desktop/src-tauri` 实施，负责转发、文件对话框与拖放、进度 Channel；业务逻辑在下列核心函数中。产品写操作经过 `service::OperationGate`（提权运行时拒绝）。

## 0. 启动顺序

1. `service::open_data(data)`：单实例锁 → Journal → 备份位置、程序日志、调试日志设置。`AnotherInstance` 时把拖入的文件交给已运行的实例（ARCHITECTURE §9）。
2. `OperationGate::for_this_process()`（提权时拒绝写入），随后 `service::verify_exiftool(gate, pkg, Scope::Key)`：在 ExifTool 首次运行前校验关键文件；不一致或缺少清单时本次运行禁止写入。
3. 启动 ExifTool 会话（`Engine::start`，校验 `-ver`）；会话数用 `settings::workers`。
4. `service::startup(store)`：崩溃恢复、未完成的清理、需要决定的恢复、备份位置问题。
5. 后台：`service::verify_exiftool(gate, pkg, Scope::All)`。

## 1. 会话与导入

| 命令（ARCHITECTURE §5.2） | mm-core | mm-cli | 说明 |
|---|---|---|---|
| `session_import_dialog` / `session_import_dropped` | `service::Session::import`、`import_folder` | —（`scan`） | 返回 `AssetId`；按卷 + File ID 去重；文件夹导入不跟随目录链接、不读取云占位符、不纳入 MoriMeta 备份库中的文件（`backup_files`，逐个选择时拒绝）；非写入格式为只读资产 |
| `session_rows` | `Session::assets`、`changed_since_import`；`inspect::scan_rows` | `rows` | 行数据由 `scan_rows` 按批读取（每批 400 个文件，读完即经回调推送；`PlanCtl` 进度与取消）：四个字段的有效值（与 Inspector 相同）、相机、型号、镜头、写入去向（`in_file` / `sidecar` / `new_sidecar` / `read_only`）；云占位符不读取（`not_downloaded`），无法读取或内容与扩展名不符时给出原因。分页、排序与筛选在适配层；"导入后被改动"按导入时的指纹 |
| `asset_detail` | `inspect::asset_detail` | `inspect` | 字段有效值、来源、冲突、全部原始标签与 sidecar 标签；文件或其 sidecar 为未下载占位符时 `NotDownloaded`（不读取） |
| `selection_aggregate` | `inspect::selection_aggregate` | `aggregate` | 每字段各值的文件数、空、冲突、不可读、未下载（不读取） |
| 会话摘要"需要注意" | `inspect::attention` | `attention` | 只读、链接、云占位符、darktable sidecar、C2PA、来源冲突、不可读；长路径、可移动介质、网络驱动器、非 NTFS 的本地卷（`other_file_system`） |

## 2. 规划与 Preview

| 命令 | mm-core | mm-cli | 说明 |
|---|---|---|---|
| `plan_create(selection, edits)` | `planner::plan_creator`、`plan_copyright`、`plan_gps`、`plan_capture_time` | `plan-creator` 等 | `PlanCtl`：进度、取消、并行读取会话数；值可用模板变量 |
| `plan_create(preset_id)` | `planner::plan_preset`（`presets::mark_untrusted`、`presets::used`） | `plan-preset` | 条件基于原始快照；多字段合并为一个条目 |
| 保存 Plan | `service::PlanBook::insert` | — | Plan 不可变，带来源（`PlanSource`） |
| `plan_page` | `PlanBook::page` | — | 按状态筛选，每页 200 行；摘要含警告数 |
| `plan_exclude` | `PlanBook::exclude` | `plan-exclude --seq` | 生成新版本；旧版本只读 |
| `plan_exclude_change` / `plan_exclude_edit` | `PlanBook::exclude_field`（`Plan::set_field_excluded`） | `plan-exclude --field F [--seq N]` | INTERACTION §9：某文件的某字段改动，或整项编辑（所有文件的该字段）；被排除的改动移到 `excluded_changes`（显示为划掉），写入与校验期望由其余字段重新合并（`PlanEntry::parts`）；全部改动被排除的文件计为排除；`--include` 恢复 |
| `plan_preflight` | `PlanBook::preflight`（`preflight::preflight`） | `preflight` | 需重扫的条目、备份位置、空间、ExifTool 版本；备份位置在同步目录中时 `backup_warning`（只提示，不阻止） |
| 确认对话框 | `Plan::required_acks`、`PlanBook::confirm_with` | —（`apply --ack`） | 高风险 Plan 缺少必需确认时拒绝；确认项随 Operation 记录 |

## 3. 执行

| 命令 | mm-core | mm-cli | 说明 |
|---|---|---|---|
| `op_execute(plan_id, version, token)` | `PlanBook::execute`（`executor::start`） | `apply` | 一次性令牌；备份位置、空间与恢复状态检查；worker 数（即 ExifTool 会话数）用 `settings::workers`；没有可写入的条目时拒绝（"nothing to write"），不产生 Operation |
| 进度 Channel | `ExecOptions.progress`（`ExecProgress`） | — | 每个文件结算后回调；批量合并在适配层 |
| `op_cancel` | `ExecOptions.cancel` | — | 不再启动新文件；提交前的文件放弃；立即结束 ExifTool |
| 继续剩余文件 | `executor::resume` | `resume` | |

## 4. History、撤销、恢复

| 命令 | mm-core | mm-cli | 说明 |
|---|---|---|---|
| `history_list(page)` | `history::list` | `history` | 文件数、修改数、状态计数、撤销链接、保留/已清理、可撤销、回滚数、警告数；备份文件夹不在（硬盘未连接）时 `backups_unavailable`，不可撤销 |
| `op_detail(id)` | `history::detail`、`now_vs_after` | `show`、`now` | 逐文件前后值；当前 vs 执行后（按页请求；未下载占位符为 `not_downloaded`，不读取） |
| 导出日志 | `history::export_view`、`export_log` | `export-log` | 默认脱敏 |
| `op_undo_plan(id, scope)` | `undo::plan_undo`（`is_forced`） | `plan-undo` | 冲突文件默认排除，可强制恢复；选定文件 = `PlanBook::exclude` |
| 重试失败 | `history::retry_plan` | `plan-retry` | 仍为只读的文件保持排除 |
| 重新规划 | `history::replan` | `plan-again` | 按 Plan 来源对失败、跳过、冲突的文件重新读取 |
| 恢复备份到文件夹 | `history::restore_backups_to` | `restore-to` | 新文件，不覆盖，核对哈希；先写入 `…​.mmrestore-N`，完整且核对后才以不覆盖的改名取得照片的名字，中断不会留下看似完整的原片 |
| 启动 | `service::open_data`（单实例锁 + Journal）、`service::startup` | —（`recover`） | 先取单实例锁（另一个 MoriMeta 或 `mm-cli` 占用同一数据目录时 `AnotherInstance`），再打开 Journal 并应用备份位置、程序日志与调试日志设置（适配层不必重复）；崩溃恢复（备份文件夹不在的 Operation 列在 `recovery_waiting`，暂不恢复、不阻止启动）、完成中断的清理（无法完成的列在 `prunes_left`，不阻止启动）、恢复摘要、提权状态与备份位置问题；提权运行时不做恢复（它也会写入） |
| 重建 Journal（数据库丢失或损坏） | `Store::import_from_backups`、`import_from_backups_in(extra)`，随后 `service::startup` | `rebuild-journal [--from DIR]` | 扫描默认、当前与 `<data>/backup-locations.txt` 中登记过的每个备份位置；数据文件夹整个丢失时由用户指定文件夹 |
| `recovery_status` | `recovery::summary` | `recovery-status` | 恢复对话框的数据 |
| `recovery_resolve` | `recovery::resolve_keep`、`dismiss` | `resolve`、`dismiss` | 需要处理的文件保持现状；保持现状并关闭 |

## 5. Preset、设置、备份

| 命令 | mm-core | mm-cli | 说明 |
|---|---|---|---|
| `presets_*` | `presets::list`、`get`、`save`、`import`、`duplicate`、`delete`；`Preset::lint` | `presets`、`preset-*` | 内置 Preset 只读；导入受大小与数量限制，首次使用标为未信任 |
| `settings_list` / `setting_set` / `settings_reset` / `settings_migrations` | `settings::KEYS`、`get`、`set`、`reset_all`、`Store::migrations()` | `settings`、`setting`、`migrations` | 已知键、默认值与校验；未知键拒绝；设置或清空备份路径立即更新 Store；调试日志立即生效；重置恢复默认路径，早期 Operation 仍用其登记的备份位置；迁移记录为 `(time_ms, description)` |
| `backup_usage` / `backup_keep` / `prune_preview` / `prune_execute` | `retention::usage`、`backups::keep`、`backups::PruneBook` | `backups`、`prune`、`keep` | 逐 Operation 的大小、保护原因、总占用与同步目录警告；清理先预览，返回一次性 token，执行前重查选中项与策略；新预览使旧 token 失效；未完成的 Operation 从不清理。设置中的管理界面已接入逐操作保留、策略/手动清理预览、撤销能力丢失确认与执行 |
| 清除只读属性 | `service::clear_read_only` | `clear-readonly` | 仅用户显式操作，写入日志 |
| ExifTool 完整性 | `service::verify_exiftool`（`Scope::Key` 启动前、`Scope::All` 之后在后台；不一致或缺少清单时自行 `OperationGate::refuse_writes`） | `exiftool-check`、`exiftool-manifest` | SECURITY_MODEL §5；不一致时禁止写入并提示重新安装 |
| 日志 | `log::init`、`log::set_debug_since` | `debug-log` | 每日文件，7 天 / 50 MB；调试日志 24 小时 |

## 6. 尚缺

- 更新后端（`update_status` / `update_check` / `update_download` / `update_cancel` / `update_install`）：已接入 Tauri updater 2.10.1。未配置构建环境变量 `MORIMETA_UPDATER_PUBLIC_KEY` 时不联网；每周自动检查仅在选择 weekly 后启用，失败尝试也登记时间。下载限定本项目 GitHub HTTPS 的 exe，限制 128 MiB；验签后要求 trusted comment 中唯一的 `version:` 字段与清单一致且比当前版本新。安装必须显式调用，通过独占门禁、无待恢复操作、Journal checkpoint 后才能交给安装器。设置界面已接入手动检查、下载进度、取消和安装确认。公钥、带签名版本的发布产物和 S6 真实更新验证仍待完成。测试签名来自临时密钥，与正式发布无关。
- 界面语言：已实施（DECISIONS §3 第 8 项）：后端英文模板即消息码，界面翻译表 `apps/desktop/src/i18n/backend.zh.json`，CI 检查覆盖。
- Clean Export：已实施（`mm-core::clean_export`，`clean_plan` / `clean_entry` / `clean_export` 命令）；`clean_export(plan_id, number_taken)` 校验界面显示的预览 id，已被替换的预览拒绝。2026-10-02 接入写入门禁，完整性失败、提权或已有写操作时拒绝；启动恢复未处理完时拒绝。
- 时区修正与"从参考同步"：按 DECISIONS D-18 属后续版本，不是 MVP 未完成项。
- 适配层：`apps/desktop/src-tauri`（命令见 `src/cmd.rs`，前端镜像类型 `src/ipc/types.ts`，手工维护）。
- `exec.workers` 在下一次执行或恢复前调整会话池；拒绝并行写入时保留当前操作的取消句柄，结束时自动清理。`undo_plan(op_id, force_seqs)` 显式指定冲突项，仍须预览与确认；`plan_again` 重新读取并规划；`recovery_keep` 保留所选文件现状。执行和恢复共用进度事件。Journal 重建已在核心与开发 CLI 支持，桌面损坏修复入口尚未接入。
