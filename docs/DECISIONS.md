# MoriMeta — 决策记录

> 2026-09-30 · Morii 授权工程侧就全部待决事项自行决定，按最稳妥、对产品最有利的方式处理（"一切都由你自己决定，全部按最理想、最优解处理"）。本文逐项记录决定与理由；与其他文档不一致时以本文为准，直到对应文档更新。四份设计文件（`DESIGN.md`、`DESIGN_SYSTEM.md`、`SCREEN_SPEC.md`、`INTERACTION_SPEC.md`）仍由设计会话维护，本文不改动它们，只写明实现采用哪种语义。
>
> 取舍次序沿用 PRODUCT_SPEC §2.1：安全 > 清晰 > 速度 > 信息密度 > 一致 > 美观 > 新颖。需要对外发出、花钱或改写公开历史的动作不在本授权内自动执行，只记录决定，由 Morii 执行（标 **Morii 执行**）。

## 1. 文档状态

- PRODUCT_SPEC、ARCHITECTURE、SAFETY_MODEL、SECURITY_MODEL、METADATA_MODEL、DEVELOPMENT_PLAN、RELEASE_PLAN v0.3：**批准**，并按本文的决定修订。DEVELOPMENT_PLAN §9 的清单视为完成：设计方向已选定（Direction A，设计 v1.0 冻结），产品 UI 从现在开始（Phase 2/3）。
- 设计 v1.0 与产品规格的冲突按 §4 处理；人工屏幕验收仍保留，在界面可运行后由 Morii 进行。

## 2. 产品范围（DEVELOPMENT_PLAN §8）

| # | 决定 | 理由 |
|---|---|---|
| D-2 签名 | 组合 ①：公开预览未签名（发布页给出 SHA-256 与 GitHub 构建证明）→ 申请 SignPath Foundation，批准后签名发布；申请 8 周未获批准则改用 OV 证书（组合 ②）。Public Beta 起所有安装包签名，同一身份。申请与购买 **Morii 执行** | 零成本起步；开源项目符合 SignPath 条件；后备与切换条件明确 |
| D-3 发布者身份 | 个人（Morii） | 单人项目 |
| D-4 更新检查 | 首次启动询问，默认不勾选；之后在设置中可改 | 本地优先、不未经同意联网 |
| D-5 可移动介质就地写入 | 禁止（维持现状）；不提供解除开关，直到 exFAT/FAT32 写入经过专门验证 | A-1：exFAT 无日志；已实现为 Blocked |
| D-6 修改时间 | 默认更新，设置可保留（维持现状） | SAFETY_MODEL §8.9 |
| D-7 备份保留 | 30 天 + 可用空间 10%（维持现状） | |
| D-8 DNG 写入 | v1.2 | 范围控制（K-7） |
| D-9 Session 持久化 | MVP 不持久化 | 隐私与一致性 |
| D-10 Perl 运行时 | 官方 Windows 包 | 维护成本最低 |
| D-11 平台 | Windows 10 22H2 + Windows 11，x64；ARM64 走仿真，不单独发布 | |
| D-12 文档语言 | 设计与工程文档中文；公开文档（README、用户文档）双语，英文为准 | |
| D-13 验证资源 | 1.0 前仍需要：真实相机语料、LR/C1/NX Studio、断电虚拟机、真实 SD 卡与 NAS、专用云同步目录。没有的环境在发布说明中写为"未验证"，并保持默认限制；不拿合成结果冒充 | 如实 |
| D-15 隐私范围 | **(c)**：就地移除 GPS + **Clean Export**（JPEG）；原片中的序列号不就地删除 | 已有 S7 验证（53/53）；只改副本，原片信息不会永久丢失；MakerNotes 序列号无法干净删除，就地清理不能兑现承诺 |
| D-16 SignPath 角色 | 申请时书面询问（**Morii 执行**） | |
| D-17 ExifTool 调用方式 | **B**（`perl.exe exiftool.pl`）作为发布默认；A 保留为开发与对照 | S0/F-105 证明两者等价；B 不带 CC0 launcher，SignPath 对组件许可证的要求更容易满足；少一个需签名的可执行文件 |
| D-18 时区修正 | **v1（v1.3 与"仅改偏移"一起）**，不进 MVP。MVP 中 Shift 遇到已有偏移字段的文件，在 Preview 中提示"UTC 偏移未修改" | 验证条件（第三方对 OffsetTimeOriginal 的解释，V-07）需要 D-13 资源；MVP 四个时间工具已定 |

## 3. 2026-09-28/30 的工程开放问题（PROGRESS）

| # | 问题 | 决定 |
|---|---|---|
| 1 | 已下载的云文件（OneDrive 等 Cloud Files 重解析点）是否写入 | **继续禁止**，理由与 Preview 文案不变（"经同步客户端写入尚未验证"）。V-04 用专用同步目录验证后再按 SAFETY_MODEL §8.3 改为"允许并提示"。维持 2026-09-28 的决定：安全优先，且拒绝是可恢复的方向 |
| 2 | V1 遇到文件自身缺陷导致 ExifTool 警告时是否接受 | **继续拒绝**。Preview 已提前警告（2026-09-30）。接受警告会让 V1 失去判别力，安全优先 |
| 3 | 向 ExifTool 论坛报告 F-104 | **报告**。工程侧准备报告稿 `research/reports/F-104_exiftool_forum.md`（最小复现，不含私人文件）；发帖 **Morii 执行** |
| 4 | Dependabot glib 告警（研究 spike） | 维持 Morii 2026-09-28 的决定：保留 |
| 5 | R-6 孤立 XMP | **保持可写**（与 Morii 2026-09-28"单独选中的 XMP 可写"一致），但每个条目在 Preview 中带警告："此 sidecar 旁没有对应的照片"。文件夹导入时单独列出（已实现）。INTERACTION_SPEC §14 的"从不编辑"按"不会随其他照片被隐式编辑"实现 |
| 6 | JPEG 自带的 `IMG.xmp` 是否作为读取来源 | **不读取**，保留 Preview 警告。JPEG 的元数据以文件内为准（MWG）；读取它会让有效值取决于一个 MoriMeta 从不写入的文件 |
| 7 | 2026-09-29 十个已推送提交中的 `Signed-off-by` 行 | **不改写历史**。公开历史不强推；该行与仓库的 DCO 要求一致，无害。今后仍不加 |
| 8 | 界面语言的消息码 | **以后端英文模板为消息码**（2026-10-01 实施时确定）：后端继续生成英文句子（Plan、Journal、日志中保存的也是它们，不改持久化格式）；每条句子的格式模板即其消息码，界面用 `apps/desktop/src/i18n/backend.zh.json` 把模板映射为中文，占位符的值（如嵌套的原因）递归翻译，系统或 ExifTool 原文原样保留。CI 以 `tools/message_inventory.py --check` 保证后端每条模板都有翻译、没有失效条目；匹配不到的文本原样显示 |
| 9 | PROGRESS 开头的过时描述 | 更新 |

## 4. 设计 v1.0 与产品规格/安全模型的对齐（DESIGN_HANDOFF_REVIEW、DESIGN_REVIEW_ENGINEERING）

实现采用的语义：

| # | 决定 |
|---|---|
| H-1 提交顺序 | 按 SAFETY_MODEL（备份与 ReadyToCommit 先持久化，再 ReplaceFileW）。界面不承诺"原路径从未缺失"；失败文案依据 Journal 与磁盘核实后的状态 |
| H-2 Retry | 永远生成新 Plan 并展示 Preview，以其版本与确认令牌执行；无变化的条目可折叠，不可跳过 |
| H-3 时间模式 | 可执行：Absolute、Shift、Sequence、Preserve Relative Timing。Change time zone、Sync from reference、Range、Random 显示为禁用并标 `1.x`，带一句说明 |
| H-4 版本 | "设计 v1.0" 指设计交付版本；产品版本另计 |
| H-5 Creator | Creator 是一个规范字段；Artist / XMP creator / By-line 只在来源详情中显示。Credit、Website、Email 在 Inspector 中只读显示（1.x 可写） |
| H-6 Clean Export | 按 D-15 (c) 进入 1.0，入口可用（JPEG） |
| H-7 清除只读属性 | 已实现为单独的显式操作（G-7） |
| R-1 恢复 | 按恢复表：不出现"使用副本"；"需要决定"对应 NeedsAttention，选项为"保持现状" / "用备份恢复原内容" |
| R-2 取消 | 文案："停止：进行中的文件若尚未写入则放弃"；摘要三类计数 done / rolled_back / cancelled |
| R-3 删除 Preset | 删除前提示可先导出；删除后提供一次"撤销删除"（界面内保留已删除 Preset 的内容直到离开该页） |
| R-4 备份超限 | 超出容量上限：列出将被清理的 Operation（失去撤销），用户确认清理或取消；只有可用空间不足才阻止 |
| R-5 Absolute 偏移 | 1.0 不提供"设置偏移"（随 D-18 到 v1.3） |
| R-6 | 见 §3 第 5 项 |
| 设计 §6 的写入范围 | 1.0 写入 JPEG（文件内）与 NEF/NRW（sidecar）；**TIFF 为只读**（Morii 2026-09-28 决定），设计稿中 TIFF 写入按只读处理 |
| "Nothing touches disk before Apply" | 指不在 Apply 前修改源照片或 sidecar；Plan 摘要落库等内部数据不在此列 |
