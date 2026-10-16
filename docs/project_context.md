# miko_next 项目上下文

最后核对于 2026-10-17；描述当前代码结构现状。设计依据见 [`plan/miko_next_plan.md`](plan/miko_next_plan.md)。

## 项目结构

### 工作空间

- Cargo workspace（`resolver = "3"`，edition 2024，`version = 0.1.0`），成员 6 个 crate。
- 仓名 **`umaai-next`**（独立仓，与旧仓 `umaai-rs` 并列）；本地目录暂用 `miko_next/`。旧仓及其 `miko_next` 分支保持不动，作为旧实现与行为基准。
- 依赖方向：`umaai-cli` → `umaai` → `umasim` → `umaai-core`；`umaai-tools` / `umaai-tests` 依赖前三者且 `publish = false`，不进发布产物。
- 层界判据：进 `umaai-core` 的门槛是「**跨算法复用且与游戏、与算法无关**」（「零依赖」只是必要条件）。类型在 core、trait 在 umasim 时，trait impl 写在 `umasim` 一侧。
- workspace 根即项目根（`AGENTS.md` 所在目录）。
- 剧本（外仓）：独立仓，依赖引用本仓 `umaai-core` / `umasim` / `umaai`，**自带 bin**（入口装配点），本仓不反向依赖；剧本专属类型 / 协议段 / 配置 / 数据与测试夹具均在外仓（2026-10-17 拍板，见 plan §2「剧本仓形态」）
- 现状：`umaai-core` 已落地通用机制与静态数据层（RNG 三流、统一错误、诊断输出机制、属性数组工具、通用统计原语与增量矩统计、CRN 种子表、局面状态标识、gamedata 纯 schema/DTO、数据加载与校验）；`umasim` 已落地规则基座与契约 trait（通用枚举 / 阶段位置标识 `StageId` / 友人状态 / 继承 / 马娘 / 支援卡 / 基础局状态与动作 / 无剧本基础局及其私有阶段机 `BasicStage`；`Person` / `ActionEnum` / `Game` / `Trainer` / `TrainingValueSource`）、搜索框架（扁平搜索内核 + 剧本适配层 + 终局多维记录）与契约参考训练员；`umaai` 已落地配置层、前端事件契约（含局面摘要）、离线运行器、运气分与记录装配、协议层（通用段 / 事件段 / 终局帧 / 帧通道）与帧驱动会话（记录落盘待有真实消费者）；`umaai-cli` 已落地呈现工具件、呈现样例与入口层，可开箱跑一局基础局冒烟；`umaai-tools` 仍为骨架。

### crate 定位

| crate | 定位 | 现状 |
|---|---|---|
| `umaai-core` | 核心层：纯 schema/DTO + 数据加载 + 零依赖纯函数工具（统计原语 / CRN 种子表）+ RNG 三流 + 统一错误 + 诊断输出机制 | `rng` / `error` / `diag` / `array` / `stats` / `crn` / `flags` / `race_mask` / `gamedata` / `data` 已落地 |
| `umasim` | 计算层：带行为的领域类型 + 契约 trait（`Game` / `Trainer` / 表来源钩子）+ 领域算法（搜索 / 训练员 / NN）；无终端依赖 | `game/{mod,stage,uma,support_card,inherit,traits}`、`game/base/{mod,stage,person,action,basic}`、`search/{mod,config,result,searchable,terminal,flat_search}`、`trainer` 已落地；呈现 / 协议件延后到上层阶段 |
| `umaai` | 引擎 lib：`Runner` 主循环 / 协议解析（单场景直连）/ `Config` / 决策编排 / luck / record（无终端与 IO 重依赖） | `config`（默认层 + 覆盖层 + 具名预设）、`frontend`（事件契约 + 局面摘要）、`decision`（决策 DTO + 决策源接缝）、`runner`（离线一局驱动 + 终局汇总 + 局面摘要下发）、`luck`（运气分与显示分换算）、`record`（运气分装配挂载）、`protocol`（帧通用段 / 事件段 / 终局帧 / 帧通道）、`session`（帧驱动会话：一帧一决策 + 终局裁决）已落地；记录落盘与入口层接线待后续阶段 |
| `umaai-cli` | 控制台工具件 + 呈现样例 + 入口层；终端依赖唯一出口 | `render`（`term` 宽度探测与颜色门禁、`style` 语义色、`explain` 领域词元、`panel` 面板、`table` 表格封装）、`frontend`（`PrintFrontend` + 三档详细度）、`cli`（参数解析 + 选择器映射 + `run_from_args` + stderr 日志器）已落地；lib + 瘦 bin 形态，pub 面不承诺稳定 |
| `umaai-tools` | 探针 / 基准 / 策略×世界接口；dev 重依赖唯一出口 | 骨架 |
| `umaai-tests` | 集成 / 端到端 / baseline 测试；`Checker`（print + 聚合 + 返回 `Err`）+ 共享夹具 | `Checker` 与 `testing` 夹具、`tests/basic_game.rs`（基础局固定种子端到端）、`tests/search_smoke.rs`（搜索端到端 + 可复现性守门）、`tests/runner_basic.rs`（经引擎运行与直接模拟逐位一致 + 夹具与默认层同值）、`tests/cli_smoke.rs`（入口装配链跑满一局 + 呈现可复现 + 参数严格口径）、`tests/session_basic.rs`（合成帧序列会话端到端 + 真通道用例）已落地；`crates/umaai/tests/` 另有配置 / 运气分 / 协议 / 帧通道集成测试与真机帧夹具（只保留通用段） |

## 依赖与配置

### 依赖

- workspace 依赖：`anyhow` / `colored`（着色，cli 受特性门控）/ `console`（终端宽度与显示宽度探测）/ `enum-iterator` / `hashbrown` / `int-enum` / `log`（门面，启用标准库特性）/ `rand`（锚定 0.9 线）/ `rand_distr` / `rayon`（并行）/ `serde`（`derive`）/ `serde_json` / `tabled`（表格，启用转义序列计宽）/ `thiserror` / `toml`（引擎配置）；内部 crate 以 path 依赖。基础依赖版本按当前最新已发布版本**钉下限**（`rand` 例外，按决策锚定 0.9 线）。
- 核心层 `umaai-core` 只允许**轻量、跨平台、通用**的依赖（现为 `serde` / `serde_json` / `thiserror` / `rand` / `int-enum` / `log` 门面）；提供 feature `diag`（默认开启，诊断输出的编译期裁剪开关）。计算层允许 `rayon`（rollout 并行）与 `log`（诊断与告警）；引擎层 `umaai` 现为 `umaai-core` / `umasim` / `anyhow` / `serde` / `serde_json` / `rand` / `toml` / `notify`（帧通道的文件通知）；终端依赖只在 `umaai-cli`（`tabled` / `console` / 可选 `colored`），dev 重依赖只在 `umaai-tools`。
- 表格库只出现在 `umaai-cli` 的 `render/table.rs`（换库只改该文件）；终端宽度探测与颜色门禁只出现在 `render/term.rs`（其余模块只拿判定结果）。着色受三重门禁：编译期特性 `color`（默认开）、`--no-color` 与 `NO_COLOR` 环境变量、stdout 是否 tty。
- 新增依赖走 `cargo add`，禁止手改 `Cargo.toml`；子 crate 依赖同步到 workspace 依赖。

### 配置与工具链

- `Cargo.toml`：workspace members + `[workspace.dependencies]` + release profile。
- `rust-toolchain.toml`：stable 钉死 1.99.0，组件 `rustfmt` / `clippy`。
- `rustfmt.toml`：稳定工具链可用选项（`max_width = 120` / `reorder_imports` / `style_edition = "2024"` 等）。
- `.gitignore`：`/target`、`logs/`、`__pycache__/`、`*.pyc`。
- `.github/workflows/ci.yml`：`cargo fmt --all --check` → `cargo clippy --workspace --all-targets -- -D warnings` → `cargo test --workspace --release`。
- 帧通道路径（`.portable` / `PluginData/SendGameStatusPlugin` / `thisTurn.json` / `finalScore.json`）是引擎**代码常量**，根目录由宿主注入，不做平台探测；实时监听用 `notify`，相关测试一律带显式超时（超时即失败）。

### 数据（`gamedata/`）

- 单一数据根（只读输入，平铺）：`cardDB.json` / `umaDB.json` / `events.json` / `text_data_dict.json` / `constants.json` / `default_config.toml`；`manifest.json` 声明 schema 版本与必需文件，是载入时的唯一校验入口（当前只覆盖五张静态表，配置文件由引擎配置层单独载入）。
- `default_config.toml` 是**唯一默认源**（Rust 侧不留同名字段缺省），按职责分段：`sim`（剧本与训练员选择器、马娘 / 卡组 / 继承 / 种子 / 局数 / 等级表预设名）、`search`（搜索参数）、`record`（记录开关、根目录、运气分显示分换算加成）、`output`（日志级别）、`presets`（具名预设表，现为比赛等级表）。默认马娘取**目标宽松**的一只，使随机训练员也能跑满全部回合（马娘的目标表决定能否跑满，见 issues）。剧本专属配置段不进本仓，由剧本仓自持。
- 剧本数据不进本仓；加载收敛到 `umaai-core` 的单一 `data` 模块（值类型 `Data` + 全局 `init` / `get` / `try_get`，载入不写日志），**路径由宿主注入**，引擎不硬编码；引擎配置同为路径注入（默认层 + 可选覆盖层）。
- 运行输出（`logs/` 等）不入数据根。

## 文档（`docs/`）

- `plan/`：重建方案（`miko_next_plan.md` 权威 + `v1_plan_reference.md` 参考）。
- `game/`：游戏机制（规则 / 实现细节 / 术语 / 固有效果）。
- `notes/`：开发笔记与待决事项（`open_items.md`）。
- `changelog.md`：变更日志（极简，每条一行）。
- `issues.md`：问题记录（状态 + 排查过程）。
- `project_context.md`：项目上下文（本文件，记录最新代码结构）。

## 开发环境

- 以 Windows 为主，Shell 为 PowerShell。
- 工具链统一 stable 1.99.0；`cargo fmt` 可执行。
- Release profile：`opt-level = 3` / `codegen-units = 1` / `lto = "thin"` / `debug = true`。
- 所有 test 与 binary 用 Release 模式编译。
