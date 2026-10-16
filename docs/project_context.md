# miko_next 项目上下文

最后核对于 2026-10-17；描述当前代码结构现状。设计依据见 [`plan/miko_next_plan.md`](plan/miko_next_plan.md)。

## 项目结构

### 工作空间

- Cargo workspace（`resolver = "3"`，edition 2024，`version = 0.1.0`），成员 6 个 crate。
- 依赖方向：`umaai-cli` → `umaai` → `umasim` → `umaai-core`；`umaai-tools` / `umaai-tests` 依赖前三者且 `publish = false`，不进发布产物。
- 层界判据：进 `umaai-core` 的门槛是「**跨算法复用且与游戏、与算法无关**」（「零依赖」只是必要条件）。类型在 core、trait 在 umasim 时，trait impl 写在 `umasim` 一侧。
- workspace 根即项目根（`AGENTS.md` 所在目录）。
- 现状：`umaai-core` 已落地通用机制与静态数据层（RNG 三流、统一错误、诊断输出机制、属性数组工具、通用统计原语与增量矩统计、CRN 种子表、局面状态标识、gamedata 纯 schema/DTO、数据加载与校验）；`umasim` 已落地规则基座与契约 trait（通用枚举 / 友人状态 / 继承 / 马娘 / 支援卡 / 基础局状态与动作 / 无剧本基础局；`Person` / `ActionEnum` / `Game` / `Trainer` / `TrainingValueSource`）、搜索框架（扁平搜索内核 + 剧本适配层 + 终局多维记录）与契约参考训练员；其余 crate 仍为骨架，尚无实际模块实现。

### crate 定位

| crate | 定位 | 现状 |
|---|---|---|
| `umaai-core` | 核心层：纯 schema/DTO + 数据加载 + 零依赖纯函数工具（统计原语 / CRN 种子表）+ RNG 三流 + 统一错误 + 诊断输出机制 | `rng` / `error` / `diag` / `array` / `stats` / `crn` / `flags` / `race_mask` / `gamedata` / `data` 已落地 |
| `umasim` | 计算层：带行为的领域类型 + 契约 trait（`Game` / `Trainer` / 表来源钩子）+ 领域算法（搜索 / 训练员 / NN）；无终端依赖 | `game/{mod,uma,support_card,inherit,traits}`、`game/base/{mod,person,action,basic}`、`search/{mod,config,result,searchable,terminal,flat_search}`、`trainer`（契约参考训练员）已落地；呈现 / 协议件延后到上层阶段 |
| `umaai` | 引擎 lib：`Runner` 主循环 / 协议解析 / `Config` / 决策编排 / luck / record | 骨架 |
| `umaai-cli` | print 前端（`Frontend` 实现）+ 入口层功能；终端呈现唯一出口 | 骨架（`main` 仅占位输出） |
| `umaai-tools` | 探针 / 基准 / 策略×世界接口；dev 重依赖唯一出口 | 骨架 |
| `umaai-tests` | 集成 / 端到端 / baseline 测试；`Checker`（print + 聚合 + 返回 `Err`） | `Checker` 与 `tests/basic_game.rs`（基础局固定种子端到端）、`tests/search_smoke.rs`（搜索端到端 + 可复现性守门）已落地 |

## 依赖与配置

### 依赖

- workspace 依赖：`anyhow` / `enum-iterator` / `hashbrown` / `int-enum` / `log`（门面）/ `rand`（锚定 0.9 线）/ `rand_distr` / `rayon`（并行）/ `serde`（`derive`）/ `serde_json` / `thiserror`；内部 crate 以 path 依赖。基础依赖版本按当前最新已发布版本**钉下限**（`rand` 例外，按决策锚定 0.9 线）。
- 核心层 `umaai-core` 只允许**轻量、跨平台、通用**的依赖（现为 `serde` / `serde_json` / `thiserror` / `rand` / `int-enum` / `log` 门面）；提供 feature `diag`（默认开启，诊断输出的编译期裁剪开关）。计算层允许 `rayon`（rollout 并行）与 `log`（诊断与告警）；终端依赖只在 `umaai-cli`，dev 重依赖只在 `umaai-tools`。
- 新增依赖走 `cargo add`，禁止手改 `Cargo.toml`；子 crate 依赖同步到 workspace 依赖。

### 配置与工具链

- `Cargo.toml`：workspace members + `[workspace.dependencies]` + release profile。
- `rust-toolchain.toml`：stable 钉死 1.99.0，组件 `rustfmt` / `clippy`。
- `rustfmt.toml`：稳定工具链可用选项（`max_width = 120` / `reorder_imports` / `style_edition = "2024"` 等）。
- `.gitignore`：`/target`、`logs/`、`__pycache__/`、`*.pyc`。
- `.github/workflows/ci.yml`：`cargo fmt --all --check` → `cargo clippy --workspace --all-targets -- -D warnings` → `cargo test --workspace --release`。

### 数据（`gamedata/`）

- 单一数据根（只读输入，平铺）：`cardDB.json` / `umaDB.json` / `events.json` / `text_data_dict.json` / `constants.json` / `default_config.toml`；`manifest.json` 声明 schema 版本与必需文件，是载入时的唯一校验入口。
- 剧本数据不进本仓；加载收敛到 `umaai-core` 的单一 `data` 模块（值类型 `Data` + 全局 `init` / `get` / `try_get`，载入不写日志），**路径由宿主注入**，引擎不硬编码。
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
