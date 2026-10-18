# 测试一览

本文件对照源码列出全部测试：按 crate 分节、按文件分小节、一行一条，便于定位用例与评估覆盖面。
测试分层与判定口径的设计依据见 [`plan/miko_next_plan.md`](plan/miko_next_plan.md) 的「测试策略」节。

口径（2026-10-18 实测，stable 工具链 Release 模式）：`cargo test --workspace --release` → **256 passed / 0 failed / 3 ignored**；
`cargo test --workspace --release -- --list` 共列出 **259 条**（258 个 `#[test]` + 1 个 doctest）。

| 层 | 位置 | 判定 | 条数 |
|---|---|---|---|
| L1 单元 | `crates/<crate>/src/`（内联 `#[cfg(test)]`） | `assert` | 214 |
| L2 模块集成 | `crates/<crate>/tests/` | print + `Checker` / `Checks`（失败返回 `Err`） | 26 |
| L3 跨 crate 端到端 | `crates/umaai-tests/tests/` | print + `Checker`（失败返回 `Err`） | 17 |
| L4 baseline | `crates/umaai-tests/tests/baseline.rs` + `crates/umaai-tests/baselines/*.json` | 比对后返回 `Err` / 非零退出码 | 1 |
| 文档示例 | doctest | 标记 `ignore`，不执行 | 1 |
| **合计** | | | **259** |

- 分 crate：`umaai-core` 59 · `umasim` 53 · `umaai` 69 · `umaai-cli` 31 · `umaai-tools` 29 · `umaai-tests` 18。
- 默认跳过 3 条（需手动跑 `--ignored`）：`channel_watches_real_writes`（真 watcher 写入链路）、`session_runs_on_real_channel`（真通道收局）、`define_terminal_record` 的 doctest（标记 `ignore`）。

## 运行方式

```bash
cargo test --workspace --release                                          # 全量（Release 是项目约定）
cargo test -p umaai-core --release                                        # 单 crate
cargo test --workspace --release -- --list                                # 只列测试名与条数
cargo test --workspace --release -- --ignored --nocapture                 # 跑默认跳过项
cargo test -p umaai-tests --release --test session_basic -- --nocapture    # 单个测试文件
```

- 所有 test / binary 一律 Release 编译（工具链见 `rust-toolchain.toml`，CI 口径见 `.github/workflows/ci.yml`）。
- 引擎的文件通道是可选特性 `file-channel`（默认开）：`cargo check -p umaai --no-default-features` 验证「无文件监听依赖」的构建，此时帧形态测试与 `tests/channel.rs` 不编不跑（`umaai-tests` 以同名特性透传）。
- 搜索的并行后端是可选特性 `parallel`（默认开，实现在 `umasim`、由 `umaai` 透传）：`cargo test -p umasim -p umaai --release --no-default-features` 跑**串行形态**（此时 rayon 不在依赖图中，`with_parallel(true)` 被降级为串行）；主线样例面（`umaai-cli` / `umaai-tools` / `umaai-tests`）始终带默认特性，不承诺该形态。
- 配置两份、同名不同位：默认层 `<数据根>/ai_config.toml`、覆盖层 `<宿主目录>/ai_config.toml`（存在才合并，无命令行开关）；**宿主目录可由入口注入**（默认当前目录），CLI / 工具用例注入临时目录，另保留「运行目录（workspace 根）不得存在 ai_config.toml」的守门断言。
- 静态数据来源两条：目录（`DirSource`，`Data::load(root)` 是其便捷入口）与内存（`MemorySource`，收 JSON 值或 JSON 文本）；`Data::from_source(&src)` 对任意来源通用，L2 用真实数据比对两条来源的载入结果。
- 集成测试与端到端测试不用 `assert`：经 `Checker`（`umaai-tests`）或同语义的 `Checks`（`crates/umaai/tests/common/mod.rs`）逐条打印 `ok` / `FAIL`，结束时聚合失败项返回 `Err`；加 `--nocapture` 才能看到逐条输出。
- 测试工作目录为 workspace 根；需要定位路径时用测试支撑提供的公共函数。
- L4 baseline 已落地：基线数据在 `crates/umaai-tests/baselines/*.json`（一基线一 JSON），判定在 `crates/umaai-tests/tests/baseline.rs`（含「改动一项期望即变红」的反自证守门）；重录：`cargo run --release -p umaai-tools --bin umaai-tools -- baseline --update`，单独校验：同名命令去掉 `--update`（可用 `--only <基线名>` 收窄）。
- 空剧本仓模板是**独立包**（不在 workspace 成员表内）：`cargo check --manifest-path templates/umaai-scenario/Cargo.toml --all-targets`（CI 单独一步，守验收条款「不改引擎一行即可编过」）；模板自身不设测试，接入纪律见 `docs/scenario/`。

## 目录
- [umaai-core (59)](#umaai-core-59)
  - [`src/array.rs` (2)](#srcarrayrs-2)
  - [`src/crn.rs` (5)](#srccrnrs-5)
  - [`src/data/manifest.rs` (1)](#srcdatamanifestrs-1)
  - [`src/data/source.rs` (2)](#srcdatasourcers-2)
  - [`src/diag.rs` (4)](#srcdiagrs-4)
  - [`src/error.rs` (2)](#srcerrorrs-2)
  - [`src/flags.rs` (4)](#srcflagsrs-4)
  - [`src/gamedata/card.rs` (2)](#srcgamedatacardrs-2)
  - [`src/gamedata/constants.rs` (3)](#srcgamedataconstantsrs-3)
  - [`src/gamedata/event.rs` (4)](#srcgamedataeventrs-4)
  - [`src/gamedata/uma.rs` (3)](#srcgamedataumars-3)
  - [`src/race_mask.rs` (4)](#srcrace_maskrs-4)
  - [`src/rng.rs` (10)](#srcrngrs-10)
  - [`src/stats.rs` (7)](#srcstatsrs-7)
  - [`tests/data_load.rs` (6)](#testsdata_loadrs-6)
- [umasim (53)](#umasim-53)
  - [`src/game/base/basic.rs` (5)](#srcgamebasebasicrs-5)
  - [`src/game/base/stage.rs` (2)](#srcgamebasestagers-2)
  - [`src/game/base/mod.rs` (11)](#srcgamebasemodrs-11)
  - [`src/game/inherit.rs` (1)](#srcgameinheritrs-1)
  - [`src/game/stage.rs` (1)](#srcgamestagers-1)
  - [`src/game/support_card.rs` (4)](#srcgamesupport_cardrs-4)
  - [`src/game/mod.rs` (1)](#srcgamemodrs-1)
  - [`src/game/traits.rs` (1)](#srcgametraitsrs-1)
  - [`src/game/uma.rs` (6)](#srcgameumars-6)
  - [`src/search/config.rs` (2)](#srcsearchconfigrs-2)
  - [`src/search/flat_search.rs` (13)](#srcsearchflat_searchrs-13)
  - [`src/search/result.rs` (3)](#srcsearchresultrs-3)
  - [`src/search/terminal.rs` (3)](#srcsearchterminalrs-3)
- [umaai (69)](#umaai-69)
  - [`src/config/mod.rs` (8)](#srcconfigmodrs-8)
  - [`src/decision.rs` (4)](#srcdecisionrs-4)
  - [`src/luck.rs` (7)](#srcluckrs-7)
  - [`src/protocol/base.rs` (8)](#srcprotocolbasers-8)
  - [`src/protocol/channel.rs` (9)](#srcprotocolchannelrs-9)
  - [`src/protocol/final_score.rs` (3)](#srcprotocolfinal_scorers-3)
  - [`src/protocol/story.rs` (6)](#srcprotocolstoryrs-6)
  - [`src/record.rs` (4)](#srcrecordrs-4)
  - [`tests/channel.rs` (3)](#testschannelrs-3)
  - [`tests/config.rs` (4)](#testsconfigrs-4)
  - [`tests/luck.rs` (4)](#testsluckrs-4)
  - [`tests/protocol.rs` (6)](#testsprotocolrs-6)
  - [`tests/runner.rs` (3)](#testsrunnerrs-3)
- [umaai-cli (31)](#umaai-cli-31)
  - [`src/render/term.rs` (5)](#srcrendertermrs-5)
  - [`src/render/style.rs` (4)](#srcrenderstylers-4)
  - [`src/render/explain.rs` (4)](#srcrenderexplainrs-4)
  - [`src/render/panel.rs` (4)](#srcrenderpanelrs-4)
  - [`src/render/table.rs` (3)](#srcrendertablers-3)
  - [`src/frontend.rs` (5)](#srcfrontendrs-5)
  - [`src/cli.rs` (6)](#srcclirs-6)
- [umaai-tools (29)](#umaai-tools-29)
  - [`src/baseline.rs` (9)](#srcbaseliners-9)
  - [`src/batch.rs` (5)](#srcbatchrs-5)
  - [`src/cli.rs` (7)](#srcclirs-7)
  - [`src/compare.rs` (3)](#srccomparers-3)
  - [`src/probe.rs` (1)](#srcprobers-1)
  - [`src/report.rs` (4)](#srcreportrs-4)
- [umaai-tests (18)](#umaai-tests-18)
  - [`tests/basic_game.rs` (2)](#testsbasic_gamers-2)
  - [`tests/baseline.rs` (1)](#testsbaseliners-1)
  - [`tests/cli_smoke.rs` (4)](#testscli_smokers-4)
  - [`tests/rule_streams.rs` (3)](#testsrule_streamsrs-3)
  - [`tests/runner_basic.rs` (1)](#testsrunner_basicrs-1)
  - [`tests/search_smoke.rs` (1)](#testssearch_smokers-1)
  - [`tests/session_basic.rs` (2)](#testssession_basicrs-2)
  - [`tests/smoke.rs` (1)](#testssmokers-1)
  - [`tests/tools_smoke.rs` (3)](#teststools_smokers-3)

---

## umaai-core (59)

本 crate 共 59 条测试，分两类：54 条内联单元测试（按源码文件分布，逐条 `assert` 判定随机流、DTO 与纯函数口径）；5 条集成测试（载入真实 `gamedata/`，逐条 `print` 观测后经 `Checker` 聚合，有失败才返回 `Err`）。
关注点：随机派生与可复现性、数据 schema 与语义校验、位段与统计原语的边界口径。

### `src/array.rs` (2)

**属性数组工具**
- `test_add_eq_and_is_default` — 属性数组逐元素累加正确，全零判定随结果翻转
- `test_split_status` — 六元组拆出五维属性与技能点，取值与位置正确

### `src/crn.rs` (5)

**根种子分发**
- `test_seed_at_deterministic` — 同根种子同序号必得同种子，可复现底线
- `test_seed_at_distinct_per_rollout` — 同根下各 rollout 序号互不碰撞
- `test_seed_at_frozen_vector` — 派生值与冻结向量比对，改常数即红
- `test_distinct_root_distinct_sequence` — 换根种子即换序列，跑批不重复

**入口播种**
- `test_from_rng_follows_entry_seed` — 入口种子相同则根种子相同，整局可复现

### `src/data/manifest.rs` (1)

**清单校验**
- `test_manifest_parse` — 清单可解析，必需文件缺失可被识别

### `src/data/source.rs` (2)

**数据来源**
- `test_file_names` — 数据项到文件名的映射，以及目录来源的根目录与标识
- `test_memory_source` — 内存来源的 JSON 文本与 JSON 值两条形态取到同一结果，缺项与坏文本报错并指出来源

### `src/diag.rs` (4)

**编译期开关**
- `test_compiled_in_follows_feature` — 编译期开关取值与编译特性一致

**运行时开关与抑制守卫**
- `test_guard_suppress_and_restore` — 抑制守卫作用域内关闭，释放后恢复默认开启
- `test_guard_restores_prev_value` — 释放后恢复进入前的值，不是硬编码开启
- `test_nested_guards` — 嵌套时内层释放后外层抑制保持

### `src/error.rs` (2)

**错误文案与错误源**
- `test_error_display` — 三类错误文案含路径、原因与定位串
- `test_error_source_chain` — 解析错误暴露底层解析器为错误源

### `src/flags.rs` (4)

**生病位集**
- `test_ill_bits` — 置位、清位、包含查询与裸位往返
- `test_ill_kinds_disjoint` — 具名种类位互不重叠，并集等于全种类位
- `test_ill_serde` — 布尔与整数两种输入形态解析一致，越界报错
- `test_event_flags_serde` — 事件数据里的布尔生病项解析为通配位

### `src/gamedata/card.rs` (2)

**支援卡 DTO**
- `test_short_name` — 短名截断：后段只保留前两字
- `test_pinyin_renames` — 拼音键名按显式重命名落到字段

### `src/gamedata/constants.rs` (3)

**常量表查询**
- `test_status_final_score_saturates` — 属性评分查表两端饱和，越界取表末
- `test_rank_name` — 评价名按分数线取档，两端饱和
- `test_event_distribution` — 事件分布前三项按表取值，第四项取剩余

### `src/gamedata/event.rs` (4)

**事件 DTO**
- `test_trigger_type` — 触发类型按标签解析，缺省为代码事件
- `test_choice_result` — 选项结果数值与枚举双向一致，构造即必定触发
- `test_defaults_and_empty_choices` — 缺省字段补齐，空选项组不 panic
- `test_collection_shape` — 事件表五个分区的键名与数据形状

### `src/gamedata/uma.rs` (3)

**马娘 DTO**
- `test_short_name` — 短名取分隔符后部分，无分隔符取全名
- `test_zip_races` — 生涯比赛回合压成位段，越界回合报错
- `test_update_turn_mask` — 自由比赛按等级门槛置位，等级表未覆盖报错

### `src/race_mask.rs` (4)

**比赛回合位段**
- `test_basic` — 置位、查询、计数与裸位段往返一致
- `test_dedup` — 重复回合只置一位
- `test_window` — 窗口边界内外：越界置位报错、越界查询为否
- `test_serde_transparent` — 对外序列化为裸位段，反序列化还原

### `src/rng.rs` (10)

**确定性与无状态性**
- `test_deterministic` — 同种子两次构造序列逐位一致
- `test_masters_differ` — 不同主种子序列无相同值
- `test_counter_continues` — 第几次取值只与序号有关，与消费路径无关
- `test_clone_independent` — 克隆后各自推进，计数互不影响
- `test_additive_no_xor_collision` — 加法派生不出现异或派生的序号错位撞车

**流派生与隔离**
- `test_stream_tags_isolated` — 不同流标记派生出的序列不重叠
- `test_typed_streams_work` — 类型包装不改变底层序列，重置后归零
- `test_event_rng` — 事件流可正常消费，重置与同种子新建一致
- `test_fill_bytes` — 字节填充按小端铺满且与取字序列一致

**局部流派生**
- `test_fork_local_stream` — 父流只推进一次，同标记确定、异标记分离

### `src/stats.rs` (7)

**切片式统计**
- `test_mean_variance_stdev` — 均值、样本方差与标准差对手算样本成立
- `test_degenerate` — 空序列与单样本取中性值且不 panic
- `test_median` — 中位数在奇数与偶数样本下取值正确
- `test_percentile` — 分位线性插值在端点与中间位置取值正确
- `test_min_max` — 单样本与多样本的最小最大值正确

**增量矩统计**
- `test_moments_matches_slice_stats` — 与切片式均值、标准差口径一致
- `test_moments_degenerate` — 退化样本取中性值，全等样本不出 NaN

### `tests/data_load.rs` (6)

**真实数据根**
- `test_load_real_data` — 真实数据可载入，统计、访问器与掩码初始态符合预期
- `test_init_idempotent` — 同来源重复初始化返回同一实例，异来源（路径或标识）报错
- `test_load_from_memory_source` — 真实数据的 JSON 文本与 JSON 值两条内存来源载入结果与目录来源一致，缺项来源报错
- `test_load_missing_root` — 数据根缺失时载入即报错

**校验与清单门控**
- `test_validate_rejects_broken_data` — 破坏关键表后被语义校验拒绝
- `test_manifest_version_gate` — 清单版本与程序不一致时载入被拒

## umasim (53)

本 crate 共 53 条 `src/**` 内联单元测试（`#[cfg(test)] mod tests`，逐条 `assert` 判定）。
覆盖两个面：`game` 的规则数值口径（构造、继承、卡牌效果、状态位集、评分分量）与 `search` 的搜索契约（可复现、CRN 对齐、预算分配、入口守门）。

### `src/game/base/basic.rs` (5)

**开局与端到端**
- `test_newgame` — 冒烟：固定种子跑满全 78 回合，输出评分与 PT 供基线固化
- `test_variable_deck_length` — 五张卡的短卡组也能构造并跑满整局，人数为卡数加理事长

**动作口径**
- `test_friend_outing_steps` — 友人出行按段占用次数、未用段保留，并挂入待触发事件队列
- `test_aijiao_friendship_unified` — 爱娇对所有正向羁绊统一加 2，训练与事件同一出口，负向不减
- `test_clinic_clears_ill` — 治病清空全部病位与不擅长训练，并回复体力且不超上限

### `src/game/base/stage.rs` (2)

- `test_stage_sequence` — 六个阶段逐个推进且终点唯一，不可越阶或回退
- `test_stage_ids` — 阶段编号与历史 CRN 阶段键逐一对齐，是外部记录的兼容口径

### `src/game/base/mod.rs` (11)

**条件边界**
- `test_can_self_race_bounds` — 自选比赛的回合窗口边界：第 13 回合开放、第 72 回合关闭
- `test_can_friend_outing_bounds` — 出行需已解锁、未进 URA 回合且仍有剩余次数

**友人加成**
- `test_apply_friend_bonus_status_pt` — 加成对五维与 PT 按百分比放大，结果向下取整
- `test_apply_friend_bonus_vital` — 体力加成只作用于正向回复，消耗不被放大
- `test_apply_friend_bonus_no_bonus` — 无加成时各维数值原样不变

**开局构造**
- `test_newgame` — 初始五维按面板与继承独立重算，等级表与友人事件 ID 就位

**随机流分层（P6e′）**
- `test_take_rule_streams_requires_master` — 主种子未注入就取规则流即报错，注入后放行
- `test_reset_rule_streams_follows_turn` — 规则流按 `(主种子, 回合)` 派生：换回合换流、同参数同流、换种子换流
- `test_take_put_roundtrip_keeps_progress` — 取出-放回保留推进过的 counter，不回退到起点
- `test_rule_streams_ignore_decision_draws` — 规则流取值不受外部消耗次数影响
- `test_apply_event_friend_bonus_integration` — 事件生效计入次数，带数值的友人事件确实入账

### `src/game/inherit.rs` (1)

- `test_inherit` — 继承属性与上限按蓝因子与剧本因子公式计算，两次采样随机不同

### `src/game/stage.rs` (1)

- `test_stage_id` — 阶段号的整数往返、展示形态与相等比较

### `src/game/support_card.rs` (4)

**卡对象与面板**
- `test_support` — 不同突破等级共享同一面板，克隆的动态状态互不影响

**效果叠加**
- `test_effect_add` — 训练效果的加算与乘区叠加口径，未知词条静默跳过

**固有**
- `test_unique_effect_21` — 固有 21：编成卡种数达标才加训练效果，不足则无
- `test_unique_effect_5` — 固有 5：按编成卡类型加初始属性，且不动初始 PT

### `src/game/mod.rs` (1)

- `test_friend` — 友人卡按稀有度与突破等级构造，加成字段来自卡面数据

### `src/game/traits.rs` (1)

- `test_sample_bucket_matches_weighted_index` — 零分配分桶采样与加权索引逐位等价，守住数值不变

### `src/game/uma.rs` (6)

**状态位集**
- `test_flag_counts_add_remove` — 次数类状态累加、扣减不为负，互斥位与病位同置同清
- `test_motivation_shield` — 心情盾优先抵挡掉心情、一次只耗一层，涨心情不耗盾
- `test_qiezhe_xiaoqie_exclusive_and_factor` — 切者与小切互斥，PT 折算系数按状态取值

**构造**
- `test_uma` — 初始五维、上限、体力心情默认值与生涯比赛位段均与数据一致
- `test_win_races` — 已赛记录转为回合列表，越界的写入被忽略

**评分口径**
- `test_score_parts_matches_calc_score` — 七个评分分量按公式独立重算，分量和等于总分

### `src/search/config.rs` (2)

- `test_defaults` — 默认配置锁定既有基线，改动即视为口径变更
- `test_builders` — 各 builder 逐字段生效

### `src/search/flat_search.rs` (13)

**可复现性与 CRN**
- `test_search_reproducible` — 同种子两次搜索的中选下标与各候选统计逐项一致
- `test_seed_actually_used` — 换入口种子必须换到不同的随机未来，种子不可形同虚设
- `test_action_order_invariance` — 候选顺序颠倒不影响各动作均分与中选动作
- `test_ordered_rollouts_align_with_crn` — 有序原始分按 rollout 序号与候选共享种子表对齐
- `test_simulate_common_reproducible` — 真实 rollout 同种子同动作逐位可复现且结果有限

**分配与预算**
- `test_uniform_budget_and_probe` — 均匀分配下每候选恰好跑满预算，探针记录计划与成败
- `test_ucb_budget_reproducible` — UCB 先保底一组再追加，跨过预算即停且同种子可复现
- `test_parallel_matches_serial` — 并行与串行结果逐位一致，保序收集不改统计
- `test_parallel_flag_follows_feature` — 并行开关取自本构建的后端能力，请求并行不得绕过特性缺失

**守门与终局**
- `test_strict_rollout` — 严格模式让 rollout 失败冒泡，默认只计数不中断
- `test_max_depth_rejected` — 剧本不支持截断估值时，非零深度在入口被拒
- `test_empty_actions_rejected` — 无候选动作直接报错
- `test_terminal_stats_accumulate` — 终局多维统计随评分累加且不参与排序

### `src/search/result.rs` (3)

- `test_weighted_mean_two_score_groups` — 等频双峰的排名加权口径，越界分数按钳位入桶
- `test_degenerate` — 空样本与单样本取中性值且不触发 panic
- `test_best_action_idx` — 最优动作按下标选出，终局统计与动作等长

### `src/search/terminal.rs` (3)

- `test_record_accumulate_and_visit` — 记录并入统计后按维度对齐，名称与数值同源绑定
- `test_no_terminal_is_zst` — 空终局记录与统计是零尺寸，不改变结果的尺寸
- `define_terminal_record`（doctest，标记 `ignore` 不执行） — 文档示例：终局记录宏的剧本侧声明用法

## umaai (69)

引擎 lib 的测试分两类：`src/**` 内联单元测试 48 条，用 `assert` 直查私有细节与纯函数；`tests/**` 集成测试 20 条，走真实数据根与真机帧夹具，经本地 `Checks` 逐条打印并聚合、失败返回 `Err`。
关注面：配置解析的严格报错口径、运气分装配链、协议段解析与帧信道、决策基线取舍、Runner 端到端事件与可复现契约。

### `src/config/mod.rs` (8)

- `test_parse_base` — 默认层完整配置可解析，字段与预设逐项核对
- `test_missing_field_rejected` — 缺必填字段必须报错并指明字段名
- `test_overlay_partial` — 覆盖层逐字段生效，未提及字段沿用默认层
- `test_overlay_unknown_key_rejected` — 覆盖层拼错键必须报错，不静默忽略
- `test_overlay_file_convention` — 约定覆盖层同名不同目录：存在才合并，缺席不报错
- `test_preset_merge` — 预设按名合并，同名覆盖、异名保留
- `test_preset_errors` — 预设缺名与非法等级表均报错，不静默兜底
- `test_cards_validation` — 卡组重复与空卡组必须被拒

### `src/decision.rs` (4)

- `test_baseline_weighted` — 基线按搜索次数加权，并判为有评分
- `test_baseline_equal_weight` — 搜索次数全为零时按候选等权取基线
- `test_baseline_skips_unscored` — 未评分候选不参与加权也不稀释基线
- `test_baseline_no_scores` — 无带评分候选时基线为零且判无评分

### `src/luck.rs` (7)

- `test_first_turn` — 首次记录无回合运气，初始基线按回合零换算
- `test_two_turns` — 两回合的回合运气与累计运气符合显示分差
- `test_multi_turns` — 累计运气等于末次减初始，仅首回合为空
- `test_epoch_change_resets` — 局号变化即重置累计运气与回合基线
- `test_zero_bonus` — 加成为零时显示分退化为原始分
- `test_display_round_trip` — 显示分与原始分换算严格互逆
- `test_snapshot_keys` — 快照序列化后四个键齐全

### `src/protocol/base.rs` (8)

**通用段解析**
- `test_optional_and_unknown` — 可选字段缺失落默认，未知键被忽略
- `test_required_missing` — 必填字段缺失必须报错
- `test_missing_envelope` — 缺外壳段必须报错

**标志与局号**
- `test_flags_mapping` — 失败率符号映射擅长与否，生病只给布尔则种类未知
- `test_epoch_key` — 局号缺失时退化为马娘 ID

**事件 ID 与回填**
- `test_decode_event_id` — 事件 ID 分族解码，兼容三位序号与未知形态
- `test_apply_key_events_families` — 卡事件按段入账，角色事件按真实 ID 入账
- `test_apply_to_uma` — 技能点回加已花，其余字段以帧为准

### `src/protocol/channel.rs` (9)

本小节的测试分属两处：**帧形态**（`frame_tests`，常驻）与**监听 / 稳定读**（`tests`，仅特性 `file-channel` 下编译）；关闭该特性时后 7 条与 `tests/channel.rs` 一起不编不跑。

**目录与命名（帧形态，常驻）**
- `test_plugin_dir` — 插件目录恒定拼在注入根目录的便携子目录下
- `test_frame_kind_lookup` — 文件名映射信道，非白名单不命中

**事件筛选**
- `test_matched_kinds` — 只认白名单文件名与创建、修改事件

**读取与兜底**
- `test_read_stable` — 内容稳定才返回，空文件与缺失文件报错
- `test_startup_fallback` — 启动兜底按白名单顺序补推现存帧
- `test_dedup_keeps_distinct` — 同内容不重复入队，内容变化一律入队
- `test_final_score_channel_independent` — 两条信道各自去重，互不影响

**通道错误分层**
- `test_watch_error_rereads` — 监视报错留痕并重读白名单兜底
- `test_read_failed_note` — 帧读不到时留痕跳过，等下一事件

### `src/protocol/final_score.rs` (3)

- `test_required_fields` — 终局契约必填字段缺失即解析失败
- `test_usable` — 五维或上限全零的坏帧判不可用
- `test_epoch` — 局号取自单局角色 ID，上一局残留帧不得匹配

### `src/protocol/story.rs` (6)

**槽位映射**
- `test_slots` — 十槽位按位置映射到五维与各项效果
- `test_short_values` — 槽位不足时报错而不 panic

**原文解析**
- `test_parse` — 协议原文可解析，缺字段容忍

**分支成帧**
- `test_two_branches` — 成功与失败两支按默认成功率分概率
- `test_single_branch` — 仅一支带数值时必定触发，多选项交训练员
- `test_no_value_errors` — 两支都无数值必须报错，不静默按零

### `src/record.rs` (4)

- `test_attach_fills_luck` — 挂载后快照与逐候选偏差就位
- `test_attach_accumulates` — 多回合推进基线并累计总运气
- `test_skip_unscored` — 无评分决策不挂载且不推进基线
- `test_epoch_change` — 切局首回合无回合运气且累计清零

### `tests/channel.rs` (3)

真 watcher 用例依赖文件系统事件派发与时序，标了 `#[ignore]`：默认跳过，需 `--ignored`；其余用例只走启动兜底与显式超时两条确定性路径。

- `channel_path_and_config_error` — 路径常量正确；目录缺失即报错，无写入时超时返回
- `channel_startup_fallback` — 启动兜底按白名单顺序补推两份现存帧
- `channel_watches_real_writes` — 真 watcher 写入链路可达且同内容去重，默认跳过、需 `--ignored`

### `tests/config.rs` (4)

- `config_load_default` — 真实默认层字段齐全，预设与继承因子可构造
- `config_has_no_scenario_segments` — 数据根配置不得残留剧本专属段
- `config_overlay_merge` — 覆盖层逐字段生效，未提及字段与预设沿用
- `config_error_paths` — 预设缺名、拼错键、覆盖文件缺失与长度不足均报错

### `tests/luck.rs` (4)

- `luck_config_wired` — 运气分加成与记录开关取自数据根默认配置
- `luck_multi_turn` — 逐回合与累计运气和独立重算一致
- `luck_skip_unscored` — 跳过无评分决策，差值跨过该回合
- `luck_epoch_and_serde` — 切局清零，快照与偏差序列化键齐全

### `tests/protocol.rs` (6)

- `protocol_turn0` — 开局真实帧字段逐项落到局面底座
- `protocol_turn13` — 事件帧还原关键事件与待处理事件
- `protocol_turn31` — 中期帧的出行、赛程与设施计数正确
- `protocol_final_frame` — 终局帧可解析并过三道闸，残留上一局帧被拒
- `protocol_bad_frames` — 缺外壳、空段与字段改名必须报错
- `protocol_event_skipped` — 事件数值缺失只跳过该事件并留痕

### `tests/runner.rs` (3)

- `runner_full_game` — 跑满全局、阶段事件成对、决策按接缝取走且无错误
- `runner_without_source` — 无决策源时照常跑满且无决策事件
- `runner_reproducible` — 同配置两次运行终局与事件序列一致

## umaai-cli (31)

覆盖三层：终端呈现原语（宽度探测、语义着色、词元翻译、面板、表格）、样例前端 `PrintFrontend`、含参数解析的入口层。
判定方式全是内联单元测试的 `assert`，逐条锁住对外可见的口径与退化行为。

### `src/render/term.rs` (5)

**宽度探测与显示宽度**
- `test_resolve_width_explicit_wins` — 显式宽度优先，不被终端探测覆盖
- `test_resolve_width_floors_explicit` — 显式宽度低于下限时收敛到最小可用列数
- `test_resolve_width_fallback` — 探测不到宽度时退化为回退列数
- `test_text_width` — 汉字按两列计宽、转义序列不计宽

**折行**
- `test_wrap_display` — 空格处断行、超宽词硬切、转义序列不计宽也不拆行

### `src/render/style.rs` (4)

**着色开关**
- `test_styler_off_is_plain` — 关闭着色时原样返回且开关状态为假
- `test_paint_keeps_text` — 开启着色后文本内容不被吞掉

**语义色调**
- `test_vital_tone` — 体力按上限归一后的三档阈值，上限缺失时兜底
- `test_delta_tone` — 增减量正负与零分别落到三种色调

### `src/render/explain.rs` (4)

**干劲名查表**
- `test_motivation` — 干劲名按一基下标取表、越界给占位符不 panic

**五维与符号**
- `test_five_status` — 五维按轴名顺序拼成一行
- `test_five_status_missing_names` — 轴名缺失时退化为下标而非 panic
- `test_signed` — 正数显式带加号、零与负数保持原样

### `src/render/panel.rs` (4)

**标题横线**
- `test_title_pads_to_width` — 标题行按显示宽度补齐、正好铺满
- `test_title_narrow_width` — 宽度不足时退化为只留前缀

**键值对齐**
- `test_key_values_align` — 值列按最长键的显示宽度对齐
- `test_key_values_empty` — 空输入返回空串不 panic

### `src/render/table.rs` (3)

**渲染形状**
- `test_render_shape` — 表头加数据行渲染出行数与内容均正确
- `test_empty_grid` — 空表只出表头、空判定只看数据行

**宽度约束**
- `test_wrap_respects_width` — 超宽内容折行、不产生超宽行

### `src/frontend.rs` (5)

**详细度档位**
- `test_normal_merges_by_turn` — 默认档同回合多阶段合并一行、末回合补落
- `test_quiet_only_final` — 静默档不输出任何过程行
- `test_verbose_per_stage` — 详细档每个阶段各出一行

**折行**
- `test_process_line_wraps_to_width` — 过程行超宽折行，每段不超宽且不丢字段

**终局块**
- `test_final_block` — 终局块含评分档次、五维表与轴名

### `src/cli.rs` (6)

**参数解析**
- `test_parse_defaults` — 无参数时各项取默认值
- `test_parse_values` — 各选项逐项取值正确
- `test_parse_errors` — 未知参数、缺值、非法与过窄宽度、互斥开关都报错

**用法与日志**
- `test_help_prints_usage` — 帮助只打印用法、不跑对局
- `test_init_logger_levels` — 级别名校验，重复初始化时新级别即时生效

**选择器口径**
- `test_unknown_selectors_rejected` — 未知剧本 / 训练员经文本配置构造后报错并列出可用值

## umaai-tools (29)

本 crate 是开发工具件（跑批 / 探针 / 基线 / 对照），测试全部为内联单元测试：口径与边界用 `assert` 守门，跑局与搜索的端到端冒烟在 `umaai-tests/tests/tools_smoke.rs`。
集成测试不放本 crate（工具件依赖 `umaai-cli`，而 `umaai-tests` 反向依赖本 crate，端到端统一放测试 crate 一侧，避免依赖边打结）。

### `src/baseline.rs` (9)

**日期与元信息**
- `test_civil_from_days` — 日期换算对齐已知纪元（含闰年与世纪边界）
- `test_today_shape` — 今天日期形如 `YYYY-MM-DD`

**期望与比对**
- `test_expect_of` — 整数与文本走精确比对，非整数与含浮点元素的数组自动带容差
- `test_compare_paths` — 比对按数组逐元素展开，缺项与多项都报
- `test_compare_tolerance` — 零容差下浮点也必须逐位一致，容差内不算不一致
- `test_compare_array_len` — 数组长度不一致直接报长度，不再逐元素展开
- `test_expect_serde` — 裸值与带容差两种写法都能反序列化，带容差分支优先

**输入自检**
- `test_input_validate` — 拦住缺搜索段的固定根用例与非 1 局数的整局用例、空卡组
- `test_run_case_rejects_old_schema` — 格式版本不符即拒绝

### `src/batch.rs` (5)

- `test_distribution` — 分布取自逐局评分，样本数与极值对齐
- `test_distribution_single_sample` — 单局样本的标准差为 0，不产生 NaN
- `test_distribution_empty` — 空样本一律中性值
- `test_spec_guards` — 跑批规格字段原样保留
- `test_throughput_zero_elapsed` — 耗时为 0 时吞吐取 0，不产生无穷

### `src/cli.rs` (7)

- `test_parse_empty` — 无参数时只打用法，不选子命令
- `test_parse_values` — 三个子命令的选项逐项取值正确，公共选项缺省值不受子命令影响
- `test_parse_order_and_forms` — 选项与子命令的先后无关，`--opt=value` 写法同样生效
- `test_parse_errors` — 未知子命令、多余位置参数、陌生开关、缺值、非法数值、过窄宽度、选项归属错都报错
- `test_stray_option_is_named` — 归属错误要指出具体是哪个选项并给用法提示
- `test_help_prints_usage` — 帮助只打用法、不跑任何子命令
- `test_join_and_token` — 五维用逗号连接，机读行的值内不出现空白

### `src/compare.rs` (3)

- `test_plan_validate` — 计划自检：负回合坐标与零回合直接拒绝
- `test_plan_serde_strict` — 计划 JSON 严格解析：拼错键与缺字段都报错，可选字段有缺省
- `test_row_deltas` — 对照行差值按「探测 − 默认」计算（评分 / PT / 五维和）

### `src/probe.rs` (1)

- `test_totals` — 探针总数由逐候选求和，成功 + 失败等于计划数

### `src/report.rs` (4)

- `test_machine_line` — 机读行按 `key=value` 拼接，字段顺序原样保留
- `test_token_folds_whitespace` — 空白折成下划线，值不破坏机读行切分
- `test_machine_line_is_splittable` — 切分后每段都是 `key=value`
- `test_table_block_respects_width` — 表格宽度由调用方给出，窄宽度不产生超宽行

## umaai-tests (18)

本 crate 是跨 crate 集成与端到端测试的所在地，覆盖引擎编排、CLI 装配、会话协议、搜索、随机流分层与工具链六条链路。
集成测试不用 `assert`，统一经 `umaai-tests/src/lib.rs` 的 `Checker` 逐条打印 `ok` / `FAIL`、聚合失败项后返回 `Err`，保证 `cargo test` 有失败信号。
共享夹具集中在 `umaai_tests::testing`：同一组马娘 / 卡组 / 继承 / 等级表与固定训练员，供直接调用、引擎 `Runner`、配置默认层与 CLI 对照。

### `tests/basic_game.rs` (2)

- `basic_game_full_run` — 验固定种子跑满全程、终局不变量与两次运行逐项一致
- `basic_game_seed_varies` — 验随机性来源真实存在：换种子终局评分应有差异

### `tests/baseline.rs` (1)

- `baseline_matches_recorded` — 逐份跑基线并比对（含判定可红的反自证守门与份数下限），不一致即聚合返回 `Err`

### `tests/cli_smoke.rs` (4)

- `cli_full_game_smoke` — 验命令行装配链跑通一局、终局块成形且同参数输出逐字节一致
- `cli_assembly_guards` — 守住入口：运行目录无覆盖层污染、缺数据根与未知参数均报错不兜底（覆盖层宿主目录由用例注入）
- `cli_overlay_from_injected_host_dir` — 验覆盖层只从注入的宿主目录读：坏覆盖层报错并点名选择器，换空目录同一参数正常跑完
- `cli_verbosity_levels` — 验三档详细度各自成形：静默只留终局、详细到阶段

### `tests/runner_basic.rs` (1)

- `runner_matches_direct_run` — 验经引擎编排与直接调用终局逐位一致，且默认配置层与夹具同参数

### `tests/search_smoke.rs` (1)

- `test_search_smoke` — 验搜索的候选结构、预算探针与可复现，不设数值基线

### `tests/session_basic.rs` (2)

- `session_frame_sequence_end_to_end` — 验合成帧序列驱动的解析、路由、决策记录与终局三道闸
- `session_runs_on_real_channel` — 真通道端到端，默认跳过、需 `--ignored`：验文件事件派发到收局

### `tests/rule_streams.rs` (3)

- `rule_streams_survive_decision_draws` — 决策流多抽若干次后局面/事件逐位不变（分流成立的判据，含"分布不许退化成空"守门）
- `fork_branches_share_rule_streams` — 同 rollout 种子的两支逐位一致、换种子换一条规则随机
- `missing_master_is_rejected` — 未注入主种子拒绝推进并给出注入入口，注入后可推进

### `tests/smoke.rs` (1)

- `smoke` — 确认测试 crate 与工作区装配可用，输出冒烟标记即过

### `tests/tools_smoke.rs` (3)

- `test_batch_and_probe` — 验跑批的逐局一致性与分布口径、探针的预算与候选对齐、同参数两次逐位一致
- `test_entry_layer` — 验工具入口的子命令分发、机读行形状与基线校验路径（含未知子命令报错、注入宿主目录的坏覆盖层入口即报错）
- `test_compare` — 验分支对照的机读行与逐回合表、同参数两次逐位一致，探测动作越界与零回合计划都报错

## 维护约定

- 新增 / 删除 / 改名测试时同步更新本文件：一行一条，测试名原样进反引号，说明只写「验什么 / 守什么口径」。
- 条数口径以 `cargo test --workspace --release -- --list` 为准；默认跳过项须注明 `--ignored`。
- 本文件只描述测试现状，不记实现细节与历史改动（历史进 `changelog.md`）。
