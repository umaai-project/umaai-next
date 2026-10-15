# 项目术语表（Project Glossary）

> 收录**项目/架构**术语：分层与 crate、契约与 seam、搜索框架、RNG 实现、输出通道、测试设施，
> 以及**仅存在于代码实现**的游戏侧标识符（trait / 结构体 / 字段名）。
> 游戏原生术语（玩法机制，中文+日文）见 [游戏通用术语表（game_glossary.md）](game_glossary.md)。
> 已按 miko\_next 重建方案刷新；重建期 P1–P7 内本表会随代码演进频繁变动。

***

## 分层与 crate

| 用语             | 中文         | 备注                                    |
| -------------- | ---------- | ------------------------------------- |
| `umaai-core`   | 核心层        | 契约 trait + 基础类型 + RNG，零重依赖            |
| `umasim`       | 计算层        | 规则基座 + 搜索框架，纯计算无终端依赖                  |
| `umaai`        | 引擎层        | Runner 主循环 + 协议解析 + 决策编排 + 在线记录       |
| `umaai-cli`    | print 前端   | 终端呈现 bin，所有终端依赖收敛于此                   |
| `umaai-tools`  | 测试工具       | 探针 / 基准 / golden runner，publish=false |
| `umaai_review` | 复盘工具       | 已移出 workspace members，代码留盘搁置          |
| 剧本（scenario）   | —          | 插在契约上的具体玩法实现                          |

***

## 契约与 seam

| 用语                    | 中文         | 备注                                    |
| --------------------- | ---------- | ------------------------------------- |
| seam                  | 接缝点        | 可替换 / 可插拔的接缝（trait 或泛型边界）              |
| `TrainingValueSource` | 训练基础值来源    | 替代对剧本数据表的硬依赖（trait）                   |
| `ScenarioRules`       | 剧本规则钩子     | 默认空实现                                 |
| `ScenarioLifecycle`   | 剧本生命周期钩子   | 默认空实现                                 |
| `FlatSearchGame`      | 扁平搜索局面抽象   | 原默认泛型落在剧本上，需去剧本化                      |
| `ActionIndex`         | 动作索引映射     | 剧本动作到全局索引，替代硬编码动作空间                   |
| `Frontend`            | 前端回调 trait | print / 手机 / DLL / Web 各自实现           |
| `Config`              | 运行配置       | serde 可构造，CLI 只是来源之一                  |
| `Summary`             | 运行结果       | 评分 + 记录汇总                             |

***

## 策略与搜索

| 用语               | 中文               | 备注                                |
| ---------------- | ---------------- | --------------------------------- |
| `Trainer<G>`     | 决策器 trait        | 未来策略替换点（手写/MCTS/NN/PBT/粒子滤波）      |
| `Evaluator<G>`   | 评估器 trait        | <br />                            |
| `RolloutBackend` | rollout 后端 trait | 原 `RamenBatchRollout`，待去 Ramen 命名 |
| `FlatSearch`     | 扁平搜索             | <br />                            |
| `Rollout`        | 模拟展开             | <br />                            |
| `MCTS`           | 蒙特卡洛树搜索          | <br />                            |
| `UCB` / `PUCT`   | 上置信界 / 预测引导搜索    | <br />                            |
| `cpuct`          | PUCT 系数          | <br />                            |
| `search_n`       | 搜索预算             | rollout 次数                        |
| `Objective`      | 目标函数             | 待建，PBT/粒子滤波复用                     |
| `ParamSpace`     | 参数空间             | 待建，可序列化，PBT 变异用                   |

***

## 随机数（RNG）

| 用语                  | 中文     | 备注                                     |
| ------------------- | ------ | -------------------------------------- |
| `SplitmixRng`       | 无状态随机流 | splitmix64(master + N×GAMMA)，Clone 值语义 |
| `TurnFixedRng`      | 回合固定流  | 人头分布 / Hint / 事件，与策略解耦                 |
| `StrategyRng`       | 策略流    | 训练成败 / 比赛 / 分身                         |
| `EventRng`          | 事件流    | 回合开始事件链                                |
| `fork_local_stream` | 局部流派生  | 父流只消耗 1 次，隔离子算法                        |
| `CRN`               | 共同随机数  | Common Random Numbers，rollout 对比用      |

***

## 输出与通道

| 用语                  | 中文         | 备注                       |
| ------------------- | ---------- | ------------------------ |
| `DecisionSink`      | 决策输出 trait | umasim 留 trait，实现上移      |
| `HumanReadableSink` | 人读输出       | 原 umasim，重建后上移 umaai-cli |
| `StdoutJsonSink`    | JSON 行输出   | 原 umasim，重建后上移 umaai-cli |
| `urafile`           | 游戏↔Rust 通道 | C# 插件数据格式                |
| `EngineEvent`       | 引擎事件       | Frontend 回调载荷            |
| `EngineError`       | 引擎错误       | FFI 边界 JSON 返回           |

***

## 测试与基准

| 用语          | 中文       | 备注                               |
| ----------- | -------- | -------------------------------- |
| `golden` 基线 | 固定种子期望结果 | 回归 + 调参对比                        |
| `testkit`   | 内部测试访问   | `#[doc(hidden)] pub mod testing` |
| `probe`     | 探针       | luck\_probe / turn\_inspect 等    |
| `bench`     | 基准       | criterion / 配对策略                 |

***

## 游戏侧代码实体（现实现命名）

> 这些是**代码里才有的标识符**（含拼音字段名）。其中多数概念本身属游戏，见 [game_glossary.md](game_glossary.md)；少数仅为程序侧产物（如 `distribution`、`FriendOutState`、`EventChoice`、`TriggerType`），无对应游戏概念。重建后多数会改名或抽象化，故集中列于此。

### 状态与流程类型

| 用语           | 中文         | 备注                          |
| ------------ | ---------- | --------------------------- |
| `Game`       | 局面状态 trait | 剧本游戏类型实现的核心接口               |
| `BaseGame`   | 基础游戏状态     | 通用状态 + 逻辑，不含人头              |
| `BasicGame`  | 无剧本基座局     | 实现 `Game`，用于引擎冒烟            |
| `Person`     | 训练人物 trait | <br />                      |
| `BasePerson` | 基础人头       | <br />                      |
| `PersonType` | 人头类型枚举     | Card / ScenarioCard / Npc … |
| `ActionEnum` | 动作 trait   | `apply(game, rng)` 修改状态     |
| `BaseAction` | 基础动作枚举     | Train / Race / Sleep …      |
| `TurnStage`  | 回合阶段枚举     | Begin / Distribute / … / End |
| `GameView`   | 展示模型       | 面向下游消费者的结构化字段               |

### 属性字段

| 用语                    | 中文         | 备注                                    |
| --------------------- | ---------- | ------------------------------------- |
| `Uma`                 | 马娘状态       | 五维 / 体力 / 干劲 / PT / flags             |
| `UmaFlags`            | 状态标识       | 切者 / 小切 / 爱娇 / 擅长训练 等                  |
| `five_status`         | 五维属性数组     | <br />                                |
| `five_status_limit`   | 五维上限数组     | <br />                                |
| `vital` / `max_vital` | 体力 / 体力上限   | <br />                                |
| `motivation`          | 干劲         | <br />                                |
| `skill_pt`            | 技能点        | <br />                                |
| `total_hints`         | Hint 打折总级数  | <br />                                |
| `race_bonus`          | 比赛加成       | <br />                                |
| `ActionValue`         | 动作效果值      | status\_pt / vital / motivation / hint\_level / friendship / flags |

### 训练字段

| 用语            | 中文    | 备注                        |
| ------------- | ----- | ------------------------- |
| `train_level` | 训练等级  | floor(次数/4)+1            |
| `distribution` | 人头分布  | \[训练位]\[人头下标]             |
| `deyilv`      | 得意率   | 拼音字段                      |
| `youqing`     | 友情加成  | 拼音字段，仅闪彩生效                |
| `shining`     | 友情训练  | void 判定（彩圈）                |
| `failure_rate` | 训练失败率 | <br />                    |

### 人物与支援卡

| 用语                   | 中文      | 备注                                |
| -------------------- | ------- | --------------------------------- |
| `Card`               | 支援卡     | <br />                            |
| `ScenarioCard`       | 剧本友人    | <br />                            |
| `TeamCard`           | 团队卡     | <br />                            |
| `Npc`                | NPC     | <br />                            |
| `Yayoi`              | 理事长     | <br />                            |
| `Reporter`           | 记者      | <br />                            |
| `FriendState`        | 友人状态    | out\_state / out\_used / event\_bonus / vital\_bonus |
| `FriendOutState`     | 友人出行阶段  | UnClicked → … → Away              |
| `InheritInfo`        | 继承信息    | blue\_count + extra\_count        |
| `CardTrainingEffect` | 支援卡训练效果 | youqing / ganjing / xunlian / deyilv / bonus / fail\_rate\_drop 等 |

### 事件 / 比赛 / 评分字段

| 用语                     | 中文      | 备注                              |
| ---------------------- | ------- | ------------------------------- |
| `EventData`            | 事件数据    | id / trigger / choices / player\_select |
| `EventChoice`          | 事件选项    | prob / value                    |
| `TriggerType`          | 触发类型枚举  | Random / Code / Fixed           |
| `unresolved_events`    | 待处理事件   | 回合内排队执行                         |
| `career_races`         | 生涯比赛 bitset | turn 11–71                      |
| `win_races`            | 自选比赛 bitset | <br />                          |
| `is_race_turn`         | 生涯比赛回合判定 | turn 73/75/77 + bitset          |
| `free_race`            | 自选比赛区间  | <br />                          |
| `calc_score`           | 终局评分    | <br />                          |
| `ScoreParts`           | 评分分量分解  | skill / pt / five\_status\[5]    |
| `total_pt`             | 总 PT    | <br />                          |
| `pt_score_rate_factor` | PT 折算系数 | 切者 ×1.1 / 小切 ×1.04            |
| `qiezhe` / `xiaoqie`   | 切者 / 小切  | 拼音字段                            |

***

## 缩写

| 用语     | 全称                         | 中文      |
| ------ | -------------------------- | ------- |
| `RNG`  | Random Number Generator    | 随机数生成器  |
| `CRN`  | Common Random Numbers      | 共同随机数   |
| `MCTS` | Monte Carlo Tree Search    | 蒙特卡洛树搜索 |
| `UCB`  | Upper Confidence Bound     | 上置信界    |
| `PUCT` | Predictor + UCT            | 预测引导搜索  |
| `NN`   | Neural Network             | 神经网络    |
| `FFI`  | Foreign Function Interface | 外部函数接口  |
| `PBT`  | Population Based Training  | 基于种群的训练 |
