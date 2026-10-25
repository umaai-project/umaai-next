# umaai-scenario（空剧本仓模板）

`umaai-next` 引擎仓的剧本 workspace 模板：复制本目录到自己的剧本仓，把 `scenario` 统一替换为
剧本名（如 `ramen`）后开工。通用方法与纪律见引擎仓
[`docs/scenario/README.md`](../../docs/scenario/README.md)、
[`repo_form.md`](../../docs/scenario/repo_form.md)、
[`dev_process.md`](../../docs/scenario/dev_process.md)、
[`perf_plan.md`](../../docs/scenario/perf_plan.md) 与
[`contracts.md`](../../docs/scenario/contracts.md)；本仓文档索引见 [`docs/README.md`](docs/README.md)，
仓内规则见 [`AGENTS.md`](AGENTS.md)。

## 结构

与 `umaai-onsen` 同构的 workspace 五 crate：

| crate / 文件 | 职责 |
|---|---|
| `crates/scenario-core/src/lib.rs` | 模块声明 + 对外重导出 + `MAX_TURN`（**不含实现**） |
| `crates/scenario-core/src/game/mod.rs` | 局面 `ScenarioGame`（字段 / `Deref` / 建局 / 快照 `ScenarioView`）+ 装配 `build_game` |
| `crates/scenario-core/src/game/stage.rs` | 阶段机 `Stage`（含 `StageId` 投影） |
| `crates/scenario-core/src/game/action.rs` | `Action` + `Display` + `ActionEnum` |
| `crates/scenario-core/src/game/traits.rs` | 契约 impl（`Game` / `TrainingValueSource` / `FlatSearchGame`） |
| `crates/scenario-core/src/data.rs` | 剧本资产 schema / 载入 / 校验（`scenario.json`） |
| `crates/scenario-core/src/config.rs` | 剧本配置段（`scenario_config.toml`，覆盖层语义） |
| `crates/scenario-core/src/trainer.rs` | 样例训练员 `ScenarioTrainer` |
| `crates/scenario-core/src/testing.rs` | 测试定位件（仓根 / 数据根） |
| `crates/scenario-cli/src/manual.rs` | 无颜色手动训练员（人工核对机制） |
| `crates/scenario-sim` | 模拟器业务：基线裁判 / 批量跑局 / 测卡测量函数（骨架） |
| `crates/scenario-ai` | 在线决策核心（骨架） |
| `crates/scenario-cli` | 终端前端：bin `scenario`（跑局） |
| `crates/scenario-tools` | 外围工具：测卡 / 复盘（骨架） |
| `data/` | 剧本资产 + 剧本配置默认层（引擎静态数据建仓时拷入） |
| `docs/` | 本仓文档骨架（索引 / 计划 / 接入包 / stub / 机制 / 术语 / 测试 / 交接） |

## 文档

| 文档 | 内容 |
|---|---|
| [`docs/README.md`](docs/README.md) | 本仓文档索引与引擎文档入口 |
| [`docs/plan/scenario_dev_plan.md`](docs/plan/scenario_dev_plan.md) | 本剧本开发计划（阶段状态 + 决策记录） |
| [`docs/plan/intake.md`](docs/plan/intake.md) | 接入包：情报位置 / 机制 / 数值 / 数据表 / 待确认 |
| [`docs/plan/stubs.md`](docs/plan/stubs.md) | 数值占位登记（`STUB` / `TBD`） |
| [`docs/mechanism.md`](docs/mechanism.md) | 机制文档（公式 / 流程 / 实现落点） |
| [`docs/glossary.md`](docs/glossary.md) | 术语单一事实源 + 口径决定记录 |
| [`docs/tests_overview.md`](docs/tests_overview.md) | 测试一览（前期以对话核对为主） |
| [`docs/handoff.md`](docs/handoff.md) | 交接提示词 |

## 上手四步

1. **复制 + 改名**：复制整个目录到剧本仓，把 5 个 package 名与 bin 名里的 `scenario` 统一替换为
   剧本名（如 `ramen-core` / `ramen-cli` / bin `ramen`），并同步改 `use scenario_core::…` 等引用。
   保留根 `[workspace]` 表，使剧本仓仍可独立构建。
2. **改依赖路径**：开发期 `path` 指向引擎仓（并列目录 / 仓内相对路径两种写法见下）；稳定期换
   git `tag`；下游宿主（Web / Android）加 `default-features = false`。
3. **补数据**：把引擎仓 `gamedata/` 的六张静态表（`cardDB.json` / `umaDB.json` / `events.json` /
   `text_data_dict.json` / `constants.json` / `manifest.json`）与 `ai_config.toml` 拷入本仓 `data/`；
   剧本仓自此自持数据、拷仓即跑。引擎数据升级时重拷并重录基线（基线由真实剧本仓自持）。
4. **填留白**：`data.rs` / `config.rs` 换成自己的资产与配置段；`game/stage.rs`、`game/action.rs`、
   `game/mod.rs`、`game/traits.rs` 换成自己的阶段、动作、结算与契约实现；`trainer.rs` 换成自己的
   手写 / 搜索训练员。阶段 / 事件 / 训练 / 比赛 / 资源等强依赖剧本的形态**不在模板预置**，
   先在 [`docs/plan/scenario_dev_plan.md`](docs/plan/scenario_dev_plan.md) 列为待确认项，
   机制核对后再实现。

## 依赖三档

```toml
# 1. 开发期：并列目录（引擎仓与剧本仓并列）
umaai-core = { path = "../umaai-next/crates/umaai-core" }
umasim     = { path = "../umaai-next/crates/umasim" }
umaai      = { path = "../umaai-next/crates/umaai" }
umaai-cli  = { path = "../umaai-next/crates/umaai-cli" }   # 仅终端入口需要（样例面）

# 2. 稳定期：同一 repo、同一 tag（cargo 只解析一份检出）
# umaai = { git = "<engine repo>", tag = "engine-v0.1.0" }

# 3. 下游（Web / Android）：自喂帧、无文件监听，并剪掉并行后端
# umaai = { git = "<engine repo>", tag = "engine-v0.1.0", default-features = false }
```

## 运行与验收

```powershell
# 数据根是代码级常量 data/（crates/scenario-cli/src/main.rs 的 DATA_ROOT）
cargo run --release --bin scenario --            # 样例训练员
cargo run --release --bin scenario -- random     # 引擎随机基线（对照用）
cargo run --release --bin scenario -- manual     # 无颜色人工选择（机制核对用）

cargo fmt --all --check
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo test --workspace --release --offline
```

- `cargo check` / `clippy` / `test` 通过（**不改引擎任何一行**）；
- 接入新剧本时引擎仓 `git diff` 为空——确需新能力时回引擎仓按「带默认实现、非破坏」加钩子；
- 三条随机轴（局面 / 事件 / 策略）+ 决策流只喂 `Trainer`；阶段自持、对外只投影 `StageId`；
  同一 `master` 下两次运行 / 两支分支逐位一致；
- 剧本资源给前端走 `ScenarioView`（形态 A 五条纪律），不进契约 trait。
