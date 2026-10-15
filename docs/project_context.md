# miko_next 项目上下文

最后核对于 2026-10-15；描述当前代码结构现状。设计依据见 [`plan/miko_next_plan.md`](plan/miko_next_plan.md)。

## 项目结构

### 工作空间

- Cargo workspace（`resolver = "3"`，edition 2024，`version = 0.1.0`），成员 6 个 crate。
- 依赖方向：`umaai-cli` → `umaai` → `umasim` → `umaai-core`；`umaai-tools` / `umaai-tests` 依赖前三者且 `publish = false`，不进发布产物。
- workspace 根即项目根（`AGENTS.md` 所在目录）。
- 现状：`umaai-core` 已落地通用机制（RNG 三流、统一错误）；其余 crate 仍为骨架（crate 级模块文档 + 占位代码），尚无实际模块实现。

### crate 定位

| crate | 定位 | 现状 |
|---|---|---|
| `umaai-core` | 核心层：与游戏无关的通用机制（RNG 三流 + 统一错误）；游戏契约归 `umasim` | `rng` / `error` 已落地 |
| `umasim` | 计算层：规则基座 / 搜索框架 / 策略与评估 / 数据加载（无终端依赖） | 骨架 |
| `umaai` | 引擎 lib：`Runner` 主循环 / 协议解析 / `Config` / 决策编排 / luck / record | 骨架 |
| `umaai-cli` | print 前端（`Frontend` 实现）+ 入口层功能；终端依赖唯一出口 | 骨架（`main` 仅占位输出） |
| `umaai-tools` | 探针 / 基准 / 策略×世界接口；dev 重依赖唯一出口 | 骨架 |
| `umaai-tests` | 集成 / 端到端 / baseline 测试；`Checker`（print + 聚合 + 返回 `Err`） | 骨架（含 `tests/smoke.rs` 冒烟） |

## 依赖与配置

### 依赖

- workspace 依赖：`anyhow` / `rand`（锚定 0.9 线）/ `serde`（`derive`）/ `serde_json` / `thiserror`；内部 crate 以 path 依赖。
- 核心层 `umaai-core` 只允许轻量依赖；终端依赖只在 `umaai-cli`，dev 重依赖只在 `umaai-tools`。
- 新增依赖走 `cargo add`，禁止手改 `Cargo.toml`；子 crate 依赖同步到 workspace 依赖。

### 配置与工具链

- `Cargo.toml`：workspace members + `[workspace.dependencies]` + release profile。
- `rust-toolchain.toml`：stable 钉死 1.99.0，组件 `rustfmt` / `clippy`。
- `rustfmt.toml`：稳定工具链可用选项（`max_width = 120` / `reorder_imports` / `style_edition = "2024"` 等）。
- `.gitignore`：`/target`、`logs/`、`__pycache__/`、`*.pyc`。
- `.github/workflows/ci.yml`：`cargo fmt --all --check` → `cargo clippy --workspace --all-targets -- -D warnings` → `cargo test --workspace --release`。

### 数据（`gamedata/`）

- 单一数据根（只读输入，平铺）：`cardDB.json` / `umaDB.json` / `events.json` / `text_data_dict.json` / `constants.json` / `default_config.toml`。
- 剧本数据不进本仓；加载收敛到 `umasim` 的单一 `data` 模块，**路径由宿主注入**，引擎不硬编码。
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
