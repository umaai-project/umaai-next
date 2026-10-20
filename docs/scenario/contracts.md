# 契约参考（剧本侧）

**稳定面**＝`umaai-core` / `umasim` / `umaai` 的 pub 面（对外承诺，`non_exhaustive` + 带默认实现的方式演进）；
`umaai-cli` / `umaai-tools` / `umaai-tests` 属**主线样例面**，pub 面不承诺稳定，剧本仓按 git `rev` 钉住或自实现。

## 1. `Game`（`umasim::game::Game`）

必备 **22 个方法 + 2 个关联类型**（缺一个就编不过）：

| 组 | 必备项 |
|---|---|
| 关联类型 | `type Person: Person` · `type Action: ActionEnum<Game = Self>` |
| 回合 | `turn` · `max_turn` |
| 规则流 | `rule_master` · `set_rule_master`（注入点，派生各轴） |
| 阶段 | `stage`（投影 `StageId`）· `next` · `run_stage`（三条轴分工都在这里） |
| 决策面 | `list_actions` · `generate_events` · `apply_event` |
| 人头 | `persons` · `persons_mut` · `init_persons` · `deyilv` |
| 局面投影 | `distribution` · `distribution_mut` · `absent_rate_drop` · `train_level` · `has_group_buff` · `uma` · `uma_mut` · `deck` |

**默认提供**（不必实现，按需覆盖）：`run_full_game` / `run_event` / `list_turn_events` / `apply_action` /
`on_simulation_end` / `person_is_available` / 人头分配与闪彩一族（`reset_distribution` / `distribute_all` /
`distribute_hint` / `at_trains` / `is_shining_at` / `shining_count` / `deck_index_of` / `count_training_persons`）/
训练一族（`calc_training_buff` / `calc_training_value` / `calc_training_failure_rate` 与各自 `default_*`）。

**人头布局契约（剧本必读）**：`persons` 的布局遵守两条硬纪律——

- **卡片前缀**：`persons[0..卡组长度]` 与 `deck` 逐槽同序（含友人卡）。晚加入的卡组人物
  （友人 `turn ≥ 2`、记者 `turn ≥ 13` 可用）一律「**开局建人头 + `person_is_available` 回合激活**」，
  不中途插队——训练加成 / 得意率等**热路径**据此按下标 O(1) 直取卡组；
- **尾部追加**：非卡人头（理事长 / 记者 / 剧本人物 / NPC）只在尾部 `add_person` 追加（引擎
  `init_persons` 同序）。因此**非卡人头下标随卡数与追加顺序浮动**（5 卡局理事长在 5、6 卡局在 6）：
  定位用 `person_index_of_type(ty)`，跨容器引用按 `card_id` 走 `person_index_of_card` /
  `deck_index_of(person_index)`（冷路径通用反查），**不要写死下标；数据表（events.json 等）
  不得携带写死的 `person_index`**——事件归属由生成点动态定位后写入。

`TrainingValueSource` 只一项：`training_basic_table()`（训练基础值表，数值是剧本资产）。

## 2. `Person` / `ActionEnum`

- `Person`（`Debug + Clone + PartialEq + Default`）：必备 `person_type` / `person_index` / `train_type` /
  `friendship` / `hint` / `set_hint` / `card_id`；默认 `is_friend` / `is_scenario_card`。
  模板直接复用引擎参考实现 `BasePerson`（真实剧本可自持人头类型）。
- `ActionEnum`（`Debug + Display + Clone + PartialEq`）：必备 `type Game` / `apply`（只把动作落到局面，
  **不取随机流**）；默认 `as_base_action`（转引擎基础动作类型，供通用逻辑识别）。

## 3. `Trainer` / `FlatSearchGame`

- `Trainer<G>`：必备 `select_action` / `select_choice`；默认 `select_event_choice`（带完整事件数据的新接口，
  回退到 `select_choice`）。决策流是 `run_stage` 的参数、**只喂 Trainer**。
- `FlatSearchGame`（`Game + Clone + Send + Sync`）：必备 `type RolloutTrainer` / `SUPPORTS_TRUNCATED_LEAF` /
  `default_rollout_trainer` / `apply_root_action`（**必须与 `run_stage` 内执行动作逐字一致**）/
  `fork_for_rollout`（**分支唯一入口**：克隆 + 注入 rollout 种子 + 重置自持流）；默认 `crn_stage_key`
  （阶段键，稳定契约）/ `search_score`（单轴排序口径）。

## 4. `Frontend`

`on_run_start` / `on_game_start` / `on_stage_begin` / `on_stage_end` / `on_uma_status` / `on_decision` /
`on_error` / `on_run_end`——**全部带默认空实现**，宿主按需覆盖。引擎只下发剧本无关字段；剧本专属资源由
剧本自持 View 提供，**契约 trait 永不加呈现方法**。

## 5. 装配入口

| 入口 | 用途 |
|---|---|
| `umaai::runner::Runner` | 离线一局：`new(&Config, &mut dyn Frontend)` + `run_game(game, &trainer, None)`；自动注入 `cfg.sim.seed` 为主种子 |
| `umaai::session::Session` | 帧驱动：`step(&RawFrame)` 一帧一决策（宿主喂帧，Web / Android 走这条）；文件通道形态另有 `run` |
| `umaai_core::Data` | 静态表载入：`init(数据根)` / `init_from(&DataSource)`（目录 / 内存两源，幂等按来源标识） |
| `umaai::Config` | 配置：`load_with_overlay_file(数据根, 宿主目录)`（两份同名 `ai_config.toml`）或 `parse(默认文本, 覆盖文本)`（非路径宿主） |

## 6. `StageId` 与规则流

- `StageId`：剧本**显式赋号的数字**，引擎只透传、不做分支；**号是稳定契约**（改号＝旧种子与基线失效）。
- `BaseGame` 规则流：`take_rule_streams()` / `put_rule_streams()` **成对**使用（中途出错也先放回）；
  `reset_rule_streams()` 在回合边界调用；`init_rule_streams(master)` 是底座入口（**刻意不与契约
  `set_rule_master` 同名**，避免 `Deref` 静默解析）。

## 7. View DTO（形态 A，值快照定稿）

```rust
/// 剧本资源快照：单一取数口 + 只放引擎不知道的字段 + 快照语义 + 可演进 + 不进契约 trait
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ScenarioView {
    /// 剧本自持资源（PT / 地区 / 挖掘…）
    pub scenario_pt: i32,
}

impl ScenarioGame {
    /// 何时取由宿主决定（终端样例与局面摘要同点取，Web / Android 在渲染循环取）
    #[must_use]
    pub fn view(&self) -> ScenarioView { /* 值快照 */ }
}
```

定稿：**值快照**（owned、`Clone`、无借用生命周期，跨线程 / 跨宿主传递安全）；**序列化由剧本提供**
（`serde` derive），引擎不背书传输格式；将来若需框架级通用取数（如搜索记录导出），另开显式 trait
（`TerminalRecord` 同法），不塞进 `Game`。
