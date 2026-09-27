# MoriMeta — License Decision Preparation (D-1)

> **Status:** 已决定 · 2026-09-27 — **GPL-3.0-or-later**（用户决定）。
> 已执行：`LICENSE` 为 gnu.org 官方 GPL-3.0 原文（SHA-256 `3972dc9744f6499f0f9b2dbf76696f2ae7ad8af9b23dde66d6af86c9dfb36986`）；workspace `Cargo.toml` 写入 `license = "GPL-3.0-or-later"`，各 crate 继承；README 写明许可声明。产品源文件（`crates/`、`tools/`、`tests/`）已加 `SPDX-License-Identifier: GPL-3.0-or-later` 头（2026-09-28）。尚未做：`CONTRIBUTING.md`（DCO/CLA，§3 第 3 问）、应用内 About → Licenses。以下为决定前的准备材料，保留备查。

## 1. 当前约束（不论选哪种）

- 产品依赖只使用宽松许可（MIT、Apache-2.0、BSD、Zlib、Unicode 等；多许可的依赖选择其中的 MIT/Apache 选项）。截至 2026-09-27，`crates/` 的全部传递依赖均满足（DEVELOPMENT_PLAN §4 Phase 1a）。
- （决定前）`Cargo.toml` 暂不写 `license` 字段；仓库暂无 `LICENSE` 文件，也不做公开推送。
- ExifTool（Artistic 或 GPL，与 Perl 相同）与 Strawberry Perl 组件以独立进程调用、原样再分发，属于聚合，不决定 MoriMeta 自身的许可证；它们的许可证文件随安装包附带（RELEASE_PLAN §7.2）。

## 2. 决定后需要做的事

| 步骤 | Apache-2.0 | GPL-3.0-or-later | 组合（如应用 GPL、核心库 Apache） |
|---|---|---|---|
| 许可证正文 | 从 apache.org 取得官方原文，存为 `LICENSE` | 从 gnu.org 取得官方原文，存为 `LICENSE`（或 `COPYING`） | 每个 crate 目录放各自的 `LICENSE`，根目录说明分配 |
| 版权与声明 | 可选 `NOTICE` 文件 | 每个源文件头或 README 中的版权与"or any later version"声明 | 按 crate 区分 |
| `Cargo.toml` | `license = "Apache-2.0"` | `license = "GPL-3.0-or-later"` | 各 crate 分别设置 |
| 源文件头 | 建议 `// SPDX-License-Identifier: Apache-2.0` | 建议 `// SPDX-License-Identifier: GPL-3.0-or-later` | 按 crate |
| `cargo-deny` 许可证白名单 | 保持宽松许可 | 可加入 GPL-3.0 兼容许可（仍建议优先宽松依赖） | 以最严格的 crate 为准 |
| 贡献方式 | `CONTRIBUTING.md`：DCO（`Signed-off-by`） | 同左；如需将来更换许可证，考虑 CLA | 同左 |
| 前端（npm）依赖 | 同样只允许兼容许可 | 同左 | 同左 |
| 应用内 About → Licenses | MoriMeta 许可证 + 第三方声明 | 同左，另附源码获取方式 | 同左 |

许可证正文必须从官方来源逐字取得，不凭记忆输入。

## 3. 需要用户回答的问题

1. 是否允许他人发布闭源的 MoriMeta 修改版？（允许 → Apache-2.0；不允许 → GPL-3.0-or-later）
2. 是否希望 `mm-exiftool`、`mm-domain` 等核心库能被闭源项目复用？（是 → 这些库可用 Apache-2.0，即使应用本体选 GPL）
3. 将来是否可能更换许可证？（可能 → 考虑 CLA；否则 DCO 即可）
