# MoriMeta — Repository Checklist

> **Status:** 草案 · 2026-09-27
> 2026-09-27：D-1 与仓库归属已由用户确定，公开仓库 `Morii9961/MoriMeta` 已创建并推送（见 §3）。本文列出每次提交前与首次公开推送前需要逐项确认的内容。

---

## 1. 入库范围

| 路径 | 入库 | 说明 |
|---|---|---|
| `crates/`、`Cargo.toml`、`Cargo.lock` | 是 | 产品代码（Phase 1） |
| `docs/*.md`（工程文档） | 是 | v0.3 草案 |
| `docs/DESIGN.md`、`DESIGN_SYSTEM.md`、`SCREEN_SPEC.md`、`INTERACTION_SPEC.md` | **否（由设计会话负责）** | 设计稿交接后由设计会话决定何时入库；工程侧不修改、不提交 |
| `MoriMeta_Project_Spec_v0.1.md`、`MoriMeta_Design_Brief_v0.1.md` | 本地是；公开前待确认（§3） | 需求基线；含个人化 Preset 示例（Morii / Moriium / Hokkaido） |
| `research/` 下的脚本、原型源码、锁定文件（`exiftool.lock.json`、`corpus.lock.json`）、`README.md`、`PROGRESS.md` | 是 | 可复现的验证依据 |
| `research/results/` | **否** | 生成结果，含本机绝对路径与样本元数据（序列号、标签值）；由脚本重新生成 |
| `research/.work/` | **否** | 下载的 ExifTool、测试语料、实验目录 |
| `target/`、`research/spikes/target/` | **否** | 构建产物 |
| 任何照片、RAW、XMP sidecar、压缩包、可执行文件 | **否** | 测试语料按 URL + SHA-256 获取；将来确需入库的小型合成夹具放在 `research/fixtures/` 并逐个说明来源 |
| `tools/` | 是 | 构建与检查脚本 |

## 2. 每次提交前

1. 只按路径显式 `git add`，不使用 `git add -A` / `git add .`（避免带入设计会话正在写的文件）。
2. 运行 `python tools/check_repo.py`：检查已暂存文件的路径规则、大小（> 1 MiB）、二进制内容，以及内容规则（本机绝对路径、用户目录路径、本机名、电子邮件地址、私钥与常见令牌格式）。有 BLOCK 必须处理，不得绕过。
3. `git diff --cached --stat` 人工过一遍新增文件列表。
4. Rust：`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`。
5. 提交信息写明做了什么与验证方式。

## 3. 首次公开推送前（需要用户确认的项目）

| # | 项目 | 状态 |
|---|---|---|
| P-1 | 许可证（D-1）确定，按 `docs/LICENSE_DECISION.md` 添加 `LICENSE` 等文件，并在 `Cargo.toml` 写入 SPDX 表达式 | 完成：GPL-3.0-or-later |
| P-2 | 仓库归属（个人账号 / 组织）与仓库名 | 完成：个人账号，`Morii9961/MoriMeta`，公开 |
| P-3 | **提交作者身份**：当前全局 Git 身份为 `Morii9961` 及一个个人邮箱。推送后所有提交的作者邮箱会公开。若希望隐藏，应在首次推送前改用 GitHub 的 noreply 地址并重写本地历史（公开后无法撤回） | 用户已允许现有个人邮箱公开，不改写历史 |
| P-4 | v0.1 原始规格与设计简报是否公开（含个人化 Preset 示例与个人网站名） | 用户决定公开 |
| P-5 | 对全部历史运行 `tools/check_repo.py`（`git ls-files` 的全部文件），并人工复核 `docs/` 中的示例值 | 首次推送前对全部已跟踪文件执行，0 拦截 |
| P-6 | 第三方声明：ExifTool 与 Strawberry Perl 的许可证文件、源码获取方式（V-11）；在打包 ExifTool 之前完成 | 打包前 |
| P-7 | `SECURITY.md`（漏洞报告渠道）、`CONTRIBUTING.md`（DCO/CLA 取决于 D-1）、`PRIVACY.md`、`CODE_OF_CONDUCT.md` | 待写 |
| P-8 | GitHub 设置：默认分支保护、必需的 CI 检查、Secret scanning、Dependabot、Actions 以 SHA 固定、最小权限 | 创建仓库时 |
| P-9 | 若走 SignPath（D-2）：代码签名政策页、团队角色（D-16）、全员 MFA | 视 D-2 |
| P-10 | README 中的项目状态如实标注（预发布、无可用版本、未签名等） | README 已按此写 |
