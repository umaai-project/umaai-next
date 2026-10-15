# 待决事项（Open Items）

> 本文件跟踪方案 v2 尚未拍板的设计点。拍板后移入
> [`docs/plan/miko_next_plan.md`](../plan/miko_next_plan.md) 对应章节并从本表移除。

| # | 事项 | 现状 / 倾向 | 备注 |
|---|---|---|---|
| 1 | **收口方式** | 待定 | 新仓并入 `umaai-rs/miko_next`：整体替换该分支内容，还是 `merge --allow-unrelated-histories` |
| 2 | **baseline 数据文件位置** | 待定 | 候选：`crates/umaai-tests/baselines/` · `crates/umaai-tests/tests/baseline/` · 仓库根 `baselines/` |
| 3 | **更新器交互细节** | 待定 | 启动自动检查+提示 / 仅 `umaai update` 子命令 / 两者；`ura_data`（小黑板数据）是否纳入 |
| 4 | **算法探索 workbench 归属** | 倾向独立项目 | 独立仓 / 仓内单列 / 暂不进仓；已留 seam |
| 5 | **测试 crate 命名** | 暂 `umaai-tests` | 备选 `umaai-it` / `umaai-e2e` |
| 6 | **发布 exe 名** | 暂 `umaai-cli` | 最终 `umaai.exe` 产物名在打包阶段定；注意与 lib crate `umaai` 的同名产物冲突 |
| 7 | **gamedata 数据版本策略** | 待定 | `manifest.json` 的 `schema_version` 语义、数据来源盖章方式 |
| 8 | **`umaai-rs-ref` 是否随进度更新** | 待定 | 现为 master 只读工作树，建议冻结不动 |

## 迁入本仓时已知的文档悬空引用

`docs/game/` 由旧仓原样迁入，其中指向旧仓 `archive/`、`replay_review.md` 等的相对链接在本仓不存在，属已知悬空（仅文本层面，不影响构建）。整理时机待定。
