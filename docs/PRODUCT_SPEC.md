# MoriMeta — Product Specification

> **Version:** 0.3（取代 v0.2；v0.1 原件 `MoriMeta_Project_Spec_v0.1.md` 仍是需求基线）
> **Status:** 已批准（2026-09-30）。原标"待决"的事项已在 [DECISIONS](DECISIONS.md) 中决定，以该文件为准。
> **Date:** 2026-09-26
> **Related:** [ARCHITECTURE](ARCHITECTURE.md) · [METADATA_MODEL](METADATA_MODEL.md) · [SAFETY_MODEL](SAFETY_MODEL.md) · [SECURITY_MODEL](SECURITY_MODEL.md) · [DEVELOPMENT_PLAN](DEVELOPMENT_PLAN.md) · [RELEASE_PLAN](RELEASE_PLAN.md) · [RESEARCH_NOTES](RESEARCH_NOTES.md) · [SPIKE_REPORT](SPIKE_REPORT.md)

本文定义 MoriMeta **做什么、不做什么、做到什么程度**。相对 v0.1 的修改集中在 §11，每项注明变更类型；相对 v0.2 的修订见 §13。

## 决策状态

| 事项 | 状态 |
|---|---|
| 公开开源 | **已确定** |
| MVP 时间工具：Absolute、Shift、Sequence、Preserve Relative Timing | **已确定**（v0.1 §33） |
| 具体许可证 | **已决定**：GPL-3.0-or-later（D-1） |
| 隐私功能的 MVP 范围 | **已决定**：(c) Clean Export + 就地移除 GPS（D-15），见 §6.8 |
| Windows 签名路线 | **已决定**：未签名公开预览 → SignPath Foundation，后备 OV（D-2），见 RELEASE_PLAN §4 |
| 时区修正是否进入 MVP | **已决定**：不进入，v1.3（D-18），见 §6.5.2 |

---

## 1. 定位

**MoriMeta 是面向摄影师的 Local-first 批量照片元数据工具。** 它以 ExifTool 为底层引擎，让用户能够对几十到几千张照片**先预览、再执行、可撤销**地修改元数据，并把常用操作保存为可重复执行的 Preset。

MoriMeta 是：

- 一个批量元数据**编辑器**（时间、作者、版权、位置等描述性元数据）
- 一个**规则化**的批处理工具（条件 + 动作 + 模板变量）
- 一个**隐私清理**工具（形式与范围待 D-15）
- 一个**可审计**的修改记录（Operation History + Undo）

MoriMeta 不是：

- 照片管理器 / DAM（不维护长期目录、不做相册、不做同步）
- RAW 转换器、照片编辑器、色彩管理工具
- Lightroom / Capture One 的替代品（与它们共存）
- 云服务（无账户、无上传、无遥测）

一句话验收：**摄影师敢把整次拍摄拖进来，看懂将发生什么，再执行；在备份有效期内、且文件没有被其他程序改动时，可以把修改撤销回原样。**

---

## 2. 优先级与产品原则

### 2.1 优先级（不可调换）

```
数据安全 > 修改可预览 > 修改可撤销 > 批量处理可靠性 > 用户体验 > 性能 > 功能数量
```

任何功能如果无法同时满足前三项，**推迟而不是降级实现**。

### 2.2 原则

| 原则 | 具体含义 |
|---|---|
| **原片默认不被修改的范围** | 专有 RAW 默认只写 XMP sidecar，RAW 文件本身只读；直接修改 RAW 不在 MVP。JPEG/TIFF 的就地写入必须先备份。 |
| **先预览，后执行** | 任何写操作都先生成 Plan 并展示 Preview；执行的是**被预览过的那个 Plan**，不是重新计算的结果。 |
| **单个文件不留半成品；批次不是原子的** | 在 SAFETY_MODEL §0 的前提下，恢复完成后每个文件只会处于修改前或修改后两种状态之一。批次可能部分完成；已提交的文件可以撤销。 |
| **诚实优先于方便** | 不支持的操作显示为 Unsupported 并说明原因；不做静默降级；不使用"移除所有隐私信息"这类无法兑现的措辞。 |
| **Local-first** | 照片与元数据不离开本机；唯一可能的网络行为是用户可控的更新检查（D-4）。 |
| **Photographer-first 词汇** | UI 使用"拍摄时间""作者""版权"等概念；底层 Tag 只在 Advanced 视图中出现。 |
| **Batch-first** | 每个交互都要在 1 / 200 / 5,000 个文件下成立。 |
| **与其他软件共存** | 尊重 Lightroom / Capture One / darktable 的 sidecar 约定，并告知用户它们何时看得见、何时看不见 MoriMeta 的修改（第三方软件的实际行为待 S3 第三方部分验证）。 |

---

## 3. 用户与场景

### 3.1 核心用户

- RAW（优先 Nikon NEF）+ JPEG 工作流的摄影师与爱好者
- 需要在交付或发布前清理隐私信息的人
- 需要修正相机时间、多机位时间同步的人
- 需要批量写入作者、版权、联系方式的人

### 3.2 关键场景（用于验收）

| # | 场景 | 必须成立的体验 |
|---|---|---|
| A | 旅行拍摄 3,000 张（NEF+JPG），相机时钟错了 | 一次 Shift 修正全部文件的拍摄时间；NEF 写入 sidecar、JPG 写入文件；Preview 显示每个文件的前后时间；可以撤销。（相机时区设错时是否同时修正 UTC 偏移，取决于 D-18。） |
| B | 双机位，B 机时钟慢 3 分 42 秒 | 用户输入偏移（或用 Preserve Relative Timing 以一张照片为参照）后，只对 B 机文件执行 Shift；Preview 按合并后的时间顺序展示。 |
| C | 发布前隐私清理 | 用户能在执行前逐项看到每个文件将被移除的具体内容（v0.1 §16），并得到不含 GPS、序列号、所有者等信息的结果。**形式待 D-15**：就地清理原片、导出干净副本，或两者都做。 |
| D | 批量写版权 | 使用 Preset「© {creator} {year}」，只对 Artist 为空的文件写入；混合值清晰可见。 |
| E | 批处理中途崩溃或断电 | 重启后看到中断的 Operation：哪些文件已完成、哪些未处理、哪些需要注意；可以继续、可以撤销已完成部分。（断电情形尚未测试，见 SAFETY_MODEL §0。） |

---

## 4. 核心概念（术语表）

| 术语 | 定义 |
|---|---|
| **Session / Library** | 当前导入到 MoriMeta 的文件集合。MVP 中 Session 不持久化（重启后需重新导入）；历史记录与备份持久化。 |
| **Asset** | 用户视角的一张照片。一个 NEF 与其 `<basename>.xmp` 构成一个 Asset；同名 NEF 与 JPG 是**两个** Asset（UI 中可成组显示）。 |
| **Write Target** | 某个 Asset 的修改实际写到哪里：`Embedded`（写入文件本身）或 `Sidecar`（写入 XMP 文件）。由格式策略决定，不由用户规则决定。 |
| **Field** | MoriMeta 定义的规范字段（如 `capture_time`、`creator`），映射到多个底层 Tag。见 METADATA_MODEL。 |
| **Edit** | 用户对字段的修改意图：保持 / 设置 / 清除 / 时间运算等。 |
| **Rule** | 条件 + 一组 Edit。 |
| **Preset** | 可保存、可复用、可导入导出的有序 Rule 列表。 |
| **Plan** | 对一组文件应用 Edit/Preset 后计算出的**不可变**修改计划：每个文件、每个字段的 before/after、将写入的具体 Tag、警告与不支持项。 |
| **Preview** | Plan 的可视化。用户可以排除文件或单个字段修改（生成新的 Plan 版本）。 |
| **Operation** | 一次被执行的 Plan。拥有 ID、日志、备份、每个文件的状态。 |
| **Backup** | 执行前原文件（或原 sidecar）的逐字节副本。 |
| **Undo** | 用备份把 Operation 中的文件恢复到执行前状态；自身也是一次被记录的 Operation。 |
| **Clean Export** | 生成新文件（副本），只保留白名单内的元数据；不修改原文件。是否进入 MVP 待 D-15。 |

---

## 5. 格式支持分级

基于 ExifTool 能力 [F-03] 与已知兼容性风险 [F-05, F-40]。**"写入"一栏在兼容性验证通过后才会开启。**

| 格式 | MVP 读取 | MVP 写入 | 写入目标 | 之后 | 备注 |
|---|---|---|---|---|---|
| JPEG | ✅ | ✅ | Embedded | — | 首要格式 |
| TIFF | ✅ | ✅ | Embedded | — | 写入需补做 S2/S3 同类验证 |
| Nikon NEF / NRW | ✅ | ✅（仅 sidecar） | Sidecar `<basename>.xmp` | 直接写 RAW（高级，需确认） | MVP 重点 RAW |
| XMP（独立 sidecar） | ✅ | ✅ | 自身 | — | RDF 属性全部保留；XML 注释不保留（SPIKE_REPORT §4） |
| DNG | ✅ | ❌ | （之后）Embedded | 写入 | Adobe 对 DNG 写入文件内部，不读 sidecar [F-40] |
| HEIC / HEIF / AVIF | ✅ | ❌ | — | 视 V-08 | ExifTool 近年有 HEIC 损坏/兼容 bug [F-05] |
| PNG / WebP | ✅ | ❌ | — | 视 V-08 | 许多软件不读 PNG/WebP 的 EXIF |
| CR2/CR3/ARW/RAF | ✅ | ❌ | — | sidecar 写入 | ARW 有历史损坏与兼容问题 [F-05] |
| 其他 | ExifTool 识别则只读显示 | ❌ | — | — | |

---

## 6. 功能规格

标记：**[MVP]** 首个公开版本必须具备；**[v1]** 1.x 系列；**[v2]** 之后；**[待决]** 等人工决定。

### 6.1 Import [MVP]

- 拖放文件/文件夹、Add Files、Add Folder、递归导入（默认开启，可关闭）。
- 不跟随目录符号链接与 junction；不导入系统/隐藏目录（如 `$RECYCLE.BIN`）。
- 以文件身份（卷序列号 + File ID）去重：同一文件经不同路径导入只出现一次。
- 自动识别 sidecar 配对：`<basename>.xmp` 归属同名 RAW；`<basename>.<ext>.xmp`（darktable）识别但只读并提示。
- `.xmp` 不作为独立 Asset 出现，除非没有配对的主文件（孤立 sidecar 单独列出）。
- 导入时标注环境风险：只读属性、云占位符、可移动介质、网络驱动器、符号链接/多硬链接、路径过长、C2PA 内容凭证。
- 导入是增量的：文件列表立即出现，元数据随扫描流式填充；可以取消。

### 6.2 Library / Metadata Table [MVP]

- 虚拟滚动表格，5,000 行保持流畅；多选、Shift/Ctrl 选择、键盘导航。
- 列：文件名、类型、拍摄时间（含时区）、相机、镜头、ISO、光圈、快门、焦距、GPS 状态、作者、版权、评级、写入目标（Embedded/Sidecar）、状态标记。
- 排序、筛选、搜索；列显示/隐藏/调整宽度/重排；布局持久化。
- 状态标记（不只用颜色）：只读、sidecar 冲突、需重新扫描、云占位符、有 C2PA、读取错误。
- 缩略图列 [v1]（MVP 只在 Inspector 中显示单张预览）。

### 6.3 Metadata Inspector（单文件）[MVP]

- 分组：Basic / Capture / Creator / Location / Technical / Advanced。
- **来源可见**：每个字段显示值来自哪里（EXIF / IPTC / XMP / sidecar）；RAW 显示"文件内值"与"sidecar 值"两层及当前生效值。
- **冲突可见**：同一字段在不同位置值不一致时显示冲突标记。
- 可编辑性可见：可编辑 / 受保护（Technical）/ 只读（由格式决定）/ 不支持。
- Advanced：原始 ExifTool 标签（Group:Tag = Value），MVP 只读。
- 单张预览图：提取文件内嵌预览，按需加载。

### 6.4 Batch Editor（多选）[MVP]

- 每个字段显示聚合状态：`相同值` / `混合（N 个不同值）` / `N/M 个文件有值` / `全部为空`。
- 每个字段的动作显式选择：**不变（默认）**/ 设为 / 清除。
- 打开编辑器不产生任何修改；只有显式设置了动作的字段进入 Plan。
- 值支持模板变量（§6.9）。
- 主按钮是 **Preview**，而不是 Apply。

MVP 可批量编辑的字段（v0.1 §33）：拍摄时间（§6.5）、Creator（Artist）、Copyright、GPS。
v1：Title、Description、Keywords、Rating、Credit/Source/Contact、City/State/Country/Location、手动镜头信息。
Technical 字段（Make/Model/ISO/光圈/快门等）在 MVP 只读（C-11）。

### 6.5 Capture Time Tools [MVP]

时间模型见 METADATA_MODEL §5。所有工具先生成 Plan，并在 Preview 中以时间轴形式展示结果。

#### 6.5.1 MVP 工具（v0.1 §33，已确定）

| 工具 | 说明 |
|---|---|
| **Absolute** | 全部设为指定的钟面时间。 |
| **Shift** | 按 ±d h m s 平移钟面时间（相机时钟错了、多机位同步）。已有的 UTC 偏移不变。 |
| **Sequence** | 起始时间 + 固定步长；排序键由用户确认（拍摄时间，或文件名自然排序）。 |
| **Preserve Relative Timing** | 指定一个参照文件的新时间，其余文件按原时间差平移（等价于计算出的 Shift）。 |

共同规则：

- GPS 时间戳（UTC）不随相机时间平移而改变。
- MakerNotes 中的厂商时间字段不修改（与 ExifTool `-AllDates` 一致），Inspector 中说明。
- 同一时间字段在文件中的所有已有位置一并更新（包括 Nikon 文件 IFD0 中非标准的 DateTimeOriginal 与相机写入的 XMP 日期，SPIKE_REPORT §4），不留下互相矛盾的旧值。
- 对 sidecar 写入的 RAW，Preview 提示：只读取文件内 EXIF 的软件仍会看到原时间。
- Windows 资源管理器按钟面时间显示拍摄时间，不使用 OffsetTimeOriginal（SPIKE_REPORT §4）。

#### 6.5.2 时区修正（D-18：v1.3，不进 MVP；MVP 按下文"若不纳入 MVP"处理）

- **依据：** v0.1 §12.2 把"相机时区错误"列为 Shift 的适用场景。Shift 只改钟面时间；文件里如果已有 EXIF 2.31 的 OffsetTimeOriginal（S3 语料中 Nikon Z8、D850 均有）或带偏移的 XMP 日期，只做 Shift 会让钟面时间与偏移互相矛盾。
- **最小形式：** 在 Shift 中增加可选项"同时把 UTC 偏移设为 X"，不另开工具。
- **工作量（估算）：** 领域逻辑与测试 2–3 人日；UI 1–2 人日；主要成本在兼容性验证。
- **验证条件：** S3 第三方部分确认 Lightroom Classic、Capture One 等对 OffsetTimeOriginal 与带偏移 XMP 日期的解释；NEF sidecar 中的偏移是否被读取（V-07）。
- **若不纳入 MVP：** Shift 在检测到文件已有偏移字段时，在 Preview 中提示"偏移未修改"。

#### 6.5.3 之后的候选

| 工具 | 级别 | 说明 |
|---|---|---|
| 仅补/改偏移（钟面时间正确） | v1 候选 | |
| 以两张照片计算机位偏移 | v1 候选 | MVP 中由用户输入偏移，或用 Preserve Relative Timing 完成 |
| Range Distribution | v1 | 在起止时间之间均匀分配 |
| Random Interval | v1 | 随机种子保存在 Plan 中，保证 Preview 与执行一致 |

### 6.6 GPS [MVP]

- 批量设为指定坐标（纬度/经度/海拔，支持十进制与度分秒输入）。
- 批量清除 GPS：见 §6.8.2。
- 地图选点 [v2]；GPX 轨迹同步 [v1.x]。

### 6.7 Creator / Copyright [MVP]

- Creator 写入 EXIF Artist、XMP dc:creator；仅在文件已有 IPTC 时同步 IPTC By-line（MWG 惯例 [F-21]）。
- Copyright 写入 EXIF Copyright、XMP dc:rights（`x-default`）、（已有 IPTC 时）IPTC CopyrightNotice。
- 中文等非拉丁字符必须完整保存。已有 IPTC 且其字符集不能表示新值、或新值超过 IPTC 字节上限时，该字段在该文件上**整体阻止**，Preview 给出三种显式处理（METADATA_MODEL §6）；默认不允许留下互相矛盾的副本。
- 写入方式采用显式映射，不使用 MWG Composite 写入：后者在 Latin IPTC 中会把中文写成 `?`，退出码仍为 0（SPIKE_REPORT §4）。

### 6.8 Privacy [MVP：D-15 = (c)]

#### 6.8.1 范围选项与取舍

v0.1 §33 的 MVP 隐私项是 "Remove GPS" 与 "Remove Serial / sensitive identifiers"；v0.1 §16 要求 "Preview exactly what will be removed"。v0.1 **没有规定**是就地修改原片还是生成副本。

| 选项 | 怎样满足"移除序列号/敏感标识" | 代价 | 验证状态 |
|---|---|---|---|
| (a) 就地清理（GPS + 序列号等） | 从原片（JPEG/TIFF）中移除 | 原片中的序列号被删除，备份过期后无法找回；**MakerNotes 中的序列号不能单独删除**：只能置空（D2Hs/D70 上 ExifTool 未报其他变化，第三方软件影响未验证），改为其他值会破坏 Nikon 加密镜头数据的解码，整体删除 MakerNotes 会丢失镜头名等信息 | ExifIFD 序列号可干净删除；MakerNotes 行为见 SPIKE_REPORT §4 |
| (b) (a) + Clean Export | 两种方式都提供 | MVP 范围最大 | 同 (a) + S7 |
| (c) Clean Export + 就地移除 GPS | 在**导出的副本**中移除；原片保留序列号 | 原片中的序列号不被清除；它服务的是"发布副本"这一工作流，不是就地清理的替代实现 | S7：JPEG 53/53 通过（SPIKE_REPORT §5） |

当前按 (c) 做验证，**范围尚未批准**。无论选哪项，MakerNotes 中的标识都无法单独删除，这一点在 Preview 中如实展示。

#### 6.8.2 就地移除 GPS（各选项共有）

- JPEG/TIFF：就地移除 GPS（有备份、可撤销）。
- 专有 RAW（sidecar 目标）："移除 GPS"的含义是**整个 Asset 在任何位置都不再含 GPS**。RAW 文件内的 GPS 无法通过 sidecar 移除，因此对 RAW 整体显示 Unsupported，不做局部写入，也不显示为已清理。
- "清除 sidecar 中的 GPS 覆盖值"（生效值回到 RAW 内嵌 GPS）是另一个操作，列为 v1 候选。

#### 6.8.3 Clean Export（若按 (b)/(c) 纳入）

- 输入：选中的 JPEG；输出：用户指定目录中的新文件（不覆盖已存在文件；冲突时由用户选择自动编号或跳过）。
- **白名单保留**：用户选择保留哪些类别（相机、镜头、曝光、拍摄时间、作者/版权；方向、色彩空间与 ICC、必需的 EXIF 结构字段总是保留），其余全部移除（METADATA_MODEL §10.1）。
- **Preview 逐项列出**每个文件将被移除的每个标签和每个数据段（含大小），按类别分组并标出高风险项；无法识别的段和标签显示为"未识别 → 移除"。
- 导出后逐个文件检查：段白名单、标签白名单、图像数据哈希不变、预测移除集合与实际移除集合一致。任何一项不满足，该文件不导出，并说明原因。
- **可以对外说明的保证**（S7 验证范围内）：导出的 JPEG 只包含白名单中的数据段和标签，且每个文件都经过上述检查。**不保证**画面内容本身（可见文字、水印、隐写）。在 S7 覆盖更多真实样本（真实 C2PA、HDR gain map、各品牌相机直出 JPEG）之前，不使用"无残留"之类的绝对措辞。

### 6.9 Template Variables [MVP 部分]

- MVP：`{year}` `{month}` `{day}`（来自原拍摄时间）、`{camera}` `{lens}` `{filename}` `{folder}` `{creator}`。
- `{index}` [v1]：必须伴随明确的排序键，Preview 中显示编号。
- 缺失值：默认该文件标为 Warning 并排除该字段修改，或使用默认值语法 `{camera|Unknown}`。**不静默替换为空字符串。**
- 变量取自**原始快照**；同一 Preset 中若同时修改拍摄时间，Preview 提示 `{year}` 取的是原时间。
- 文件名模板属于 Rename Engine [v2]。

### 6.10 Rules [MVP 简化版]

- Rule = 条件（可选）+ 动作列表；Preset = 有序 Rule 列表。
- MVP 条件：字段为空 / 不为空 / 等于 / 包含；文件类型 / 扩展名；多个条件为 AND。
- v1：Any / Not / 嵌套、文件夹名、文件名匹配、日期范围、相机/镜头条件。
- 所有条件基于执行前的原始快照求值；动作按顺序合成，同一字段后者覆盖前者，覆盖在 Preview 中以警告标出；无规则链式依赖。
- "RAW 写入 XMP"不是规则，而是格式策略（C-01）。
- 规则构建器是可读的行式列表（IF … THEN …），可启用/禁用、可重排；不是节点图。

### 6.11 Presets [MVP]

- 创建、编辑、复制、删除、应用（应用 = 生成 Plan → Preview）。
- 存储为带 `schema_version` 的 JSON；可导出，导入需通过 schema 校验。
- 内置通用 Preset：Copyright Template、Remove GPS；（若 D-15 纳入 Clean Export）Clean Export for Web。
- v0.1 中的 Morii / Moriium / Hokkaido 等个人 Preset 作为文档示例，不作为内置项。
- 每个 Preset 显示：摘要、涉及字段、风险级别、最后使用时间。

### 6.12 Preview / Diff [MVP] —— 产品中最重要的界面

- 顶部摘要：文件数、字段修改数；按 Add / Modify / Remove / Warning / Unsupported / Conflict / Blocked / No change 分类计数；按写入目标计数。
- 主体：File × Field × Before × After；可按类型、文件、字段、状态筛选。
- 每行可展开到 Tag 级别：将写入/删除的具体 Tag。
- 可排除单个文件、单个字段修改、整个规则；排除生成新的 Plan 版本并重新汇总。
- 显示执行前检查：磁盘空间、备份位置、只读/锁定文件、导入后被外部修改的文件。
- 主操作按钮在用户查看摘要前不抢眼；高风险 Plan（移除类、影响 > 1,000 文件、含 Unsupported）需要额外确认。
- 5,000 文件的 Plan 在 UI 中流畅浏览（后端分页提供数据）。

### 6.13 Apply / Progress / Summary [MVP]

- 进度：已处理 / 总数、当前文件、成功 / 警告 / 失败 / 跳过计数。
- **取消的语义（界面与文档统一使用）：** 取消会停止后续文件；尚未提交的文件没有被修改；已提交的文件保留修改，可以在备份有效期内、且文件未被其他程序改动时撤销。
- 完成摘要：成功、警告、失败、跳过；操作：查看失败文件、重试失败、撤销整个 Operation、导出日志、完成。
- 错误信息为人话：文件、原因、其他文件是否安全、建议动作（ARCHITECTURE §10）。
- 执行期间禁止：安装更新、未确认就关闭应用、对同一文件发起新的写操作。

### 6.14 History / Undo [MVP]

- History 是 Operation Journal 的视图：时间、名称、文件数、修改数、结果、备份状态。
- 每个 Operation：Inspect（字段级 before/after）、Undo（整批或选定文件）、Retry Failed、Export Log。
- **撤销的条件：** 备份仍在保留期内；文件当前内容等于该 Operation 执行后的内容。文件之后又被修改时标为 Conflict，用户可选择跳过或"强制恢复"（强制恢复前先备份当前内容）。
- Undo 本身是一个 Operation，可以再次撤销。
- 备份按保留策略清理后，History 仍保留记录，但该 Operation 不能再撤销，界面说明原因。

### 6.15 Crash Recovery [MVP]

- 启动时检测未完成的 Operation；在任何其他写操作之前先处理。
- 恢复界面显示：已完成 N、未处理 M、需要注意 K。
- 操作：继续执行剩余文件 / 撤销已完成部分 / 保持现状并关闭。
- **"继续"的含义：** 对同一份已持久化的 Plan 中尚未完成的文件重新做执行前检查后继续；要求应用、字段注册表、ExifTool 版本均未变化；内容哈希不一致的文件进入 Conflict。继续前显示确认页（剩余文件数、新出现的冲突），并提供"重新预览"入口。
- 自动清理 MoriMeta 在照片目录中残留的临时文件（仅限 Journal 中登记过、且哈希核对无误的文件名）。

### 6.16 Settings [MVP]

- General：语言（English / 简体中文）、主题（浅色/深色/跟随系统）。
- Metadata：默认作者/版权模板、时间显示格式、修改后是否保留文件修改时间（默认不保留，见 SAFETY_MODEL §8.9，D-6）。
- RAW：RAW Safe Mode 状态说明（MVP 中恒为开启，不可关闭）。
- Backup：备份位置、保留策略、当前占用、手动清理。
- Privacy & Updates：更新检查（手动 / 自动，首次启动询问，D-4）、渠道、日志隐私说明、打开日志目录。若签名路线要求（RELEASE_PLAN §4），安装程序同时提供关闭更新检查的选项并展示隐私说明。
- Advanced：ExifTool 版本与路径（只读显示）、并发数、调试日志（带隐私警告）。

### 6.17 First Launch [MVP]

最多三屏：① MoriMeta 做什么；② 安全模型（RAW 写 sidecar、先预览、自动备份与撤销及其条件）；③ 隐私（本地处理、更新检查选择）。不做教程轮播。

### 6.18 Metadata Export [v1]

JSON / CSV 导出选中文件的规范化字段（MoriMeta 不依赖任何下游系统）。

### 6.19 Advanced Metadata Mode

- MVP：只读原始标签视图。
- v2：受控写入白名单标签（仍走 Plan / Preview / Backup / 验证）。

---

## 7. 非功能需求

### 7.1 性能目标（初始目标，S4 后校准 [V-06]）

| 规模 | 导入并显示文件列表 | 元数据扫描完成（本地 SSD） | Plan 生成 | 执行（JPEG 就地，SSD） |
|---|---|---|---|---|
| 100 | < 1 s | < 2 s | < 0.5 s | < 10 s |
| 1,000 | < 2 s | < 15 s | < 2 s | < 2 min |
| 5,000 | < 5 s | < 75 s | < 10 s | < 10 min |

读取吞吐与文件内容关系很大：同一简单 JPEG 约 490–715 files/s（单进程），含厂商 MakerNotes 的混合样本约 35 files/s（SPIKE_REPORT §1）。上表在 S4 用真实语料测量前只是目标。
所有规模下 UI 不冻结（交互延迟 < 100 ms）；扫描与执行均可取消；进度至少每 250 ms 更新一次。执行时间以安全为先：备份 + 验证会使写入慢于直接调用 ExifTool，这是有意的。

### 7.2 可靠性（设计目标与前提）

- **目标：** 软件缺陷、进程被终止都不应导致原文件损坏或丢失；恢复完成后每个文件只处于执行前或执行后两种状态之一。
- **前提：** 见 SAFETY_MODEL §0（文件系统、存储刷盘、没有绕过共享模式的并发写入者、备份卷可用、MoriMeta 未被篡改）。已验证的环境：本地 NTFS、SMB（回环）。尚未验证：exFAT/FAT32、云同步目录、断电。
- 单个文件失败不影响其他文件；崩溃后可恢复到明确状态。

### 7.3 可访问性

键盘可完成全部核心流程；可见焦点；状态不只依赖颜色；支持系统缩放 100–200%；屏幕阅读器可读取表格与 Preview 的主要信息（以 WebView2 实测为准 [V-09]）。

### 7.4 国际化

首发 English + 简体中文；所有文案外置；日期/数字按 locale 格式化，但元数据值按原样显示；界面在较长中文标签下不破版。

### 7.5 隐私

无遥测、无崩溃上报、无账户。唯一可能的网络请求是更新检查（可关闭，D-4），请求中不含照片、元数据或路径。日志默认脱敏（SECURITY_MODEL §8）。

---

## 8. 明确不做（Non-goals）

RAW 转换、照片编辑、色彩管理、DAM/持久图库、云同步、账户、AI 标注、人脸/图像识别、社交、画廊托管、NAS 图库管理、视频元数据编辑（v2 以后再评估）、Telemetry、公开 Nightly 渠道。

---

## 9. Roadmap 摘要

详见 [DEVELOPMENT_PLAN](DEVELOPMENT_PLAN.md)。

- **MVP（1.0）**：§5 与 §6 中标记为 MVP 的全部内容；隐私部分以 D-15 的决定为准。
- **v1.x**：DNG 写入、其他 RAW sidecar、更多字段、Range/Random 时间工具、`{index}`、嵌套规则、JSON/CSV 导出、缩略图列、GPX 同步、NEF 直接写入（高级）、HEIC/PNG/WebP 写入（取决于 V-08），以及 D-15 未纳入 MVP 的隐私形式。
- **v2**：Rename Engine、Advanced 写入、Folder Watch、CLI/Headless、macOS/Linux、Preset 分享。

---

## 10. 成功标准

沿用 v0.1 §37 的十条，并补充可测量的门槛（均为发布阻断测试标准，在 SAFETY_MODEL §0 的前提下执行）：

1. 在 5,000 文件语料上的故障注入测试中，不变量检查器 0 违例（S2 原型阶段：NTFS 60 例固定崩溃点 + 300 次随机终止、SMB 30 + 150 次，0 违例）。
2. Preview 中显示的 after 值与执行后重新读取的值 100% 一致（逐文件比对）。
3. 所有 MVP 操作在撤销条件满足时可撤销，撤销后文件与执行前逐字节一致（S2 原型：50/50）。
4. 5,000 文件场景满足 §7.1 目标，UI 无冻结。
5. 用户无需了解 ExifTool 即可完成场景 A–E（可用性测试 ≥ 5 名摄影师）。

---

## 11. 相对 v0.1 的修改（依据与迁移影响）

**变更类型：** `收窄已列 MVP`（v0.1 §33 明确列入 MVP、现被缩小或改变形式，需批准）· `澄清未定范围`（v0.1 未明确规定，v0.3 做出具体规定）· `新增`（v0.1 没有的要求）· `待决`。

| # | v0.1 原文 | v0.3 | 变更类型 | 依据 | 迁移影响 |
|---|---|---|---|---|---|
| C-01 | §13 规则示例 "IF File Type = NEF THEN write compatible metadata to XMP" | 写入目标由格式策略决定，不是用户规则 | 澄清未定范围 | 写入位置是安全属性，不应被规则组合绕过 | 规则 DSL 中没有该类动作；UI 显示每个文件的写入目标 |
| C-02 | §16 Privacy Cleaner 列出可清除的项目，未规定就地或副本；§33 MVP 列 "Remove GPS / Remove Serial / sensitive identifiers" | 形式与范围交由 D-15 决定（§6.8）；就地移除 RAW 内数据不可行 | 待决（涉及 §33 已列 MVP 项的实现形式） | ExifTool 不建议对 RAW 全面删除 [F-04]；MakerNotes 标签不可单独删除 [F-17]，置空/改值的影响见 SPIKE_REPORT §4 | 取决于 D-15 |
| C-03 | §5.2 允许高级用户手动开启 RAW 直接写入；§33 MVP 的 RAW 项只有 "NEF Read" 与 "XMP-oriented safe workflow" | MVP 不提供 RAW 直接写入；v1 仅 NEF，需兼容性验证 | 澄清未定范围 | §5.2 称 MVP 应尽量避免把 RAW 直接修改作为卖点；ExifTool 警告可能损坏文件 [F-04, F-05] | 设置中不出现开关 |
| C-04 | §7 按 UI 分组的嵌套 `interface Metadata` | 扁平 Field Registry + 每格式读取优先级与写入映射 + 来源/冲突 | 澄清未定范围 | Diff/Undo/Preview 需要字段级可枚举键 | 前端按字段 ID 与分组元数据渲染 |
| C-05 | §7 "为未来替换或扩展 Metadata Provider 留空间" | 保留引擎接口仅用于测试替身，不设计多引擎 | 澄清未定范围 | 没有同等的替代引擎 | 无 |
| C-06 | §6.3 后端职责含 Filesystem watch | MVP 不做；改为指纹比对检测导入后外部修改 | 澄清未定范围（Folder Watch 在 v0.1 §35 Phase 2） | 执行前的指纹与内容核对才是安全所需 | 无 |
| C-07 | §19 Undo / Restore Backup，语义未定义 | 基于备份的逐字节恢复 + 内容哈希核对 + 冲突处理；Undo 自身为 Operation；撤销有期限与条件 | 澄清未定范围 | 其他软件可能在之后修改文件 | History 显示冲突与不可撤销原因 |
| C-08 | §2.2 "支持自动 Backup"；§18 示例 "Backup: Enabled" | 就地写入时备份强制，不提供关闭 | 澄清未定范围 | Undo 与崩溃恢复都依赖备份 | 需要空间预检与保留策略 |
| C-09 | §20 `.morimeta/` 在照片目录，或统一应用目录（待定） | 统一应用数据目录（可配置）；照片目录中只短暂出现登记过的临时文件 | 澄清未定范围 | 避免污染用户目录 | Portable 模式推迟 |
| C-10 | §12.2 Shift 适用于"相机时区错误、夏令时、多机位时间同步"；未提 OffsetTime、GPS UTC | 明确时间模型；Shift 不改偏移；GPS 时间不随动；时区修正作为新增建议单列（§6.5.2，D-18） | 澄清未定范围 + 新增建议（待决） | EXIF 2.31 OffsetTime*；S3 语料中 Nikon Z8/D850 已写入偏移 | 取决于 D-18 |
| C-11 | §11.2 列出 Camera 字段并要求标记为 Technical Metadata；§33 MVP 批量编辑只有 Capture Time、Artist、Copyright、GPS | MVP 中 Technical 字段只读；v1 只开放手动镜头信息 | 澄清未定范围 | 与 §33 一致；误改真实拍摄参数代价高 | Inspector 中显示为受保护 |
| C-12 | §4.1 首阶段"优先支持" JPEG/TIFF/PNG/WebP/HEIF/AVIF，未区分读写 | MVP 全部读取；写入仅 JPEG/TIFF/NEF(sidecar)/XMP | 澄清未定范围 | ExifTool 近期 HEIC 问题 [F-05] | 格式表明确标注 |
| C-13 | §15 Morii / Moriium / Hokkaido 等 Preset | 作为文档示例；内置通用 Preset | 澄清未定范围 | 公开发布需要通用性（v0.1 §27 也要求） | 无 |
| C-14 | §28.2 "考虑 … 例如 Stable / Beta / Nightly" | Stable + Beta；Nightly 不公开 | 澄清未定范围 | 小团队难以支撑三个公开渠道 | 无 |
| C-15 | §5.2 Mode B（直接写 RAW）要求"写入后验证文件可重新读取" | 所有写入在提交前验证临时文件（目标值、无附带变化、图像数据哈希、无新增错误），不通过则原文件不被触碰 | 新增 | 事后验证只能发现损坏，不能阻止损坏；S3 中 V3 拦截了 `parseType="Literal"` 被改写 | 执行速度下降（可接受） |
| C-16 | §14 模板变量，缺失值与 `{index}` 排序未定义 | 缺失 → Warning 或默认值语法；`{index}` 必须带排序键（v1） | 澄清未定范围 | 静默空字符串会产生错误元数据 | 无 |
| C-17 | §26 Rename 可与 Metadata Rules 一起执行（Phase 2） | Rename 在 v2，且成组重命名 RAW + `.xmp` + `.acr` | 澄清未定范围 | LR 15 的 `.acr` sidecar [F-41] | 无 |
| C-18 | 未提及只读文件、云占位符、SD 卡、网络盘、链接、C2PA、其他软件覆盖 sidecar | 纳入导入检测、执行预检与提示 | 新增 | ExifTool 可改写只读文件 [F-13]；SAFETY_MODEL §8 | 新增状态标记与文案 |
| C-19 | §33 Batch Edit 含 GPS，形式未定义 | 设为坐标 / 清除；RAW 的"移除 GPS"整体 Unsupported | 澄清未定范围 | sidecar 无法移除 RAW 内数据 | 无 |
| C-20 | 未规定"执行的是被预览的结果" | Plan 不可变并持久化；执行 = 执行该 Plan | 新增 | 否则 Preview 失去意义 | 需要持久化可执行 Plan |
| C-21 | 未提及 C2PA 内容凭证 | 检测 JUMBF/C2PA 并警告、默认排除修改 | 新增 | 修改会使凭证失效（S3：APP11 原样保留，签名随之失效） | 新增状态标记 |

---

## 12. 待人工决策（产品层面）

完整列表见 DEVELOPMENT_PLAN §8。与产品直接相关的：

1. **D-15** 隐私功能的 MVP 范围（(a)/(b)/(c)，§6.8.1）。
2. **D-18** 时区修正是否进入 MVP（§6.5.2）。
3. **D-4** 更新检查默认值（建议：首次启动询问）。
4. **D-8** DNG 写入（建议 v1）。
5. **D-9** Session 持久化（建议 MVP 不持久化）。
6. **D-6** 修改后是否保留文件修改时间（建议不保留）。

---

## 13. 相对 v0.2 的修订

| 位置 | 修订 |
|---|---|
| §1、§2.2、§6.13、§6.14 | 撤销与取消的措辞统一，写明撤销的期限与条件；"每个文件原子"改为有前提的表述 |
| §6.5 | MVP 时间工具恢复为 v0.1 §33 的四项；"修正时区"改为新增建议（D-18）；"仅补偏移""双机参照同步"移到 v1 候选 |
| §6.7、§6.8 | IPTC 部分写入改为整体阻止；隐私范围改为待决（D-15），列出三个选项与各自满足 v0.1 的方式；RAW 的 GPS 移除改为整体 Unsupported；删除"无残留"措辞，改为 S7 验证范围内的表述 |
| §6.15 | "继续剩余文件"改为基于持久化 Plan 的定义 |
| §7.1、§7.2、§10 | 性能数据注明与样本有关；可靠性改为"目标 + 前提" |
| §11 | 增加"变更类型"列；C-02、C-03、C-08、C-11、C-12、C-14、C-15 按 v0.1 原文重写；新增 C-21 |
