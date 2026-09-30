# MoriMeta — Metadata Model

> **Version:** 0.3 · **Status:** 已批准（2026-09-30，按 [DECISIONS](DECISIONS.md) 修订）· **Date:** 2026-09-26
> 本文定义 Metadata Abstraction Layer：MoriMeta 如何把 ExifTool 的底层标签（Tag）映射为摄影师可理解的字段（Field），如何读取调和、如何写入、如何表示变化。
> `[F-xx]`/`[V-xx]` 见 [RESEARCH_NOTES](RESEARCH_NOTES.md)；验证数据见 [SPIKE_REPORT](SPIKE_REPORT.md) §4–§5。

---

## 1. 三层模型

```text
Presentation   分组与文案（Basic / Capture / Creator / Location / Technical / Advanced）
      ▲          —— 纯 UI 元数据，可随设计调整
Field layer    规范字段：capture_time, creator, copyright, gps, …
      ▲          —— FieldRegistry：值类型、读取优先级、写入映射、安全等级
Raw tag layer  (Group1, TagName) → RawValue，来自 ExifTool -json -G1 -a
                 —— 保留全部来源信息，Advanced 视图直接显示
```

v0.1 中按 UI 分组的嵌套接口被替换为**扁平字段注册表**（PRODUCT_SPEC C-04）。

---

## 2. FieldRegistry

### 2.1 字段定义

```rust
struct FieldDef {
    id: FieldId,                         // 稳定字符串 ID，出现在 Preset JSON 与 Journal 中，永不重命名
    value_type: ValueType,
    safety: SafetyClass,
    availability: Availability,          // MVP 可写 / 只读 / v1 …
    read: Vec<ReadSource>,               // 按优先级排序，按格式族区分
    write: WriteMapping,                 // 按写入目标区分（Embedded 的格式族 / Sidecar）
    constraints: Vec<Constraint>,        // 长度（按编码后字节）、字符集、范围
    privacy: Option<PrivacyCategory>,
}

enum SafetyClass { Descriptive, Temporal, Location, Technical, Structural }
```

注册表是**版本化的数据**（`registry_version`）。每个 Plan 和 Operation 记录其使用的注册表版本。

### 2.2 写入总原则

1. **标准位置创建，已有位置全部更新**：新值写入该字段的标准位置；该字段在非标准但已知位置也存在旧值时一并更新（或删除），不留下互相矛盾的副本。例：Nikon Z8/D850 的 NEF 及由其派生的 JPEG 在 IFD0 中也有 DateTimeOriginal（SPIKE_REPORT §4）。
2. **字段在单个文件上要么全部写成，要么整体阻止**：该字段在某个文件上有任何已存在位置写不进去（字符集不能表示、超过字节上限、位置不可写），这个字段在这个文件上整体标为 `Blocked`，Plan 中说明原因并给出显式处理（§6）。默认处理永远不允许留下不一致的副本。
3. **IPTC 仅在文件已有 IPTC 时写入**（MWG 惯例 [F-21]），并受 §6 约束。
4. **lang-alt 字段总是写 `x-default`**，不得使用无语言后缀的写法（会删除其他语言 [F-27]，S0 复现）。
5. **列表型标签**：同一命令中既用 `-tagsFromFile @` 复制又赋值时，必须把被赋值的标签从复制中排除，否则值会被追加而不是替换（S3 实测 By-line 变成 `["Café", "森 Morii"]`）。
6. **每个写入都能被验证**：执行后重读，所有目标 Tag 的值必须等于期望值，否则失败（SAFETY_MODEL §5 V2）。
7. **MakerNotes 永不作为写入目标**（只读显示）[F-17]。

### 2.3 写入映射策略（ADR-08）：采用显式映射

| 方案 | 结果（S3，6 个 JPEG） |
|---|---|
| A. MWG Composite（`-MWG:Creator=`） | 无 IPTC 或 UTF-8 IPTC 时与 B 完全相同；**Latin IPTC 中写入中文时把 By-line 写成 `? Morii`，退出码 0，仅有警告，并把 IPTCDigest 更新为"已同步"** |
| B. 显式映射 | 与 A 结果相同；Latin IPTC 情况下按 §6 先转换为 UTF-8 再写入，值正确，IPTCDigest 更新 |

**决定（草案）：** 采用 B。MWG 写入的上述行为属于静默破坏，不能接受。MWG 规则表仍作为映射设计的参照 [F-21]。
**IPTCDigest：** 写入 IPTC 且文件已有 IPTCDigest 时，同时写 `-Photoshop:IPTCDigest=new`。原因：只改 IPTC 而不更新摘要，读取方（MWG 规则）会认为 XMP 不再同步并忽略 XMP（S0 更正后的 F-28）。

---

## 3. 值类型

| ValueType | 说明 | 相等性（用于 Diff 与验证） |
|---|---|---|
| `Text` | 单值文本 | 精确字符串比较（新输入值 NFC 规范化，已有值原样） |
| `LangAltText` | XMP lang-alt；MVP 只编辑 `x-default`，其他语言原样保留 | 比较 `x-default` |
| `TextList { ordered }` | Creator（Seq）、Keywords（Bag） | 有序：逐项；无序：多重集合 |
| `CaptureTime` | §5 | 钟面时间 + 亚秒 + 偏移 |
| `GeoPoint` | §7 | 纬度/经度 1e-7 度容差，海拔 0.01 m |
| `Rating` | -1、0–5 | 整数 |
| `Number` / `Rational` | 技术字段，只读 | 数值容差 |
| `Enum` | 如 Orientation、Flash，只读 | 枚举值 |

**可接受的值（输入校验）：** 单行字段不接受任何控制字符；多行字段只接受 TAB/LF/CR；任何字段都不接受 NUL、U+FFFE、U+FFFF。这些字符无法经 argfile 精确传递或会被 ExifTool 静默替换（SPIKE_REPORT §2）。空字符串表示"清除"，不作为值写入（ExifTool 的 `-TAG=` 即删除）。

```rust
enum FieldState<T> {
    Absent,
    Present { value: T, source: Location },
    Conflicting { effective: T, source: Location, others: Vec<(T, Location)> },
    Invalid { raw: String, source: Location },   // 如 "0000:00:00 00:00:00"
    Unreadable,
}
```

---

## 4. 读取调和（Reconciliation）

在 Rust 中实现，不使用 `-use MWG` 读取（保留全部来源，避免 strict 模式隐藏数据 [F-21]）：

1. ExifTool 以 `-json -G1 -a -api StructFormat=JSONQ` 读取 SCAN_TAGS（主文件与 sidecar 同一命令）。JSONQ 使所有值都带引号：默认 JSON 会把看起来像数字的字符串、`true`/`false` 转成数字或布尔值，并丢失末尾换行（SPIKE_REPORT §2.2）。
2. 对每个字段，按注册表 `read` 优先级收集候选值（EXIF > XMP > IPTC 为基础顺序；个别字段见 §8）。
3. 全部候选一致 → `Present`；不一致 → `Conflicting`（生效值取最高优先级），Inspector 显示所有来源。
4. **Sidecar 叠加**：对 Sidecar 目标的 Asset，sidecar 中存在的字段值覆盖文件内值（与 Adobe 行为一致，待 V-03 确认），并保留文件内值用于两层视图。注意 Nikon Z/D850 的 NEF 内已有相机写入的 XMP（含 `xmp:CreateDate`、`crd:*`），它属于"文件内值"。
5. IPTC 字符集：`CodedCharacterSet` 不是 UTF-8 时按 ExifTool 默认 Latin 解码 [F-20]；疑似乱码不自动修复，只提示。

调和逻辑是纯函数，位于 `mm-domain`，以表驱动测试覆盖。

---

## 5. 时间模型

### 5.1 表示

```rust
struct CaptureTime {
    local: NaiveDateTime,          // 相机钟面时间（秒精度）
    subsec: Option<SubSec>,        // 保留原始位数的字符串形式，如 "07" / "070"
    offset: Option<FixedOffset>,   // UTC 偏移；未知时为 None（不猜测）
}
```

| 来源 | 格式 | 备注 |
|---|---|---|
| `ExifIFD:DateTimeOriginal` + `SubSecTimeOriginal` + `OffsetTimeOriginal` | `YYYY:MM:DD HH:MM:SS` / 数字 / `±HH:MM` | Exif 2.31+ 才有 Offset；Nikon Z8、D850 已写入 |
| `IFD0:DateTimeOriginal` | 同上 | **非标准位置**，Nikon NEF 中存在；作为"已有位置"更新，不作为读取首选 |
| `XMP-exif:DateTimeOriginal` / `XMP-photoshop:DateCreated` / `XMP-xmp:CreateDate` | ISO 8601，偏移可选 | Nikon 相机内 XMP 有 `xmp:CreateDate`（带亚秒与偏移） |
| `IPTC:DateCreated` + `IPTC:TimeCreated` | `YYYY:MM:DD` + `HH:MM:SS±HH:MM` | |
| MakerNotes（如 Nikon TimeZone、DaylightSavings） | 只读 | 用于提示相机设置的时区；不作为写入目标 |

无效值（全零、空串、月份 00 等）→ `FieldState::Invalid`，时间工具对这些文件只允许 Absolute。

### 5.2 操作语义

**MVP（v0.1 §33，已确定）：** 对每个文件 f，原值 `t = (local, subsec, offset)`：

| 操作 | 结果 |
|---|---|
| `Absolute(L)` | `(L, None, offset)`；亚秒标签删除（不留下旧亚秒） |
| `Shift(Δ)` | `(local + Δ, subsec, offset)`；偏移不变 |
| `Sequence(start, step, order)` | 按 `order` 排序后第 i 个文件：`(start + i·step, None, offset)` |
| `PreserveRelative(anchor, L)` | `Δ = L - anchor.local`，对全部文件执行 `Shift(Δ)` |

**新增建议（D-18，未批准）：** `Shift(Δ, set_offset: Some(O'))`——在 Shift 的同时把偏移设为 `O'`。

**之后的候选：** `SetOffsetOnly(O')`、`SyncFromReference(a, b)`（v1 候选）；`Distribute(start, end, order)`、`RandomInterval(min, max, seed, cap?)`（v1）。

排序键（Sequence / Distribute / `{index}`）：`CaptureTimeThenName`（默认）或 `NaturalFileName`，Preview 首列显示序号。

### 5.3 写入映射（Temporal）

| 目标 | 写入 | 条件 |
|---|---|---|
| Embedded（JPEG/TIFF） | `ExifIFD:DateTimeOriginal`；`ExifIFD:SubSecTimeOriginal`（按 §5.2）；`ExifIFD:OffsetTimeOriginal` 不改（除非 D-18 通过且用户选择） | 总是 |
| | `ExifIFD:CreateDate` + `SubSecTimeDigitized`（+ `OffsetTimeDigitized` 同上） | "同时修改数字化时间"（默认开，与 Lightroom 一致 [F-42]） |
| | 已有的 `IFD0:DateTimeOriginal` | 已存在时更新（§2.2 原则 1） |
| | 已有的 `XMP-photoshop:DateCreated`、`XMP-exif:DateTimeOriginal`、`XMP-xmp:CreateDate` | 已存在时更新；XMP 不存在时不新建 XMP 包（是否应新建待 V-05） |
| | `IPTC:DateCreated` + `IPTC:TimeCreated` | 仅已有 IPTC |
| Sidecar（NEF） | `XMP-exif:DateTimeOriginal`、`XMP-photoshop:DateCreated`、`XMP-xmp:CreateDate`（含偏移） | 具体字段集以 V-07 为准 |
| 不修改 | `IFD0:ModifyDate` 与 `SubSecTime`（与 Lightroom 一致 [F-42]）、GPS 时间戳（UTC）、MakerNotes、文件系统时间（SAFETY_MODEL §8.9） | |

S3 已核对：对 z8.jpg 执行 Absolute 时只改变上述标签，`IFD0:ModifyDate` 与 `SubSecTime` 保持不变。

实现说明（2026-09-27，PHASE1_REPORT）：Embedded 目标中每个已有位置按自身形态写入新钟面时间——仅日期的值仍只写日期，各位置自身的偏移保持不变，亚秒按 §5.2 处理；`XMP-xmp:CreateDate` 按上表“已存在时更新”，与“同时修改数字化时间”选项无关。ExifTool 13.59 不带偏移写 `IPTC:TimeCreated` 时会填入本机时区，因此 IPTC 时间只以文件已有的偏移写入。
**显示差异（S3）：** Windows 资源管理器按钟面时间显示"拍摄日期"，属性值按本机时区换算，**不使用 OffsetTimeOriginal**。

---

## 6. 文本与字符集

| 容器 | 规则 |
|---|---|
| XMP | 总是 UTF-8 [F-20]。 |
| EXIF "ASCII" 字符串（Artist、Copyright…） | 纯 ASCII 直接写入；含非 ASCII 时以 UTF-8 写入（MWG 建议 [F-20]）。Windows 资源管理器正确显示 UTF-8 的 Artist/Copyright（S3）；其他软件显示待 V-05。 |
| IPTC IIM（已有 IPTC 时） | `CodedCharacterSet` = UTF-8 → 直接写入。未定义（按 Latin 解释）且新值可用 cp1252 表示 → 按 Latin 写入。**不能表示时（如中文）该字段整体 Blocked**，Preview 提供三种显式处理：① 把该文件的 IPTC 整体转为 UTF-8（作为独立变更项展示）；② 删除该字段的 IPTC 副本（作为 Remove 项展示）；③ 排除该文件的该字段修改。 |
| IPTC 转 UTF-8 的做法 | `-tagsFromFile @ -IPTC:all --IPTC:<本次赋值的标签> -IPTC:CodedCharacterSet=UTF8 <赋值>`，并 `-Photoshop:IPTCDigest=new`（若已有摘要）。S3：全部原有 IPTC 值保留；新增 `EnvelopeRecordVersion` 与 `CodedCharacterSet`。 |
| IPTC 长度上限 | 取自 ExifTool IPTC 表的 `Format => 'string[0,N]'`（如 By-line 32、CopyrightNotice 128、ObjectName 64、City 32、Keywords 64 字节）。**ExifTool 超长时按字节截断且仍写入，UTF-8 下会切断多字节字符**（S3：`森`×11 = 33 字节，存储结果已损坏）。Planner 必须按**编码后字节数**预检：超长 → 该字段整体 Blocked，Preview 提供：缩短值 / 删除该 IPTC 副本 / 排除。永不截断。 |
| 规范化 | 用户输入的新值做 NFC；已有值原样保留、原样显示。 |
| 控制字符 | 见 §3 可接受的值。 |

---

## 7. GPS 模型

```rust
struct GeoPoint { lat: f64, lon: f64, alt: Option<f64> }   // WGS84，十进制度，海拔米
```

- 输入：十进制度或度分秒；范围校验；7 位小数。
- 写入（Embedded）：`GPS:GPSLatitude/Ref`、`GPS:GPSLongitude/Ref`、`GPS:GPSAltitude/Ref`、`GPS:GPSVersionID`（缺失时）；已存在的 `XMP-exif:GPS*` 一并更新。
- 写入（Sidecar）：`XMP-exif:GPSLatitude`、`XMP-exif:GPSLongitude`、`XMP-exif:GPSAltitude(+Ref)`。
- **移除 GPS 的定义：该 Asset 在任何位置都不再含 GPS。**
  - Embedded：删除 GPS IFD 全部标签 + 已存在的 XMP GPS 标签（含 GPS 时间戳）。不删除地名字段（属于 Location 字段）。
  - Sidecar 目标（专有 RAW）：RAW 内 GPS 无法移除 → 该 Asset 整体 `Unsupported`，不做局部写入。
  - "清除 sidecar 中的 GPS 覆盖值"是另一个操作（结果是生效值回到 RAW 内嵌 GPS），v1 候选。
- 设置坐标时不修改 GPS 时间戳。
- 实现说明（2026-09-27）：未给出海拔时删除已有海拔（否则描述另一个位置）；海拔参考按名称写入（ExifTool 13.59 把写入的 `1` 当作 0，单独写负海拔会丢失符号）；GPS 在规划与验证时按数值读取并以容差比较。

---

## 8. MVP 字段表

`R` = 读取优先级（高 → 低），`W-E` = Embedded 写入，`W-S` = Sidecar 写入；"(existing)" = 仅更新已存在的标签。

| FieldId | 类型 | 安全等级 | R | W-E | W-S | MVP |
|---|---|---|---|---|---|---|
| `capture_time` | CaptureTime | Temporal | ExifIFD:DateTimeOriginal(+SubSec,+Offset) > XMP-exif:DateTimeOriginal > XMP-photoshop:DateCreated > IPTC:DateCreated+TimeCreated（IFD0:DateTimeOriginal 只在前者缺失时参考） | §5.3 | §5.3 | 可写 |
| `digitized_time` | CaptureTime | Temporal | ExifIFD:CreateDate(+…) > XMP-xmp:CreateDate | 随选项 | 随选项 | 只随动 |
| `creator` | TextList(ordered) | Descriptive | XMP-dc:Creator > IFD0:Artist > IPTC:By-line | IFD0:Artist（`; ` 连接）、XMP-dc:Creator、IPTC:By-line(existing IPTC，§6) | XMP-dc:Creator | 可写 |
| `copyright` | LangAltText | Descriptive | XMP-dc:Rights[x-default] > IFD0:Copyright > IPTC:CopyrightNotice | IFD0:Copyright、XMP-dc:Rights-x-default、IPTC:CopyrightNotice(existing IPTC，§6) | XMP-dc:Rights-x-default | 可写 |
| `gps` | GeoPoint | Location | GPS:* > XMP-exif:GPS* | §7 | §7 | 可写/可移除 |
| `title` | LangAltText | Descriptive | XMP-dc:Title > IPTC:ObjectName | — | — | 只读（v1） |
| `description` | LangAltText | Descriptive | XMP-dc:Description > IFD0:ImageDescription > IPTC:Caption-Abstract | — | — | 只读（v1） |
| `keywords` | TextList(bag) | Descriptive | XMP-dc:Subject > IPTC:Keywords | — | — | 只读（v1） |
| `rating` | Rating | Descriptive | XMP-xmp:Rating | — | — | 只读（v1） |
| `city` `state` `country` `sublocation` | Text | Location | XMP-photoshop / XMP-iptcCore > IPTC | — | — | 只读（v1） |
| `make` `model` | Text | Technical | IFD0 | — | — | 只读 |
| `lens` | Text | Technical | Composite:LensID > ExifIFD:LensModel > XMP-aux:Lens | — | — | 只读 |
| `iso` `f_number` `exposure_time` `focal_length` `focal_length_35mm` `exposure_comp` `flash` | Number/Enum | Technical | ExifIFD（MakerNotes 仅显示） | — | — | 只读 |
| `software` | Text | Technical | IFD0:Software > XMP-xmp:CreatorTool | — | — | 只读 |
| `orientation` | Enum | Structural | IFD0:Orientation | — | — | 只读 |
| `dimensions` | — | Structural | File/Composite | — | — | 只读 |
| `serial_numbers` | Text | Technical（隐私） | ExifIFD:SerialNumber、ExifIFD:LensSerialNumber、XMP-aux:*SerialNumber、MakerNotes:SerialNumber | 视 D-15 | — | 只读显示；移除方式视 D-15 |

本表在 S3 第三方部分（V-03、V-05、V-07）完成后冻结为 `registry_version = 1`；此后映射变更需要版本号递增与迁移说明。

---

## 9. 变化模型（Plan 的数据结构）

```rust
struct Plan {
    id: PlanId, version: u32,
    registry_version: u32, exiftool_version: String,
    created_from: PlanSource,           // Edits | Preset{id, snapshot} | Undo{op} | CleanExport{…}
    seed: Option<u64>,
    entries: Vec<PlanEntry>,
    summary: PlanSummary,
}
struct PlanEntry {
    asset: AssetId,
    fingerprint: Fingerprint,           // size, mtime, file_id（+ sidecar 的同样信息）
    target: WriteTarget,                // Embedded{path} | Sidecar{path, exists} | Export{src, dst} | None
    status: EntryStatus,                // Ready | ReadyWithWarnings | NoChange | Unsupported | Conflict | Blocked(reason)
    changes: Vec<FieldChange>,
    tag_ops: Vec<TagOp>,                // Preview 的 Tag 级明细；执行与"继续"都只使用它（持久化，SAFETY_MODEL §9）
    notes: Vec<Note>,
}
struct FieldChange {
    field: FieldId,
    before: FieldState<Value>,
    after:  FieldState<Value>,
    kind: ChangeKind,                   // Add | Modify | Remove
    origin: RuleRef,
    excluded: bool,
}
```

- after 与 before 相等的变化被丢弃；全部丢弃后条目为 NoChange，不写文件。
- Preview 的分类计数由上述结构派生。

### 9.1 批量编辑器的聚合

```rust
enum Aggregate<T> {
    AllAbsent,
    Same { value: T, present: usize, total: usize },
    Mixed { distinct: usize, top: Vec<(T, usize)>, present: usize, total: usize },
    HasConflicts { count: usize },
    HasInvalid { count: usize },
}
```

聚合在后端计算。

---

## 10. 隐私分类目录

用于 Preview 的分组解释、就地移除的明细、导入时的隐私提示。分类只用于**展示分组**；Clean Export 的安全性不依赖分类完整（未分类的内容一律按"其他/未识别"移除，见 §10.1）。

| 类别 | 代表性内容 | 风险说明 |
|---|---|---|
| GPS 坐标 | `GPS:*`、`XMP-exif:GPS*`、`XMP-iptcExt:LocationCreated/LocationShown` 中的坐标 | 精确位置 |
| 地名 | `XMP-photoshop:City/State/Country`、`XMP-iptcCore:Location`、IPTC 对应项 | 位置 |
| 序列号 | `ExifIFD:SerialNumber/LensSerialNumber`、`XMP-aux:*SerialNumber`、`XMP-exifEX:*SerialNumber`、`MakerNotes:SerialNumber/InternalSerialNumber` | 设备可追踪；MakerNotes 内的只能置空或随 MakerNotes 整体删除（SPIKE_REPORT §4） |
| 所有者 | `ExifIFD:OwnerName/CameraOwnerName`、`XMP-aux:OwnerName`、MakerNotes 所有者字段 | 身份 |
| 编辑历史与软件 | `XMP-xmpMM:History/DerivedFrom/Pantry`、`Photoshop:DocumentAncestors`、`IFD0:Software`、`XMP-xmp:CreatorTool`、`XMP-x:XMPToolkit` | 工作流泄露、原始文件名 |
| 内部标识 | `XMP-xmpMM:DocumentID/InstanceID/OriginalDocumentID`、`ExifIFD:ImageUniqueID` | 跨文件关联 |
| 嵌入预览 | `IFD1:ThumbnailImage`、`Photoshop:PhotoshopThumbnail`、JFXX 缩略图、MPF 预览图（EOI 之后的数据）、FlashPix（FPXR） | 可能包含裁剪前的原图 |
| 人物 | `XMP-mwg-rs:RegionInfo`、`XMP-MP:RegionInfo`、`XMP-iptcExt:PersonInImage` | 人脸位置与姓名 |
| 注释 | `ExifIFD:UserComment`、`IFD0:XP*`、`COM` 段 | 自由文本 |
| 内容凭证 | `JUMBF`（C2PA，APP11） | 修改会使签名失效 |
| 厂商数据 | `MakerNotes:*`（整块） | 包含序列号、设置、部分位置/时间 |
| 结构性 | JFIF 段、FlashpixVersion、ExifImageWidth/Height | 不含个人信息，Clean Export 中移除 |
| 未识别 | 未知 APPn 段、EOI 之后的数据、ExifTool 未知标签 | 内容未知，按移除处理 |

### 10.1 Clean Export（若 D-15 纳入）：保留白名单与检查

**保留规格（KeepSpec）：** 用户选择保留的类别被翻译为"删除全部，再从源文件复制回白名单"：

```text
-all= -tagsFromFile @ -ICC_Profile <白名单标签…> -XMP-x:XMPToolkit= -o <输出> <源>
```

| 类别 | 保留的标签 | 默认 |
|---|---|---|
| 方向与色彩（强制） | IFD0:Orientation、ICC_Profile、ExifIFD:ColorSpace、ExifIFD:Gamma、InteropIFD:InteropIndex | 总是 |
| EXIF 结构（强制，从源复制，避免 ExifTool 替换为默认值） | IFD0:XResolution/YResolution/ResolutionUnit、IFD0:YCbCrPositioning、ExifIFD:ExifVersion、ExifIFD:ComponentsConfiguration、InteropIFD:InteropVersion | 总是 |
| 相机 | IFD0:Make、IFD0:Model | 开 |
| 镜头 | ExifIFD:LensMake、LensModel、LensInfo | 开 |
| 曝光 | ExposureTime、FNumber、ISO、ExposureProgram、ExposureCompensation、MeteringMode、Flash、FocalLength、FocalLengthIn35mmFormat、WhiteBalance | 开 |
| 拍摄时间 | DateTimeOriginal、SubSecTimeOriginal、OffsetTimeOriginal | 开 |
| 作者与版权 | IFD0:Artist、IFD0:Copyright、XMP-dc:Creator、XMP-dc:Rights、XMP-xmpRights:* | 开 |
| 标题/描述/关键词 | XMP-dc:Title/Description/Subject | 关 |

MakerNotes 在 Clean Export 中**永不保留**。镜头名仅存在于 MakerNotes 的机型（S3：D70 的 LensID 随 MakerNotes 删除而丢失）在 Preview 中提示"镜头名称将丢失"。

**段白名单（输出）：** SOI、APP0 JFIF、单个 APP1 Exif、单个 APP1 XMP（无 ExtendedXMP）、APP2 ICC_PROFILE、APP14 Adobe、DQT、DHT、DRI、SOFn、SOS + 编码数据、EOI；**EOI 之后不得有任何数据**。

**检查（每个输出文件）：** ① 段解析成功且全部在段白名单内；② 完整标签读取（`-a -G0:1 -u -U`）中每个标签都在标签白名单或结构白名单内；③ `ImageDataHash` 与源相同；④ Preview 预测的移除集合 = 实际移除集合，且没有"预测保留却被移除"的标签。任何一项失败 → 该文件不导出。

**S7 结果（SPIKE_REPORT §5）：** 53 个 JPEG 源（含合成风险样本、MPF、FPXR、十余种未知 APPn、多段 EXIF、EOI 后数据）全部通过；预测与实际 53/53 一致；4 个负对照全部被阻止。

---

## 11. Advanced 视图

- 显示 `-a -G1 -s` 的完整原始标签，按 Group 折叠；标注是否可写、是否 MakerNotes、是否属于隐私分类。
- MVP 只读。v2 写入仍须经过：白名单校验（SECURITY_MODEL §4.3）→ Plan → Preview → 备份 → 验证。
