# MoriMeta — Phase 1b 千张 JPEG 写入与撤销验证

> 2026-09-27；本记录只证明下述样本和环境中的 `mm-cli` 核心链路。原始运行结果位于被 Git 排除的 `research/results/s4/`，样本副本、Plan、Journal 和备份位于被排除的 `research/.work/s4/`。可用 `research/s4/verify_phase1b_scale.py` 重跑。

## 目标与条件

- 目标：核对 `DEVELOPMENT_PLAN.md` §4 Phase 1b 的“1,000 个 JPEG 执行 Creator 修改与 Undo”规模条件，不宣称整个 Phase 1b 已退出。
- 基线：运行前 `main` 指向 `d85d330380ff3bd9df8854ea07c1bc62f026aeca`；新增 `--files-from UTF8_FILE` 只解决 1,000 路径超过 Windows 命令行长度的问题。
- 系统：Windows 11 build 10.0.26200；工作卷 E 为本地 NTFS；`mm-cli` release 构建；锁定的 ExifTool 13.59、官方 launcher；单 ExifTool 会话、单文件顺序处理，未实现 worker pool。未测量杀毒软件开关状态。
- 数据：从 ExifTool 13.59 源码包的 `t/images/` 取 8 张 JPEG 测试夹具，每张复制 125 份，得到 1,000 个独立路径，总输入 3,021,375 字节。只改副本，不改源夹具。下表哈希为每种源夹具的 SHA-256。

| 样本 | 字节 | SHA-256 |
|---|---:|---|
| Writer.jpg | 251 | `c4ccfb8dc64caf6622d8cf330741dff990fb33b73aa274010352db02aa161aaa` |
| Nikon.jpg | 1,703 | `26e9f26631281d399002b28ac2bb468cbb8cb12ffc7a309631bbfb18fed410be` |
| Canon.jpg | 2,697 | `98c290283dbff10950bd0eac63bf95804fd09661de727df4f2946d203ca9e7a2` |
| XMP.jpg | 10,314 | `8b39cd1636913ef5027dd6816b006a3942e6df5377aa2ebfcf082916e9859b39` |
| Sony.jpg | 2,779 | `7376ad79cad2ac750e765ad253df2f3ae297c2089a562d00d88e9338ea8980b7` |
| Olympus.jpg | 1,573 | `86265e9c942bf3a4f72e8b69f238bc63bbaa93f77184e4623b4f13aba1c366f8` |
| Pentax.jpg | 2,721 | `554756d3cbbebad49d8ab67ecb8add51740a022015f97d6bd97acce87551a3c6` |
| GPS.jpg | 2,133 | `4fb0a681632d618a2a35f463127e3e39ab5eac683f88e7945f9b3ea82692d843` |

## 方法与结果

运行：`cargo build --release -p mm-cli`，再执行 `python research/s4/verify_phase1b_scale.py --count 1000`。脚本拒绝复用已存在的工作目录；将 Creator 设为 `Scale Verification Creator`；逐文件记录写入前 SHA-256；经 `plan-creator --files-from` → `apply` → 分批 `scan` 复读 → `fsck` → `plan-undo` → `apply` → `fsck`；最后再次逐文件计算 SHA-256，并检查临时/bak 残留。

| 检查 | 实测 |
|---|---:|
| Plan：Ready / Blocked / No change | 1,000 / 0 / 0 |
| Apply：Done / 其他状态 | 1,000 / 0 |
| 写入后文件 SHA-256 与写入前不同 | 1,000 / 1,000 |
| 通过新 `scan` 进程复读 Creator 为目标值 | 1,000 / 1,000 |
| Apply Operation 的 `fsck` 问题 | 0 |
| Undo Plan：Ready / Blocked | 1,000 / 0 |
| Undo：Done / 其他状态 | 1,000 / 0 |
| Undo 后 SHA-256 与各自写入前一致 | 1,000 / 1,000 |
| Undo Operation 的 `fsck` 问题；残留 temp/bak | 0；0 |

此次墙钟时间：Plan 35.91 秒，Apply 145.17 秒，分批复读 11.04 秒，Undo 38.50 秒。样本很小且被重复复制，**这些时间不能作为 S4 真实语料性能结论**。原始结果：`research/results/s4/phase1b-scale-20260927T042328Z.json`（本地、被排除提交）。

本轮另在可运行 Windows 替换操作的环境通过 `cargo test --workspace`（54/54）、`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`；新增单测覆盖 UTF-8 BOM、CRLF、中文路径和带空格路径的文件清单解析。

首次在受限执行环境中运行时，`ReplaceFileW` 对前四个文件返回 Windows 错误 5，安全熔断将其余 996 个标记为 cancelled；该次 Operation 的 `fsck` 为 0 问题。独立的最小替换探针在工作目录及系统临时目录均复现错误 5，在不限制该 API 的执行环境中成功；同一测试脚本随后得到上表结果。失败运行的原始结果为 `research/results/s4/phase1b-scale-20260927T042040Z.json`（本地、被排除提交），不能算产品通过记录。

## 结论边界

本次满足 **1,000 个 JPEG Creator 写入并逐字节 Undo** 的功能规模验证，但只覆盖 8 个重复的 ExifTool 测试夹具，总体量约 3 MB。它不证明 1,000 张不同的相机原片、10–40 MB 大文件、真实 NAS/云同步/exFAT、断电、第三方软件显示或 S4 性能目标。Phase 1b 的故障注入矩阵与其他缺口仍按 `PHASE1_REPORT.md` 追踪。
