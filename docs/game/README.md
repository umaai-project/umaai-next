# 游戏机制文档目录（game/）

> 本目录集中存放 umaai-rs 的**游戏机制与术语**文档。2026-10-14 由 `.trae/documents/` 根目录迁入，并把原《游戏通用机制》按「游戏规则 / 实现细节」拆成两份。

## 文件清单

| 文件 | 内容 | 定位 |
|---|---|---|
| [game_rules.md](game_rules.md) | **游戏规则**：任何剧本通用的育成机制（回合 / 阶段 / 年份 / 开局 / 训练 / 比赛 / 事件 / 状态 / 继承 / 友人 / 评分 / 育成结束），以游戏本身的行为为准 | 单一事实源（规则） |
| [game_implementation.md](game_implementation.md) | **实现细节**：代码落点、数据约定、模拟简化与真机差异、已知 bug、未实现 / 外部下发、seam 与待上移清单、RNG 实现 | 单一事实源（实现口径） |
| [game_glossary.md](game_glossary.md) | 游戏通用术语表（中文 / 日文 / 程序名） | 术语 |
| [glossary.md](glossary.md) | 项目术语表（分层 / crate / seam / 搜索 / 通道 / 测试 / 代码实体） | 术语 |
| [support_card_unique_effect.md](support_card_unique_effect.md) | 支援卡固有效果（unique_effect）编号体系与映射（`game_rules.md` §10.3 的展开） | 专题展开 |

> 本批 P0.5 的缺口核对过程记录已归档：[archive/game_mechanics_gap_review.md](../archive/game_mechanics_gap_review.md)（55 项缺项 + 18 项文档↔代码不符 + 已拍板口径 + 落实核对）。

## 划分约定

1. **规则 vs 实现**：
   - 「游戏里怎么运作」→ `game_rules.md`（不写代码符号、文件名、行号）。
   - 「本仓怎么写、哪里不一样、哪里有 bug、哪些没做」→ `game_implementation.md`。
2. **通用 vs 剧本**：只有**任何剧本都适用**的机制进 `game_rules.md`；剧本专属机制（拉面做面 / 吃面 / 地区、温泉挖掘、无人岛建设等）留在各剧本资料或后续剧本仓文档。剧本共有的「外壳」（点数档位、进化技能、剧本链接等）按已拍板口径**由剧本自行处理**，通用层只留 seam。
3. **术语**：游戏原生词进 `game_glossary.md`，项目 / 架构词进 `glossary.md`。
4. **数据约定**：卡 ID 编码、事件 ID 分段、训练表列序等属实现侧约定，暂集中在 `game_implementation.md` §3，规模变大后可拆为独立文档。

## 来源与维护

- 规则来源：游戏实测 / 小黑板数据（`logs/`、`gamedata/*.json`）+ 上游资料（`mee-umasim/data/*_memo.md`）；实现口径以本仓代码为准，关键结论附 `crates/` 路径。
- 代码改动导致规则或实现口径变化时，**在提交前统一更新本目录文档**（与 `changelog.md` / `project_context.md` 同批）。
- 相关外部文档：[miko_next_rebuild_plan.md](../miko_next_rebuild_plan.md)（重建方案与 seam 设计）、[project_context.md](../project_context.md)（项目上下文）、[replay_review.md](../replay_review.md)（复盘口径）。