# 变更日志

本文件简要记录每次任务的修改内容。每条一行，不含代码细节。

## 2026-10-14
- 初始化 miko_next 项目骨架：独立 git 仓 + workspace（6 crate：umaai-core / umasim / umaai / umaai-cli / umaai-tools / umaai-tests）+ rust-toolchain / rustfmt / .gitignore / CI
- 建立 `docs/` 目录结构（plan / game / notes + changelog / issues），迁入游戏机制文档（规则 / 实现细节 / 术语 / 固有效果）
- 迁入通用数据 `gamedata/`（cardDB / umaDB / events / text_data_dict / constants / default_config，排除剧本数据）
- 新增 `AGENTS.md` 与重建方案 v2（`docs/plan/miko_next_plan.md`），记录本轮讨论定稿决策；v1 方案作参考随仓保留
- 工具链统一 stable：`rust-toolchain.toml` 由 nightly 改为钉死 stable（1.99.0），`rustfmt.toml` 精简为稳定可用选项（放弃 nightly 专属风格），CI 同步；解除「禁用 `cargo fmt`」限制

## 2026-10-15
- 细化 `AGENTS.md` 的 Git 与文档规则：不自动提交 / 推送（push 由用户手动执行），提交前统一更新三份文档，并明确 changelog / issues / project_context 各自口径
- 新增 `docs/project_context.md`：记录最新版代码结构
- `umaai-core` 收敛为「与游戏无关的通用机制层」：逐字移植 RNG 三流（含单元测试），新增统一错误类型，crate 文档与模块声明同步
- 核心层引入 `rand` 依赖（锚定 0.9 线，同步到 workspace）
- 工具链钉死版本上调，CI 与方案文档同步
