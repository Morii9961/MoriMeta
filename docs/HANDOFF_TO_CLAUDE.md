# MoriMeta — Claude 接手记录

> 2026-09-27。此文件是工作索引，不替代当前仓库、测试输出和规格原文。Claude 接手后应先独立检查 `git status`、最近提交及相关文档，再判断下一步。

## 已确认的工作

1. 接手时 `main` 有 6 个本地提交，HEAD 为 `d85d330`；此前交接称 JPEG + Creator 的 `mm-cli` 链路和 53 个测试已完成。本轮从源码、Plan、Journal、测试与文档重新核查了相关范围，没有仅凭旧报告宣布通过。
2. 工程侧只读核对了四份新设计文件：`DESIGN.md`、`DESIGN_SYSTEM.md`、`INTERACTION_SPEC.md`、`SCREEN_SPEC.md`。逐项冲突、原文位置和实施影响见 [DESIGN_HANDOFF_REVIEW.md](DESIGN_HANDOFF_REVIEW.md)。四份文件本轮未修改、未暂存、未提交；它们由设计会话负责。`FROZEN · v1.0` 是设计交付状态，不是产品规格 v0.3 获批或 Morii 完成人工屏幕验收。
3. Phase 1b 的 1,000 JPEG Creator→Undo 条件已用 ExifTool 的 8 种测试夹具各复制 125 份验证：Plan 1,000 Ready；Apply 1,000 Done；独立复读 Creator 1,000/1,000；Undo 1,000 Done；逐文件 SHA-256 1,000/1,000 回到写入前；两次 `fsck` 均 0 问题。完整条件、样本哈希、首次受限环境失败和局限见 [PHASE1B_SCALE_VALIDATION.md](PHASE1B_SCALE_VALIDATION.md)。该结果不代表真实相机语料、第三方软件或 S4 性能。
4. 为避开 Windows 命令行长度限制，开发用 `mm-cli` 的 `scan`、`plan-creator` 加入 `--files-from UTF8_FILE`；规模验证脚本在 `research/s4/verify_phase1b_scale.py`。源样本、测试副本、原始结果、数据库及备份都留在 Git 排除目录，不纳入提交。
5. 本轮 `cargo test --workspace` 54/54 通过；Rust 格式、Clippy（warnings 视为错误）、Python 脚本语法及仓库敏感信息检查通过。Windows 文件替换测试在受限环境被拦，最小探针定位后在可运行该 API 的环境重测通过。

## 设计与产品边界

- 最重要的安全冲突：`INTERACTION_SPEC.md:86` 的“atomic swap → journal entry”与 `SAFETY_MODEL.md:100-115,145-154` 的持久化顺序及进程终止中间态相反。实现必须遵守安全模型；UI 文案不能无条件说原路径从未短暂缺失。
- `INTERACTION_SPEC.md:95` 的 Retry 可跳过 Preview 与 `PRODUCT_SPEC.md:60`、后端 Plan 版本和确认令牌门禁不一致。Retry 应生成并展示新 Plan 的 Preview。
- 时间工具在 MVP 仅四项；设计列出的时区、双机参照、Range、Random 尚受 D-18 或 v1 范围限制。Clean Export 受 D-15 限制。Creator/Artist 的规范字段与底层标签要区分。详见核对文件。
- 暂不开始产品 UI。设计稿的 `.dc.html` mock 文件不在当前仓库；即使工程侧实现前取得，也须保留人工屏幕验收这一步。

## 下一个工作顺序

1. 独立检查当前分支、未跟踪文件、提交范围；核对 [PHASE1_REPORT.md](PHASE1_REPORT.md)、[DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md) §4/§5、[SAFETY_MODEL.md](SAFETY_MODEL.md)、[SECURITY_MODEL.md](SECURITY_MODEL.md) 及两份本轮记录。核查提交/测试状态，不以本文代替现场结果。
2. 设计冲突交给产品/设计所有者对齐，工程侧保留四份原设计文件原样。D-15、D-18 与四种/后续时间模式的 UI 表述需要 Morii 后续决定；D-1（许可证）与 GitHub 仓库归属未定，暂不创建或推送公开仓库。Morii 已同意现有个人邮箱公开，**不用改写六个已有提交的历史**。
3. 继续独立于设计决议的核心工作，优先补 Phase 1b 安全验证缺口：`PHASE1_REPORT.md` G-1 的真实磁盘满和 Journal 写入失败，以及 `SAFETY_MODEL.md` §12 的 JPEG 故障矩阵。若所需设备/权限缺失，先完成可复现的本地故障注入与不变量检查，再明确列出仍需的环境。不要以模拟结果冒充断电、NAS、exFAT 或真实磁盘满测试。
4. D-13 资源到位后，用 **1,000 张不同的相机 JPEG 副本**与真实大文件在目标存储上补规模/性能检查；第三方显示兼容性按 `tests/compat-lab/README.md` 人工矩阵进行。请 Morii 提供资源或确认首发不覆盖的环境，而不是从 ExifTool 合成/重复夹具外推结论。
5. 每项完成的改动按 [REPOSITORY_CHECKLIST.md](REPOSITORY_CHECKLIST.md) 显式暂存路径、运行 `python tools/check_repo.py`、检查暂存 diff，并执行 Rust 格式、Clippy、workspace 测试。四份设计文件、`research/.work/`、`research/results/`、照片/RAW、私人路径或原始元数据均不入库。若受限执行环境让 `ReplaceFileW` 返回错误 5，先用最小探针区分环境限制与产品失败，再在获准环境中运行必要测试。

## 尚需 Morii 的决定或资源

- **当前 1,000 副本验证不需要补充资源**，已完成；核心实现暂不因设计决议停工。
- D-15 隐私功能范围、D-18 时区修正是否进 MVP，以及人工屏幕验收，均在相应产品/UI 阶段需要。
- D-13 的相机原片副本、第三方软件与测试机、真实存储/断电等环境，分别用于真实语料、兼容性和环境风险验证。许可证 D-1 与仓库归属只在公开仓库前必须确定。
