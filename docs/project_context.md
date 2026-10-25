# miko_next 项目上下文

最后核对于 2026-10-24；描述当前代码结构现状。设计依据见 [`plan/miko_next_plan.md`](plan/miko_next_plan.md)。

## 项目结构

### 工作空间

- Cargo workspace（`resolver = "3"`，edition 2024，`version = 0.1.0`），成员 7 个 crate。
- 仓名 **`umaai-next`**（独立仓，与旧仓 `umaai-rs` 并列）；本地目录 `umaai_next/`（项目代号 **miko_next**，方案文档沿用该名）。旧仓及其 `miko_next` 分支保持不动，作为旧实现与行为基准。
- 依赖方向：`umaai-cli` → `umaai` → `umasim` → `umaai-core`；`umaai-tools` / `umaai-tests` 依赖引擎并复用 `umaai-cli` 的装配与呈现件，`umaai-tests` 另依赖 `umaai-tools`（基线判定壳用其模型与跑法）；两者 `publish = false`，不进发布产物。`umaai-update` **独立于引擎依赖链**（不依赖任何本仓引擎 crate），由宿主入口按需引用（P6c′-a）。
- 层界判据：进 `umaai-core` 的门槛是「**跨算法复用且与游戏、与算法无关**」（「零依赖」只是必要条件）。
- 类型在 core、trait 在 umasim 时，trait impl 写在 `umasim` 一侧。
- **剧本通用件**：跨剧本复用的机制以**带默认实现、非破坏**的方式进 `umasim` / `umaai`；首批为温泉剧本 M1 前置的四个通用件（比赛事件构造 / 训练后默认处理 / 羁绊统一出口 / 配置覆盖层泛型解析），默认行为不变、L4 基线未位移。2026-10-24 追加两件：红点完整口径可复用件 `BaseGame::do_hint`（**默认行为不等价**，训练路径红点节奏变化、L4 基线已重录）与分桶采样 pub 原语 `sample_bucket`（等价改动，剧本仓已改用引擎件）。
- workspace 根即项目根（`AGENTS.md` 所在目录）。
- 剧本（外仓）：独立仓，依赖引用本仓 `umaai-core` / `umasim` / `umaai`，**自带 bin**（入口装配点），本仓不反向依赖；剧本专属类型 / 协议段 / 配置 / 数据与测试夹具均在外仓（2026-10-17 拍板，见 plan §2「剧本仓形态」）
- 空剧本仓模板 `templates/umaai-scenario/`：**独立 workspace**（根 workspace `exclude`，不进 7 crate 成员表；内部五 crate `scenario-core` / `scenario-sim` / `scenario-ai` / `scenario-cli` / `scenario-tools`，与 `umaai-onsen` 同构），`scenario-core` 只依赖引擎稳定面，`data/` 自持剧本资产与配置默认层，仓内 `AGENTS.md` 记模板规则，`docs/` 自带文档骨架（索引 / 开发计划 / 接入包 / 占位登记 / 机制 / 术语 / 测试 / 交接），`scenario-cli` 提供 `scenario` / `random` / `manual` 三档（manual 为无颜色人工选择）；CI 用 `--manifest-path … --workspace` 单独跑 `fmt` / `check` / `clippy` / `test` 守门；接入指南、仓库形态、开发流程与性能优化见 `docs/scenario/`（P7′）
- 现状：`umaai-core` 已落地通用机制与静态数据层（RNG 三流、统一错误、诊断输出机制、属性数组工具、通用统计原语与增量矩统计、CRN 种子表、局面状态标识、gamedata 纯 schema/DTO、数据加载与校验〔来源抽象：目录 / 内存〕）；`umasim` 已落地规则基座与契约 trait（通用枚举 / 阶段位置标识 `StageId` / 友人状态 / 继承 / 马娘 / 支援卡 / 基础局状态与动作 / 无剧本基础局及其私有阶段机 `BasicStage`；`Person` / `ActionEnum` / `Game` / `Trainer` / `TrainingValueSource`）、随机流分层（决策流只给训练员；局面流与事件流归局面底座、按 `(主种子, 回合)` 每回合派生；策略流由具体局面自持）、搜索框架（扁平搜索内核 + 剧本适配层 + 终局多维记录）与契约参考训练员；通用机制诊断埋点七处（事件 / 羁绊 / 比赛 / 训练后处理 / 失败分支，rollout 期间静默）、零分配泛型分桶采样（休息分档与事件分布热路径，与 rand 加权索引逐位一致；已 pub 化为 `umasim::game::sample_bucket` 供剧本仓复用）；`umaai` 已落地配置层、前端事件契约（含局面摘要）、离线运行器、运气分与记录装配、协议层（通用段 / 事件段 / 终局帧 / 帧通道）与帧驱动会话（记录落盘待有真实消费者）；`umaai-cli` 已落地呈现工具件、呈现样例与入口层，可开箱跑一局基础局冒烟；`umaai-tools` 已落地批量跑批、固定根搜索探针、基线框架、分支推进对照、组卡 / 测卡工具（含自持实验计划、逐槽贪婪与多重比较校正）、输出原语与单 bin 多子命令入口（基线重录在其内）；`umaai-tests` 已落地基线自动判定与工具冒烟，L4 基线可回归、可按需重录；空剧本仓模板（独立 workspace，五 crate）与接入文档已落地（P7′）；`umaai-update` 已落地分发客户端公共库（频道清单协议与发布侧生成、HTTP 与本地测试两种传输、无状态哈希判定、先全下后装的备份回滚式安装），入口接线 / 自更新 / 发布侧待开工（P6c′-a）。

### crate 定位

| crate | 定位 | 现状 |
|---|---|---|
| `umaai-core` | 核心层：纯 schema/DTO + 数据加载 + 零依赖纯函数工具（统计原语〔含配对差分与多重比较校正〕/ CRN 种子表）+ 随机流原语（无状态流 + 三条类型隔离的规则流 + 种子派生）+ 统一错误 + 诊断输出机制 + 小值类型纯文本 `Display`（数值摘要 / 状态词 / 病位集；训练轴名单一出口 `GameConstants::train_name`） | `rng` / `error` / `diag` / `array` / `stats` / `crn` / `flags` / `race_mask` / `gamedata` / `data` 已落地 |
| `umasim` | 计算层：带行为的领域类型 + 契约 trait（`Game` / `Trainer` / 表来源钩子）+ 领域算法（搜索 / 训练员 / NN）；无终端依赖 | `game/{mod,stage,uma,support_card,inherit,traits}`、`game/base/{mod,stage,person,action,basic}`（`BaseGame` 持主种子与两条引擎流，构造期按 `card_id` 拒绝重复支援卡；非卡人头按身份反查、不写死下标〔人头布局契约：卡片前缀逐槽同序 + 非卡尾部追加，前缀不变式有守门测试，训练加成默认实现按前缀直取卡组〕；训练值 / 失败率默认公式与基础局设施等级按基础值表实际长度取值〔等级可多于 5、表外训练点名拒绝〕；**剧本可复用件**：比赛事件构造 `push_race_event`、训练后默认处理 `default_after_train`、羁绊统一出口 `add_friendship`（爱娇与卡面同步只此一处）、红点完整口径 `do_hint`（次数 / 等级双截断 / 单卡累计 / 超限只给属性）、`Person::set_friendship`）、`search/{mod,config,result,searchable,terminal,flat_search,par}`、`trainer` 已落地；呈现 / 协议件延后到上层阶段 |
| `umaai` | 引擎 lib：`Runner` 主循环 / 协议解析（单场景直连）/ `Config` / 决策编排 / luck / record（无终端与 IO 重依赖） | `config`（默认层 + 同名覆盖层约定 + 具名预设；泛型 `OverlayConfig` / `parse_with_overlay` 供剧本仓配置段复用同一套语义）、`frontend`（事件契约 + 局面摘要）、`decision`（决策 DTO + 决策源接缝）、`runner`（离线一局驱动 + 终局汇总 + 局面摘要下发）、`luck`（运气分与显示分换算）、`record`（运气分装配挂载）、`protocol`（帧通用段 / 事件段 / 终局帧 / 帧通道〔特性 `file-channel`〕）、`session`（帧驱动会话：一帧一决策 + 终局裁决）已落地；记录落盘与入口层接线待后续阶段 |
| `umaai-cli` | 控制台工具件 + 呈现样例 + 入口层；终端依赖唯一出口 | `render`（`term` 宽度探测 / 折行与颜色门禁、`style` 语义色、`explain` 领域词元〔轴名经核心层单一出口〕、`panel` 面板、`table` 表格封装）、`frontend`（`PrintFrontend` + 三档详细度）、`cli`（参数解析 + 配置覆盖层〔宿主目录可注入，默认当前目录〕+ 选择器映射 + `run_from_args` + stderr 日志器）已落地；lib + 瘦 bin 形态，pub 面不承诺稳定 |
| `umaai-tools` | 探针 / 基准 / 基线 / 对照 / 组卡测卡工具；dev 重依赖唯一出口（当前零第三方新增） | `batch`（固定种子批量跑局 + 分布统计）、`probe`（固定根搜索成本探针）、`baseline`（基线模型 / 跑法 / 比对 / 读写）、`compare`（分支推进对照：JSON 对照计划 + 单探测动作 vs 默认一选）、`deck`（组卡内核：卡池筛选 / 静态面板分 / 构成枚举〔卡种下限按全卡组 7 类口径，含友人 / 团队与必带卡，口径存疑待 miko 门禁确认〕/ 逐类型实测扫描与多重比较校正 / 两级排序与独立种子集验证 / 自持实验计划 `Plan` / 基础局测量函数 / 卡种原语 `deck_kinds`・`fixed_card_types`）、`arms`（多臂同种子配对跑批内核，测量函数闭包注入）、`cardtest`（测卡内核：固定基准卡组只换一槽或逐槽贪婪 + 显式候选 + 可比性守卫）、`report`（机读单行 + 人读面板与表格）、`cli`（参数解析 + 子命令分发 + 宿主目录可注入）与瘦 bin（单 bin 多子命令 `bench` / `probe` / `baseline` / `compare` / `deck` / `cardtest`）已落地；跑局装配复用 `umaai-cli` 的装配函数、呈现复用其呈现件 |
| `umaai-tests` | 集成 / 端到端 / baseline 测试；`Checker`（print + 聚合 + 返回 `Err`）+ 共享夹具 | `Checker` 与 `testing` 夹具、`baselines/`（一基线一 JSON）、`tests/basic_game.rs`（基础局固定种子端到端）、`tests/search_smoke.rs`（搜索端到端 + 可复现性守门）、`tests/runner_basic.rs`（经引擎运行与直接模拟逐位一致 + 夹具与默认层同值）、`tests/cli_smoke.rs`（入口装配链跑满一局 + 呈现可复现 + 装配链与覆盖层注入守门）、`tests/session_basic.rs`（合成帧序列会话端到端 + 真通道用例）、`tests/baseline.rs`（基线自动判定）、`tests/tools_smoke.rs`（工具跑批 / 探针 / 组卡 / 测卡 / 入口链冒烟）、`tests/smoke.rs`（测试 crate 装配冒烟）已落地；`crates/umaai/tests/` 另有配置 / 运气分 / 协议 / 帧通道集成测试与真机帧夹具（只保留通用段） |
| `umaai-update` | 分发客户端公共库（**零引擎依赖**、不在引擎依赖链上）：频道清单协议与发布侧清单生成 / 传输（HTTP + 本地测试模式）/ 无状态哈希判定 / 备份回滚式安装 | `manifest`（协议 + `build`）、`channel`（源常量 + 频道声明 + 目标）、`transport`（`Transport` trait + `HttpTransport`〔特性 `http`，默认开〕）、`testing`（`TestTransport` 内存假源）、`updater`（`check` / `update` / 临时目录与回滚）、`hash`、`error` 已落地；入口接线 / 自更新 / 发布侧待开工（P6c′-a，方案见 `docs/plan/tools_update.md`） |

## 依赖与配置

### 依赖

- workspace 依赖：`anyhow` / `colored`（着色，cli 受特性门控）/ `console`（终端宽度与显示宽度探测）/ `enum-iterator` / `hashbrown` / `int-enum` / `lexopt`（命令行解析，工具入口用）/ `log`（门面，启用标准库特性）/ `rand`（锚定 0.9 线）/ `rand_distr` / `rayon`（并行后端，**可选依赖**）/ `serde`（`derive`）/ `serde_json` / `sha2`（哈希，更新库用）/ `tabled`（表格，启用转义序列计宽）/ `thiserror` / `toml`（引擎配置）/ `ureq`（HTTP 客户端，**可选依赖**，更新库用，只开 rustls、关默认特性）；内部 crate 以 path 依赖。基础依赖版本按当前最新已发布版本**钉下限**（`rand` 例外，按决策锚定 0.9 线）。
- 核心层 `umaai-core` 只允许**轻量、跨平台、通用**的依赖（现为 `serde` / `serde_json` / `thiserror` / `rand` / `int-enum` / `log` 门面）；提供 feature `diag`（默认开启，诊断输出的编译期裁剪开关）。计算层 `umasim` 的 `rayon` 是**可选依赖**：特性 `parallel`（默认开启）关闭后批量 rollout 退化到串行分支、rayon 不进依赖图（能力查询 `search::parallel_supported()`）；引擎层 `umaai` 以 `default-features = false` 引 `umasim` 并同名透传 `parallel`，故下游 `default-features = false` 可整链剪掉 rayon。引擎层 `umaai` 现为 `umaai-core` / `umasim` / `anyhow` / `serde` / `serde_json` / `rand` / `toml` / `notify`（帧通道的文件通知，**可选依赖**：特性 `file-channel` 默认开启，关闭后连依赖一起去掉，帧经 `Session::step` 送入）；终端依赖只在 `umaai-cli`（`tabled` / `console` / 可选 `colored`），dev 重依赖只在 `umaai-tools`；**网络依赖只在 `umaai-update`**（`sha2` + 可选 `ureq`，特性 `http` 默认开，关闭后宿主自备 `Transport`，引擎与剧本都不依赖该 crate）。
- 表格库只出现在 `umaai-cli` 的 `render/table.rs`（换库只改该文件）；终端宽度探测与颜色门禁只出现在 `render/term.rs`（其余模块只拿判定结果）。着色受三重门禁：编译期特性 `color`（默认开）、`--no-color` 与 `NO_COLOR` 环境变量、stdout 是否 tty。
- 工具层 `umaai-tools` 现为 `umaai-core` / `umasim` / `umaai` / `umaai-cli`（复用其装配函数与呈现件）/ `anyhow` / `serde`（derive）/ `serde_json` / `rand` / `lexopt`（选项解析）——除命令行解析外没有额外第三方依赖（统计走核心层原语、计时走标准库、日期换算内置）；测试层 `umaai-tests` 另依赖 `umaai-tools`（基线判定壳）。工具件与下游的边界：`umaai-tools` 属主线样例面，pub 面不承诺稳定、不进发布产物。
- 新增依赖走 `cargo add`，禁止手改 `Cargo.toml`；子 crate 依赖同步到 workspace 依赖。
- 更新库 `umaai-update` 现为 `serde`（derive）/ `serde_json` / `sha2` / `thiserror` / `ureq`（可选，特性 `http` 默认开）；**不依赖引擎任何 crate**。宿主自备传输只需实现 `Transport`；`TestTransport` 提供不联网的内存假源（库自身测试与宿主自测用）。
- 随机流纪律（P6e′）：**决策流**是 `run_stage` 的参数、只喂 `Trainer`；**局面流**与**事件流**由 `BaseGame` 持有，按 `derive_seed(master, [回合, tag])` 每回合重新派生；**策略流**（动作结算与剧本机制）由具体 `Game` 自持并从同一 master 派生。`master` 由整局入口（`Runner` / `Session`，取 `sim.seed`）与搜索分支（`fork_for_rollout(rollout_seed)`）注入，**未注入即报错**；搜索不再有「按阶段重播种」这一档。基座侧的入口叫 `init_rule_streams`、取值叫 `master`，与契约方法刻意不同名（见 `AGENTS.md` 的 `Deref` 底座命名纪律）。

### 配置与工具链

- `Cargo.toml`：workspace members + `[workspace.dependencies]` + release profile。
- `rust-toolchain.toml`：stable 钉死 1.99.0，组件 `rustfmt` / `clippy`。
- `rustfmt.toml`：稳定工具链可用选项（`max_width = 120` / `reorder_imports` / `style_edition = "2024"` 等）。
- `.gitignore`：`/target`、`logs/`、`__pycache__/`、`*.pyc`、`templates/**/target/`。
- `.github/workflows/ci.yml`：`cargo fmt --all --check` → `cargo clippy --workspace --all-targets -- -D warnings` → `cargo test --workspace --release`；另守两档降级形态：workspace 全默认特性关闭仍无 warning 编过（同时覆盖更新库「关掉 HTTP 特性、宿主自备传输」的形态）、串行引擎（`-p umasim -p umaai --no-default-features`）跑测试并断言依赖图里没有 rayon；末尾单独一步守空剧本模板（独立 workspace，`fmt` + `check` + `clippy -D warnings` + `test --release`，均带 `--manifest-path templates/umaai-scenario/Cargo.toml --workspace`）。
- 帧通道路径（`.portable` / `PluginData/SendGameStatusPlugin` / `thisTurn.json` / `finalScore.json`）是引擎**代码常量**，根目录由宿主注入，不做平台探测；实时监听用 `notify`（特性 `file-channel`），相关测试一律带显式超时（超时即失败）。帧的形态（信道种类 / 帧原文 / 痕迹类型 / 目录常量）不带特性依赖，常驻可用。

### 数据（`gamedata/`）

- 单一数据根（只读输入，平铺）：`cardDB.json` / `umaDB.json` / `events.json` / `text_data_dict.json` / `constants.json` / `ai_config.toml`（默认层）；`manifest.json` 声明 schema 版本与必需文件，是载入时的唯一校验入口（当前只覆盖五张静态表，配置文件由引擎配置层单独载入）。
- 配置两份、**同名不同位**：数据根内的 `ai_config.toml` 是**默认层**（唯一默认源，Rust 侧不留同名字段缺省），宿主目录内同名的另一份是**覆盖层**（桌面入口＝当前目录，存在才合并，段缺失整段沿用）；宿主目录由装配入口注入（默认当前目录），测试因此不污染运行目录。默认层按职责分段：`sim`（剧本与训练员选择器、马娘 / 卡组 / 继承 / 种子 / 局数 / 等级表预设名）、`search`（搜索参数）、`record`（记录开关、根目录、运气分显示分换算加成）、`output`（日志级别）、`presets`（具名预设表，现为比赛等级表）。默认马娘取**目标宽松**的一只，使随机训练员也能跑满全部回合（马娘的目标表决定能否跑满，见 issues）。剧本专属配置段不进本仓（模板样例除外），由剧本仓自持。
- 剧本数据不进本仓（`templates/umaai-scenario/data/` 的模板样例资产除外）；加载收敛到 `umaai-core` 的单一 `data` 模块：来源抽象 `DataSource`（`table<T: DeserializeOwned>` + `key`，内置 `DirSource` 目录 / `MemorySource` 内存 JSON 值或文本）、值类型 `Data`（`from_source` / `load`）与全局 `init` / `init_from` / `get` / `try_get`（幂等凭来源标识，载入不写日志）；**来源与配置都由宿主注入**，引擎不硬编码路径，也不假设数据在磁盘上。
- 运行输出（`logs/` 等）不入数据根。
- 分发与更新：公共库 `umaai-update` 已落地、入口尚未接线（P6c′-a）——更新源是产品 crate 的编译期常量；远端 `update.json` 清单逐文件登记哈希、可选 `schema_version` 门控；`ai_data` 频道装到数据根（含默认层配置），`ura_data` 频道装到发布目录的 `.portable`；判定无状态（清单哈希 vs 本地文件），安装先全下后装、失败回滚，临时目录在工作目录下且成功后删除。详见 [`plan/tools_update.md`](plan/tools_update.md)。
- 基线数据不在数据根：`crates/umaai-tests/baselines/*.json`（一基线一 JSON：`input` 自持、`expect` 逐项带容差、`meta` 记版本与日期**不含路径**）；判定见 `crates/umaai-tests/tests/baseline.rs`，重录走 `umaai-tools baseline --update`。

## 文档（`docs/`）

- `plan/`：方案（`miko_next_plan.md` 权威 + `v1_plan_reference.md` 参考 + `tools_deck_cardtest.md` 组卡 / 测卡 + `tools_update.md` 自动更新）。
- `game/`：游戏机制（规则 / 实现细节 / 术语 / 固有效果）。
- `scenario/`：剧本仓接入指南（`README.md`）+ 仓库形态（`repo_form.md`）+ 开发流程（`dev_process.md`）+ 性能优化（`perf_plan.md`）+ 契约参考（`contracts.md`，P7′）；模板 `templates/umaai-scenario/docs/` 另有本仓文档骨架。
- `notes/`：开发笔记与待决事项（`open_items.md`）与 miko 新剧本预告（`miko_scenario_preview.md`，持续更新的未确认情报）。
- `changelog.md`：变更日志（极简，每条一行）。
- `issues.md`：问题记录（状态 + 排查过程）。
- `tests_overview.md`：测试一览（分层 + 一行一测试，随实现更新）。
- `project_context.md`：项目上下文（本文件，记录最新代码结构）。

## 开发环境

- 以 Windows 为主，Shell 为 PowerShell。
- 工具链统一 stable 1.99.0；`cargo fmt` 可执行。
- Release profile：`opt-level = 3` / `codegen-units = 1` / `lto = "thin"` / `debug = true`。
- 所有 test 与 binary 用 Release 模式编译。
