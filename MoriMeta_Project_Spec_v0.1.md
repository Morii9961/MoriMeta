# MoriMeta

> Photography Metadata Workflow / Batch EXIF Editor  
> **Status:** Planning / v0.1  
> **Product Type:** Local-first desktop application  
> **Primary Audience:** Photographers and photography enthusiasts  
> **Initial Platform:** Windows  
> **Planned Release:** Public release  
> **Core Metadata Engine:** ExifTool

---

## 0. 一句话定义

**MoriMeta 是一个面向摄影师的本地桌面元数据管理与自动化工具。**

它不是简单的 EXIF 编辑器，而是将以下能力整合为一个现代、可靠、可扩展的桌面应用：

- 批量元数据编辑
- 规则化处理
- Preset 工作流
- 隐私清理
- 修改预览
- 安全回滚
- 摄影工作流自动化

核心目标：

> 让摄影师能够安全、可预览、可撤销地，对数十至数千张照片执行复杂的元数据修改。

---

# 1. 项目背景

现有元数据工具普遍存在以下问题：

1. ExifTool 功能极强，但 CLI 使用门槛较高。
2. 很多 GUI 工具更适合单张图片，而非摄影师的批量工作流。
3. 批量编辑能力通常只支持“全部设为同一个值”。
4. 缺少摄影场景真正需要的规则系统。
5. 修改前缺少清晰的 `Before → After` 预览。
6. Undo / Backup / Restore 机制不足。
7. GPS、版权、时间修正、隐私清理等功能分散在不同工具中。
8. RAW 文件直接写入存在较高误操作风险。
9. 很难保存并重复执行一整套工作流。
10. 面向普通摄影师的元数据 UI 往往暴露了过多难以理解的底层 Tag。

MoriMeta 不重新实现完整的元数据解析器，而是：

> **以 ExifTool 作为底层 Metadata Engine，在其之上构建现代、可视化、安全、批量优先的摄影工作流。**

---

# 2. 产品原则

## 2.1 Local First

照片与元数据默认全部在本地处理。

默认：

- 不上传照片
- 不上传 EXIF / IPTC / XMP
- 不要求账户系统
- 不依赖在线服务
- 不收集影像内容

如果未来增加联网功能，例如自动更新、Crash Report 或使用统计，必须明确区分于照片处理流程，并提供清晰的隐私说明与关闭选项。

## 2.2 Non-destructive First

数据安全优先于功能便利性。

任何写入操作必须满足：

1. 写入前可 Preview
2. 用户明确确认后才执行
3. RAW 默认使用更安全的策略
4. 支持自动 Backup
5. 支持 Undo / Restore
6. 单个文件失败不得破坏整批任务
7. 禁止任何静默破坏性写入

原则：

> 永远不要让一次错误点击毁掉一组原片。

## 2.3 Batch First

MoriMeta 的主要场景不是“修改 1 张照片”，而是“修改 20 / 200 / 2000 / 5000 张照片”。

因此所有 UX、性能和任务模型必须优先服务批量处理。

## 2.4 Photographer First

UI 使用摄影师熟悉的概念：

- 拍摄时间
- 相机
- 镜头
- 曝光参数
- GPS
- 作者
- Copyright
- Rating
- Keywords
- Caption
- Location
- Preset

而不是直接把 ExifTool 的大量底层 Tag 原样暴露给普通用户。高级用户可以进入 **Advanced Metadata Mode**。

## 2.5 Public Release First

MoriMeta 计划对外发布，因此从第一版开始必须考虑：

- 安装与卸载体验
- 跨设备兼容
- 自动更新或版本检查
- 清晰的错误提示
- 稳定的配置迁移
- 用户数据目录设计
- Crash-safe 操作
- 日志脱敏
- 隐私政策
- 开源许可证
- 第三方依赖许可证
- ExifTool 分发与调用方式的合规性
- Issue / Bug Report 工作流
- 用户文档
- Release Notes
- 可维护的配置格式
- 版本兼容策略

项目不能只以“开发者自己能跑”为验收标准。

---

# 3. 目标用户

## 3.1 核心用户

- 摄影师
- 摄影爱好者
- RAW 工作流用户
- 需要整理大量照片元数据的用户
- 需要发布前清理隐私信息的用户
- 需要修正相机时间 / 时区的用户
- 需要批量写入版权信息的用户

## 3.2 典型使用场景

### 场景 A：旅行摄影

一次旅行拍摄 3000 张照片，需要：

- 修正相机时区
- 按轨迹补 GPS
- 写入 Creator / Copyright
- 清除公开发布版本中的敏感信息
- 输出供图库 / 网站使用的元数据

### 场景 B：批量发布时间修正

一组照片需要：

- 从指定时间开始
- 每张增加固定或随机间隔
- 保留或重新生成时间关系

### 场景 C：公开发布前隐私清理

保留：

- Camera
- Lens
- ISO
- Aperture
- Shutter
- Focal Length

同时移除：

- GPS
- Serial Number
- Owner Name
- Internal IDs
- Editing History

---

# 4. 第一阶段支持格式

## 4.1 Image

优先支持：

- JPEG / JPG
- TIFF
- PNG
- WebP
- HEIF / HEIC
- AVIF

## 4.2 RAW

第一阶段重点：

- Nikon NEF

后续逐步扩展：

- Canon CR2 / CR3
- Sony ARW
- Fujifilm RAF
- Adobe DNG

## 4.3 Sidecar

- XMP

---

# 5. RAW 文件安全策略

## 5.1 Mode A — XMP Sidecar

默认推荐：

```text
DSC_0001.NEF
DSC_0001.xmp
```

优先将用户可安全外置的数据写入 XMP Sidecar。

目标：

- 尽量避免直接修改 RAW
- 降低原始文件损坏风险
- 提升与常见摄影软件的兼容性
- 提供更清晰的撤销路径

## 5.2 Mode B — Direct RAW Metadata Write

高级用户可以手动开启：

```text
Allow direct RAW metadata editing
```

必须：

- 明确风险提示
- 默认创建 Backup
- 二次确认
- 显示即将修改的 Tag
- 写入后验证文件可重新读取

MVP 阶段应尽量避免把“RAW 直接修改”作为核心卖点。

---

# 6. 推荐技术架构

## 6.1 Desktop

推荐：**Tauri 2**

优势：

- 本地文件系统能力
- 体积较轻
- 便于调用 ExifTool
- 适合 Windows 首发
- 后续可扩展 macOS / Linux
- Rust 适合文件操作、队列和进程管理

## 6.2 Frontend

推荐：

- React
- TypeScript
- Vite

UI 层可考虑：

- Tailwind CSS
- shadcn/ui

但视觉系统应独立设计，不做“默认组件库拼装感”。

## 6.3 Backend

Rust 负责：

- File IO
- ExifTool process management
- Job queue
- Backup
- Restore
- Filesystem watch
- Operation history
- Error normalization
- Cancellation
- Logging

## 6.4 Metadata Engine

核心：**ExifTool**

```text
MoriMeta UI
    ↓
Application Layer
    ↓
Rule / Batch Engine
    ↓
Metadata Abstraction Layer
    ↓
ExifTool Adapter
    ↓
ExifTool
    ↓
Image / RAW / XMP
```

UI 不应直接依赖 ExifTool CLI 参数。

---

# 7. Metadata Abstraction Layer

内部建立稳定的数据模型，例如：

```ts
interface Metadata {
  basic?: BasicMetadata;
  capture?: CaptureMetadata;
  camera?: CameraMetadata;
  lens?: LensMetadata;
  creator?: CreatorMetadata;
  location?: LocationMetadata;
  privacy?: PrivacyMetadata;
}
```

ExifTool Adapter 负责：

```text
ExifTool Tags
        ↕
MoriMeta Metadata Model
```

这样可以：

- 隔离底层 Tag 差异
- 降低 UI 与 ExifTool 耦合
- 为未来替换或扩展 Metadata Provider 留空间
- 更容易做 Preview / Diff / Undo

---

# 8. 核心界面结构

建议：

```text
┌──────────────────────────────────────────────────┐
│ MoriMeta                            248 files     │
├─────────────┬────────────────────────────────────┤
│ Library     │                                    │
│ Presets     │ File Table                         │
│ Rules       │                                    │
│ History     │ filename  date  camera  gps ...   │
│ Settings    │                                    │
├─────────────┴────────────────────────────────────┤
│ Metadata Editor / Rule Editor                    │
│                                                  │
│ Date Taken       2026-09-04 12:27                │
│                 → +2 min / image                 │
│                                                  │
│ Creator          Morii                           │
│ GPS              Remove                          │
│                                                  │
│                Preview Changes                   │
└──────────────────────────────────────────────────┘
```

---

# 9. Library / Import

支持：

- Drag & Drop
- Add Files
- Add Folder
- Recursive Folder Import

导入后快速读取：

- Filename
- File Type
- Resolution
- Date Taken
- Camera
- Lens
- ISO
- Aperture
- Shutter
- Focal Length
- GPS
- Artist
- Copyright
- Rating
- Keywords

---

# 10. Metadata Table

主界面采用批量表格。

支持：

- 多选
- Shift Select
- Ctrl Select
- Search
- Sort
- Filter
- 可配置列
- 大数据量虚拟滚动

建议列：

```text
Filename
Date Taken
Camera
Lens
ISO
Aperture
Shutter
Focal Length
GPS
Artist
Copyright
Rating
Keywords
```

---

# 11. Metadata Editor

## 11.1 Basic

- Capture Date
- Title
- Description
- Rating
- Keywords

## 11.2 Camera

- Make
- Model
- Lens Make
- Lens Model
- Focal Length
- 35mm Equivalent
- ISO
- Aperture
- Shutter Speed
- Exposure Compensation
- Flash
- Software

此类字段应标记为 **Technical Metadata**，避免用户无意中修改真实拍摄信息。

## 11.3 Creator / Copyright

- Artist
- Creator
- Copyright
- Credit
- Source
- Website
- Email
- Contact

Preset 示例：

```text
Creator:
Morii

Copyright:
© Morii 2026
```

## 11.4 Location

- Latitude
- Longitude
- Altitude
- City
- State / Province
- Country
- Country Code
- Location
- Sublocation

---

# 12. Capture Time Tools

这是 MVP 第一优先级功能之一。

## 12.1 Absolute Time

```text
2026-09-04 12:27:00
```

## 12.2 Time Shift

```text
+8 hours
-1 hour
+37 seconds
```

适用于相机时区错误、夏令时、多机位时间同步。

## 12.3 Sequence Time

```text
Start:
12:27:00

Each image:
+2 minutes
```

生成：

```text
001 → 12:27
002 → 12:29
003 → 12:31
```

## 12.4 Range Distribution

```text
Start: 12:27
End:   12:45
```

自动在所选图片之间均匀分配时间。

## 12.5 Random Interval

```text
Interval:
2–3 minutes
```

可选：

```text
Maximum Time:
12:30
```

需要预览所有生成结果。

## 12.6 Preserve Relative Timing

原照片：

```text
12:01
12:04
12:09
```

指定第一张为：

```text
15:00
```

得到：

```text
15:00
15:03
15:08
```

保持原时间差。

---

# 13. Batch Rule Engine

这是 MoriMeta 区别于普通 Metadata Editor 的核心。

建议：

```ts
type MetadataRule =
  | SetValueRule
  | RemoveValueRule
  | DateOffsetRule
  | DateSequenceRule
  | CopyFieldRule
  | ConditionalRule;
```

示例：

```text
IF
File Type = NEF

THEN
Write compatible metadata to XMP
```

```text
IF
GPS Exists

THEN
Remove GPS
```

```text
IF
Artist is empty

THEN
Artist = Morii
```

未来可允许 Filename、Folder Name、Extension、Camera、Lens、Date、Metadata Field 参与条件判断。

---

# 14. Template Variables

支持：

```text
{year}
{month}
{day}
{camera}
{lens}
{filename}
{folder}
{index}
```

示例：

```text
Copyright:
© Morii {year}
```

文件名模板：

```text
{date}_{location}_{index}
```

---

# 15. Preset System

## 15.1 Morii Copyright

```text
Artist = Morii
Copyright = © Morii
Website = moriium.com
```

## 15.2 Public Release

```text
Remove GPS
Remove Serial Number
Remove Owner Name
Remove Sensitive Device IDs

Keep Camera
Keep Lens
Keep Exposure
```

## 15.3 Web Safe

面向网站上传。

## 15.4 Social Media Safe

面向社交媒体上传。

## 15.5 Moriium Safe

面向 Moriium Gallery。

## 15.6 Hokkaido 2027

```text
Creator = Morii
Country = Japan
Region = Hokkaido
Copyright = © Morii 2027
```

---

# 16. Privacy Cleaner

提供一键隐私清理能力。

可清除：

- GPS
- Camera Serial Number
- Lens Serial Number
- Owner Name
- Internal Serial Number
- Embedded Thumbnail
- Editing History
- Software History
- MakerNotes
- Device Identifiers

必须支持：

```text
Preview exactly what will be removed
```

不得使用含糊的“Remove all private metadata”而不告诉用户具体内容。

---

# 17. Preview / Dry Run

所有 Batch Operation 在真正写入前必须经过 Preview。

例如：

| File | Field | Before | After |
|---|---|---|---|
| 001.NEF | Date | 12:21 | 12:27 |
| 002.NEF | Date | 12:23 | 12:29 |
| 003.JPG | GPS | Tokyo | Removed |

顶部显示：

```text
248 files
731 metadata changes
```

并标记：

- Add
- Modify
- Remove
- Warning
- Unsupported

---

# 18. Change Safety

执行前明确显示：

```text
248 files will be modified

12 RAW
236 JPEG

Backup:
Enabled
```

用户确认后才允许：

```text
Apply Changes
```

高风险操作需要额外提示。

---

# 19. History / Undo

每次任务形成一个 Operation。

例如：

```text
09:43
Applied "Moriium Safe"
248 files

09:51
Shifted capture time +8h
248 files
```

支持：

- Inspect Changes
- Undo
- Restore Backup
- Retry Failed
- Export Log

---

# 20. Backup Strategy

建议每次操作生成结构化记录。

可考虑：

```text
.morimeta/
    history.json
    backups/
    operations/
```

或者使用统一的应用数据目录，避免在用户照片目录中制造过多隐藏文件。

正式设计时需要评估：

- Portable project mode
- Global application mode
- Backup retention
- Disk usage
- Backup cleanup
- Recovery after crash

---

# 21. ExifTool Process Model

不要为每张图片单独启动 ExifTool。

应研究并优先采用：

```text
ExifTool -stay_open
```

维护长期进程，避免：

```text
spawn → parse → exit
```

在数千张照片场景中的巨大开销。

需要实现：

- process lifecycle
- timeout
- restart
- malformed output recovery
- cancellation
- queue

---

# 22. 性能目标

测试规模：

```text
100 files
1,000 files
5,000 files
```

要求：

- UI 不冻结
- metadata parsing 后台执行
- incremental loading
- virtualized table
- progress reporting
- cancellable jobs
- 单文件错误不阻断整批
- 写入任务可部分重试

目标：

### 100 images

接近即时加载。

### 1000 images

在合理时间内完成 Metadata Scan。

### 5000 images

仍保持 UI 可交互。

---

# 23. Error Handling

不要只显示：

```text
ExifTool exited with code 1
```

应转换成用户可理解的信息。

例如：

```text
DSC_3921.NEF

Failed to write metadata.

Reason:
File is read-only.

Suggested action:
Check file permissions.
```

批处理结果：

```text
998 succeeded
2 failed
```

提供：

- Retry
- Ignore
- Inspect
- Export Error Log

---

# 24. Logging

开发模式：

- Detailed ExifTool logs
- Debug information
- Performance timing

用户模式只记录必要信息：

- Operation
- Time
- File count
- Result
- Errors

禁止默认记录：

- 图片内容
- 完整 GPS 历史
- 用户隐私字段
- 不必要的绝对路径

公共版本必须考虑日志脱敏。

---

# 25. GPX Sync

建议放入 v1.5 / v2。

支持导入：

```text
track.gpx
```

读取：

- timestamp
- latitude
- longitude
- altitude

根据 Capture Time 自动匹配照片。

需要支持：

```text
Timezone Offset
Camera Clock Offset
Interpolation
Maximum Match Difference
```

典型场景：相机时间比手机轨迹慢 3m42s，修正 Offset 后自动匹配。

---

# 26. Rename Engine

Phase 2 功能。

例如：

```text
001.jpg
002.jpg
003.jpg
```

或：

```text
20270120_Wakkanai_001.jpg
```

模板：

```text
{date}_{location}_{index}
```

可与 Metadata Rules 一起执行。

---

# 27. Moriium Integration

未来可形成：

```text
Z8 RAW
 ↓
MoriMeta
 ↓
Metadata Clean
 ↓
Derivative Pipeline
 ↓
AVIF / WebP
 ↓
Moriium Gallery
```

MoriMeta 可导出结构化 JSON：

```json
{
  "camera": "Nikon Z8",
  "lens": "Viltrox 35mm F1.2 LAB",
  "iso": 64,
  "aperture": 1.8,
  "shutter": "1/320",
  "location": "Wakkanai",
  "date": "2027-01-20"
}
```

供其他系统读取。

注意：Moriium Integration 不应成为 MoriMeta 的硬依赖，公共版本必须保持通用性。

---

# 28. Public Release Requirements

由于 MoriMeta 将对外发布，以下内容从项目早期就应纳入规划。

## 28.1 安装

Windows 首发至少提供：

- 标准 Installer
- 清晰的版本号
- Uninstall
- Upgrade path

后续考虑：

- macOS
- Linux

## 28.2 自动更新

考虑：

- 手动 Check for Updates
- 可选 Auto Update
- Release Channel

例如：

```text
Stable
Beta
Nightly
```

默认不应频繁打扰用户。

## 28.3 Code Signing

正式公开发布时评估：

- Windows code signing
- macOS notarization

避免用户看到过多未知发布者警告。

## 28.4 Privacy

需要公开：

```text
PRIVACY.md
```

至少解释：

- 图片是否上传
- 元数据是否上传
- 日志保存位置
- 是否有 telemetry
- Crash Report 是否上传
- 如何关闭数据收集

原则：

> 默认尽可能不收集。

## 28.5 Telemetry

如果未来需要匿名使用统计，必须：

- 明确说明
- 最小化收集
- 不包含照片
- 不包含完整文件路径
- 不包含 EXIF 内容
- 不包含 GPS
- 可以关闭

MVP 完全可以不做 Telemetry。

## 28.6 第三方依赖与许可证

公开发布前必须检查：

- ExifTool 的使用与再分发方式
- Rust crates
- npm packages
- UI assets
- Fonts
- Icons

需要准备：

```text
THIRD_PARTY_NOTICES
LICENSE
```

不要在未核实许可证前直接打包第三方二进制文件。

## 28.7 Documentation

至少需要：

```text
README.md
INSTALLATION.md
USER_GUIDE.md
PRIVACY.md
SECURITY.md
CONTRIBUTING.md
CHANGELOG.md
```

## 28.8 Bug Reports

Issue Template 至少包含：

- MoriMeta version
- OS
- File format
- ExifTool version
- Steps to reproduce
- Expected behavior
- Actual behavior
- Error log

必须提醒用户：上传日志前检查其中是否包含私人文件路径或元数据。

## 28.9 Security

建立：

```text
SECURITY.md
```

重点关注：

- Path traversal
- Shell injection
- ExifTool argument injection
- Malicious filenames
- Symlink handling
- Arbitrary file overwrite
- Backup restore correctness

禁止通过拼接 Shell Command 的方式调用 ExifTool，必须采用安全参数传递。

---

# 29. Accessibility

公开版本应考虑：

- 键盘导航
- 可读的对比度
- 清晰的 Focus State
- 不仅依赖颜色表达状态
- 可缩放 UI
- 大字体下布局不崩坏

---

# 30. Internationalization

第一阶段架构预留 i18n。

建议首发：

- English
- 简体中文

未来可扩展其他语言。

不要把 UI 文案散落硬编码在组件中。

---

# 31. Configuration

配置必须：

- 版本化
- 可迁移
- 出错时可恢复默认
- 不因升级导致应用无法启动

需要考虑：

```text
config_version
```

---

# 32. Crash Recovery

如果应用在批量写入中崩溃，重新打开后必须能够：

- 识别未完成 Operation
- 显示哪些文件已成功
- 显示哪些文件未处理
- 允许 Retry
- 允许 Restore

避免“任务进行到第 487 张崩溃后完全不知道发生了什么”。

---

# 33. MVP 1.0

MVP 必须严格控制范围。

## Import

- Files
- Folder
- Drag & Drop

## Read

- Basic EXIF
- Camera
- Lens
- Exposure
- Date
- GPS
- Creator

## Batch Edit

- Capture Time
- Artist
- Copyright
- GPS

## Time Tools

- Absolute
- Shift
- Sequence
- Preserve Relative Timing

## Privacy

- Remove GPS
- Remove Serial / sensitive identifiers

## Presets

- Create
- Save
- Apply

## Safety

- Preview
- Backup
- Undo
- Operation History

## RAW

- NEF Read
- XMP-oriented safe workflow

## Public Release

- Installer
- Versioning
- README
- Privacy statement
- Error reporting
- Dependency/license review

---

# 34. 明确的非 MVP 功能

第一阶段不要做：

- RAW Converter
- Photo Editor
- Color Management
- Lightroom Replacement
- Full DAM
- Cloud Sync
- Account System
- AI Tagging
- Face Recognition
- Image Recognition
- Social Network
- Gallery Hosting
- NAS Library Management

MoriMeta 不是 Lightroom。

不要 Scope Creep。

---

# 35. Phase 2

MVP 稳定后考虑：

- GPX Sync
- Rename Engine
- Advanced Rule Builder
- Folder Watch
- Batch Export Metadata
- JSON / CSV Export
- Metadata Copy / Paste
- Multiple Preset Stacks
- More RAW formats
- macOS
- Linux

---

# 36. Phase 3

可能方向：

- Workflow Pipeline
- CLI / Headless Mode
- Plugin System
- Preset Sharing
- Community Preset Repository
- Lightroom / Capture One 辅助工作流
- Local API
- Moriium Adapter

这些均不是当前承诺。

---

# 37. 项目成功标准

MoriMeta 第一阶段成功，不是因为它支持最多字段，而是因为：

1. 用户敢把 1000 张照片拖进去。
2. 用户清楚知道软件将修改什么。
3. 用户可以在执行前完整 Preview。
4. 用户可以可靠 Undo。
5. RAW 用户不会因为默认操作损坏原片。
6. 批量任务不会让 UI 卡死。
7. 错误提示对普通摄影师是可理解的。
8. 用户可以把常见工作流保存成 Preset。
9. 软件安装后无需阅读 ExifTool 手册即可使用。
10. 用户第二次仍愿意继续打开 MoriMeta。

---

# 38. Claude 第一阶段任务

收到此文档后，**不要立即开始大规模实现。**

第一阶段只做研究与架构。

请完成：

1. 检查本文需求是否存在架构冲突或技术风险。
2. 研究 ExifTool 对 JPEG、HEIF、TIFF、NEF、XMP 的读取与写入机制。
3. 研究 RAW 直接修改与 Sidecar XMP 两种方案的安全性与兼容性。
4. 核实 ExifTool 的公开发布 / 打包 / 调用方式及许可证要求。
5. 设计 Tauri + React + Rust + ExifTool 的整体架构。
6. 设计 frontend / backend / domain / metadata engine 模块划分。
7. 设计 Batch Rule Engine 数据结构。
8. 设计 Preview / Dry Run 机制。
9. 设计 Undo / Backup / Transaction Strategy。
10. 设计 Operation Journal 与 Crash Recovery。
11. 设计跨平台 ExifTool 管理策略。
12. 设计配置版本与迁移策略。
13. 分析 Windows 公共发布需要处理的安装、签名和自动更新问题。
14. 做基础 Threat Model，重点检查命令注入、恶意文件名与任意文件覆盖。
15. 给出项目目录树。
16. 制定 MVP → v1 → v2 Roadmap。
17. 给出适合公开仓库的文档结构。
18. 标记所有必须进一步验证、不能凭经验假设的地方。

涉及以下内容时：

- ExifTool 行为
- Tag 可写性
- RAW 安全性
- XMP 兼容性
- 第三方许可证
- Tauri 安全模型
- 平台签名 / 自动更新

**必须查阅官方文档，不要凭记忆推测。**

---

# 39. Claude 第一阶段输出

请生成：

```text
docs/
├── PRODUCT_SPEC.md
├── ARCHITECTURE.md
├── METADATA_MODEL.md
├── SAFETY_MODEL.md
├── SECURITY_MODEL.md
├── DEVELOPMENT_PLAN.md
└── RELEASE_PLAN.md
```

同时在根目录准备：

```text
README.md
LICENSE
CONTRIBUTING.md
SECURITY.md
PRIVACY.md
CHANGELOG.md
```

但在架构文档通过人工确认之前：

> **不要开始大规模实现。**

---

# 40. 优先级

整个项目的优先级必须始终保持：

```text
数据安全
>
修改可预览
>
修改可撤销
>
批量处理可靠性
>
用户体验
>
性能
>
功能数量
```

任何功能如果无法保证安全、可预览、可恢复，应推迟而不是强行加入。

---

# 41. 最终产品愿景

MoriMeta 最终希望成为：

> **一个摄影师敢放心把整次拍摄拖进去处理的 Metadata Workflow Tool。**

它不试图代替 Lightroom，也不试图成为大型数字资产管理系统。

它专注于一件事：

> **把照片元数据处理这件原本复杂、危险、零散的工作，变成清晰、安全、可重复的摄影工作流。**
