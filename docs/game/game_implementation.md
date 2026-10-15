# 游戏机制实现细节（Implementation Notes）

> 本文档记录 umaai-rs **当前代码实现**层面的口径：代码落点、数据约定、模拟简化与真机差异、已知 bug、未实现项与重构 seam。
> 游戏本身的机制（该怎么运作）见 [游戏通用规则（game_rules.md）](game_rules.md)；本目录的划分约定见 [README.md](README.md)。
> 结论来源：2026-10-14 P0.5 缺口核对（过程记录见 [归档核对表](../archive/game_mechanics_gap_review.md)），关键结论均已二次核对代码。

---

## 目录

1. [定位与划分](#1-定位与划分)
2. [代码地图](#2-代码地图)
3. [数据约定](#3-数据约定)
4. [模拟简化与真机差异](#4-模拟简化与真机差异)
5. [已知 bug / 待修正](#5-已知-bug--待修正)
6. [未实现 / 外部下发](#6-未实现--外部下发)
7. [剧本 seam：事件率与连续事件率](#7-剧本-seam事件率与连续事件率)
8. [生命周期与元数据 seam](#8-生命周期与元数据-seam)
9. [附录 B 对照与待上移清单](#9-附录-b-对照与待上移清单)
10. [随机数（RNG 三流）实现](#10-随机数rng-三流实现)

---

## 1. 定位与划分

| 内容 | 归属 |
|---|---|
| 游戏怎么运作（触发条件、公式、数值、上限、互斥） | [game_rules.md](game_rules.md) |
| 术语（中文 / 日文 / 程序名） | [game_glossary.md](game_glossary.md)（游戏）/ [glossary.md](glossary.md)（项目） |
| 本仓怎么实现、与真机差异、bug、seam、数据约定 | **本文** |
| 剧本专属机制（拉面、温泉、无人岛…） | 各剧本资料 / 后续剧本仓文档 |

写作纪律：规则文档不出现代码符号与行号；本文出现代码符号、文件名与常量键。

---

## 2. 代码地图

| 规则章节 | 主要代码位置（`crates/umasim/src/`） |
|---|---|
| 回合与流程 | `game/mod.rs`（`TurnStage`、`Game`/`BaseAction` 枚举）、`game/traits.rs`（`Game::run_stage`/`next`） |
| 育成阶段 / 年份 | `game/base/mod.rs`（`is_xiahesu`、`can_self_race`）、`game/base/basic.rs`（`BasicGame`） |
| 开局与卡组 | `game/base/mod.rs::BaseGame::new`、`game/support_card.rs`、`gamedata/support_card.rs` |
| 训练 | `game/traits.rs`（`calc_training_buff`/`calc_training_value`/`calc_training_failure_rate`/`distribute_*`）、`game/base/basic.rs::do_train` |
| 比赛 | `game/base/action.rs::do_race`、`game/uma.rs`（比赛表 bitset）、`gamedata/uma.rs`（`FreeRaceData`） |
| 休息与外出 | `game/base/action.rs` |
| 事件 | `game/base/mod.rs::apply_event`、`gamedata/event.rs`、`gamedata/events.json` |
| 状态与属性 | `game/uma.rs`（`Uma`/`UmaFlags`/`add_value`/`add_motivation`） |
| 继承 / 支援卡 | `game/inherit.rs`、`game/support_card.rs` |
| 友人 | `game/mod.rs::FriendState`、`game/base/basic.rs` |
| 评分 | `game/uma.rs::calc_score`/`score_parts`、`gamedata/config.rs`（`GameConstants`） |
| 育成结束 | `game/traits.rs::on_simulation_end`、`game/base/basic.rs`、`game/ramen/game.rs::add_mandatory_events` |
| 在线记录 / 协议 | `crates/umaai/src/`（`protocol/`、`decision/record`、`plot/`） |

数据文件（工作区 `gamedata/`）：`constants.json`（通用常量）、`default_config.toml`（默认配置与可覆盖项）、`events.json`（事件表）、`cardDB.json`、`umaDB.json`、`scenario_onsen.json`、`scenario_ramen.json`、`text_data_dict.json`。

---

## 3. 数据约定

> 本节属**实现侧数据约定**（不是游戏机制）。内容会继续增长，达到一定规模后可拆为独立文档。

1. **卡 ID / 突破等级编码**：`idrank = 卡 ID × 10 + 突破等级`，突破等级 0–4（>4 报错）。突破等级决定取 `card_value[突破等级]` 的哪一档面板，以及初始羁绊。
2. **卡组长度**：配置 `cards: [u32; 6]`，启动校验长度 = 6。**新剧本可能需要不同张数，重构时应改为剧本可配置。**
3. **卡类型码**：0 速 / 1 耐 / 2 力 / 3 根 / 4 智 / 5 友人 / 6 团队；开局统计 `card_type_count[7]`。
4. **训练基础值表**：`[训练位][等级] → [速, 耐, 力, 根, 智, PT, 体力]` 共 7 列；体力列通常为负（消耗），智训练为正（回体）；副属性即「非本训练位且 > 0」的属性列。
5. **五维属性游戏侧 id**（用于协议 / 资料对照，**与卡类型码序号不同**）：速 101 / 力 102 / 根 103 / 耐 105 / 智 106。
6. **事件 ID 分段**：8001–8003 支援卡连续事件；4012 / 4013 局中继承；5007 马娘通用「切者」事件；`80905xxxx` 友人事件；系统事件键见 `events.json`。
7. **种子编码**：`splitmix64(master + N × GAMMA)`，`GAMMA = 0x9E37_79B9_7F4A_7C15`（冻结不可改）；三流 tag 见 [§10](#10-随机数rng-三流实现)。

---

## 4. 模拟简化与真机差异

以下内容**不进规则文档**（属模拟口径或数值近似）：

| # | 项 | 本仓做法 |
|---|---|---|
| 1 | 比赛奖励数值 | 真机为「随机某个属性 +15」等，模拟把奖励**平均化**为固定数值表（`race_career` / `race_g1-g4` 等） |
| 2 | 自选比赛结果分支 | `player_select = true`：第 0 组固定体力 −15，第 1 组 33% 大成功（体力 −5）/ 66% 成功（体力 −20）；属模拟数据 |
| 3 | 训练失败判定 | 同一失败率**连掷两次**，第二次仍失败即大失败（概率 ≈ 失败率²）；失败 / 大失败的效果由事件表结算 |
| 4 | 随机事件分布 | 四分类权重：支援卡 0.35 / 马娘 0.25 / 掉心情 0.10 / 无事 0.30；另有「无事件回合」列表（含夏合宿） |
| 5 | 系统事件数值 | `training_fail` / `training_fail_low` / `normal_outing_1-3` / `normal_outing_doll` / `ticket` / `ending` / 点击类事件等固定数值均在 `events.json` |
| 6 | 事件判定细节 | 事件按 id 计数，随机事件有最大触发次数（0 = 无限）；**单选项组忽略概率**；多选项组按概率加权，全部为 0 时事件无效；选项的「成功 / 大成功 / 失败 / 大失败」结果标签通用层只用于展示 |
| 7 | 生病 | 只做简化模拟（标记 + 解锁治病），不完善各状态异常 |
| 8 | 幸运体质 | 仅计数与特征位，**尚无免疫接线** |
| 9 | 比赛胜负 | 默认全胜，无胜负 roll |
| 10 | 评分 / 运气分等 AI 口径 | `pt_score_rate` / `hint_pt_rate` 折算、`pt_favor_rate`（= 1.0 等价于直接用 `calc_score`）、运气分 baseline、显示值反向换算等属 **AI 口径**，见 `project_context.md` / `replay_review.md`，不属游戏规则 |

---

## 5. 已知 bug / 待修正

| # | 项 | 现状 | 处理 |
|---|---|---|---|
| 1 | 夏合宿训练等级计数 | 三条训练成功路径都无条件 `train_level_count += 1`，合宿只把**显示等级**钉 5 → 合宿后会跳级 | **疑为程序 bug**，重构时改为「合宿不计入基础等级」 |
| 2 | 出场率提升未实现 | `absent_rate_drop` 是**死字段**（无写入、恒 0），不在权重只能靠类型表 | 待补数据源（规则见 [game_rules.md §5.2](game_rules.md#52-人头分配)） |
| 3 | 团队卡 Buff | `group_buff_turn` 全仓无写入 → `has_group_buff()` 恒 false → 团队卡 / 拉面友人卡永不闪彩 | **由使用团队卡的剧本自行维护**，通用层不得假设其自动生效 |
| 4 | 点击友人概率 | 配置里的 `friend_click`(0.4) 零引用，实际每次点击都触发点击友人事件 | 规则按「无概率门槛」描述，配置项应删除或接线 |
| 5 | 事件状态增减未应用 | `add_flags` / `remove_flags` 只被打印，`apply_event_choice` 无调用者 → `good_trainer` / `aijiao` 实际不可获得 | 状态由 URA 下发，需接线（见 [§6](#6-未实现--外部下发)） |
| 6 | 自选比赛区间下界 | 通用规则从 turn 13 起，`BasicGame` 用 `turn > 13`（14 起，历史行为） | 重构统一 |
| 7 | `pt_score` 取整 | 规则写 `floor`，实现为 `as i32` 截断（正数等价，口径不精确） | 加注 / 统一 |
| 8 | turn ≥ 72 比赛等级 | 取 grade = 4（`race_g4`），与「URA 固定 G1」注释矛盾 | **保持原状**（已拍板：turn ≥ 72 时 grade 不参与判定） |
| 9 | 阶段枚举 | `StoryShop` 定义后无人使用但被 `Sequence` 推进；`End` 定义后无人处理 | P1 定稿时清理或指定归属 |
| 10 | 继承上限注释 | 代码注释写「3 星蓝因子提供 16 上限」，实现 `as i32` 截断实为 15 | 注释与规则文档按 15（估算）表述 |
| 11 | 结束事件触发时点 | 基础实现在 turn 77 **Begin** 阶段就 push 结束事件；规范逻辑（规则 §13）要求统一在 **turn 77 结束后（`AfterTrain` 或更晚）** 判定 | **重构时必须改过来**：结束事件统一到 turn 77 结束后判定 |
| 12 | 训练后羁绊分情况 + 爱娇范围 | 训练后羁绊：base / 拉面对**所有人头**统一 +7、温泉对理事长 / 记者用 +2；规则要求**支援卡 +7、理事长 / 记者 +5**。爱娇目前只在训练后一处做 +2 | 重构统一：训练后按人头类型给羁绊（支援卡 +7 / 理事长·记者 +5）；爱娇**对所有羁绊获取 +2** |

---

## 6. 未实现 / 外部下发

| 项 | 现状 | 说明 |
|---|---|---|
| 切者 | 局内由 **URA / 游戏下发**；模拟路径存在事件 5007 + 0.05 判定 | 部分马娘另有获得路径，模拟不考虑（已拍板） |
| 爱娇 / 擅长训练 | 由 **URA 下发**，模拟端当前无接线 | **需处理**：接入后按**全局 +2** 作用于**所有**羁绊获取（现仅训练后支援卡 +7→+9 一处），失败率 ∓2 |
| 剧本链接 | 仅温泉剧本实现；拉面仅有 `link_chara_id` 数据；`is_link_card` 恒 false | 剧本机制，通用层只留 seam |
| 团队卡 Buff | 见 [§5](#5-已知-bug--待修正) 第 3 条 | 由剧本维护 |
| 剧本内置资源 / 档位加成 | 各剧本自行处理（ラーメン Pt、パフォーマンス、メカEN、サポーターPt、適性Pt、発展Pt、期待度…） | 不进通用规则；通用层只提供 seam 点 |
| 剧本进化技能 / 剧本链接 / 训练等级批量提升 / 赛后能力提升 / 编成外人数 / 多人友情 / 粉丝数 / 非比赛型目标 | 由剧本自行处理或复用现有比赛模拟 | 已拍板：不追加通用机制条目 |

---

## 7. 剧本 seam：事件率与连续事件率

- **需要通用 seam（已拍板）**：支援卡事件率、支援卡连续事件率必须**可被剧本改写**。
- 当前 `event_probs`（`constants.json`）为全局固定值，剧本只能自行重写 `generate_events` 逻辑。
- 其余事件相关配置（无事件回合、剧本事件表等）由剧本自行处理。

---

## 8. 生命周期与元数据 seam

| 埋点 | 现状 |
|---|---|
| 局开始前 | 未实现（各剧本 inherent `newgame`） |
| 初始状态 | 已有（`BaseGame::new` + `Game::init_persons`） |
| 动作前 | 未实现 |
| 预测前 | 未实现（trainer 侧直调计算） |
| 动作后 | 未实现（三剧本各自 open-code） |
| 回合末 | 未实现（`TurnStage::End` 无人处理；RMJ / buff 倒计时都在剧本 `next()` 里做） |
| 局结束后 | 已有（`Game::on_simulation_end`，默认空实现；当前仅温泉 override 且为空） |

`ScenarioMeta` 覆盖现状：仅「回合数」部分实现（且硬编码 77）；剧本 id 有（12/14 常量）；显示名 / 等级提升回合 / 训练自动升级 / 是否有嘉宾 / 第二训练状态 / 联动角色均未抽象为元数据。

---

## 9. 附录 B 对照与待上移清单

来自 `miko_next_rebuild_plan.md` 附录 B 的机制覆盖对照（详见 [归档核对表](../archive/game_mechanics_gap_review.md) §1.8）：

- **两者都无（需先立规格）**：出场率提升（= 上面第 2 条）、休息预测（休息结果在 apply 当场随机）、洗牌修正（无规格）。
- **需上移为通用/ seam 的实现**：
  - 训练值上下层（拉面 3 份等价实现 + 温泉 1 份）→ `TrainingValueSource`；
  - Hint 总量上限、强制启发数（拉面 / 温泉各一份，通用 base 无）；
  - 夏合宿休息自动治病（仅拉面分支）；
  - 理事长 / 记者训练后羁绊 +2（仅温泉实现，拉面未区分）；
  - 剧本事件源、回合末结算、动作后事件 → `ScenarioLifecycle`。

---

## 10. 随机数（RNG 三流）实现

> RNG 为项目机制（可复现性设计），不属游戏规则；术语见 [glossary.md](glossary.md)。

- **底层**：`SplitmixRng`，第 N 次随机 = `splitmix64(master + N × GAMMA)`；值语义，Clone 即独立实例（MCTS 克隆局面隔离的基础）。
- **三条流**：

| 流 | 随机点 | master 派生 |
|---|---|---|
| 回合固定流 | 人头分布、训练角标、Hint、回合事件 | `(rule_master, turn)` |
| 策略流 | 训练成败 / 大失败、分身、比赛结果、吃面落地、休息 / 外出结果 | `(rule_master, turn, STRATEGY_TAG)` |
| 事件流 | 回合开始事件链（友人解锁判定、事件生成、事件应用） | `(rule_master, turn, EVENT_TAG)` |

- 另有决策种子 / 探针 / 分身派生 tag（bench 与剧本分身用）；`fork_local_stream(parent, tag)` 从父流取 1 个字派生局部流，父流只消耗 1 次。
- **适用范围**：三流目前仅在**拉面剧本**接线；通用 `Game` / `Trainer` / `run_event` 签名仍走 `&mut StdRng`。重建时应把三流下沉到核心层并统一。