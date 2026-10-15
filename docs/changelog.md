# 变更日志

本文件简要记录每次任务的修改内容。每条一行，不含代码细节。

## 2026-10-14
- 初始化 miko_next 项目骨架：独立 git 仓 + workspace（6 crate：umaai-core / umasim / umaai / umaai-cli / umaai-tools / umaai-tests）+ rust-toolchain / rustfmt / .gitignore / CI
- 建立 `docs/` 目录结构（plan / game / notes + changelog / issues），迁入游戏机制文档（规则 / 实现细节 / 术语 / 固有效果）
- 迁入通用数据 `gamedata/`（cardDB / umaDB / events / text_data_dict / constants / default_config，排除剧本数据）
- 新增 `AGENTS.md` 与重建方案 v2（`docs/plan/miko_next_plan.md`），记录本轮讨论定稿决策；v1 方案作参考随仓保留
