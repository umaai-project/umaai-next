# umaai-scenario 模板项目规则

## 对话规则与工作规范

1. **语言**：思考和回答使用中文，语气简洁、专业。
2. **避免信息过载**：需要分析大量代码 / 数据并提取结论时，使用子代理。
3. **方案选择**：涉及不在上下文中的领域知识或需要选型时，停下来给出建议交用户选择，不直接生成代码。
4. **提案前审视**：提案前从「做减法」的角度重新考虑一次。

## 项目上下文

- 本目录是 **umaai-next（引擎仓）的剧本仓模板**：复制到自己的剧本仓后，把 `scenario` 统一替换为
  剧本名（如 `ramen`），以 path / git 依赖引用引擎的 `umaai-core` / `umasim` / `umaai`；
  **不 fork 引擎、不在引擎仓加剧本 crate**。
- 引擎契约：`../../docs/scenario/contracts.md`（`Game` 必备方法 / `Deref`「底座不得与契约同名」/
  View DTO）；接入纪律：`../../docs/scenario/README.md`；仓库形态 / 开发流程 / 性能优化：
  `../../docs/scenario/{repo_form,dev_process,perf_plan}.md`。
- 成熟参考实现：`../../../umaai-onsen`（温泉剧本仓，workspace 五 crate 形态的来源）。
- 本模板文档：`README.md`（结构 / 上手四步 / 依赖三档 / 验收）；本仓文档索引见 `docs/README.md`，
  含开发计划、接入包、stub 登记、机制、术语、测试一览与交接骨架。

## 文档规则

以下文档随代码同步维护，在提交准备阶段统一更新（见「Git 操作」）：

1. **README 状态**：结构 / 验收命令 / 数据说明的现状变化，不写修改过程。
2. **接入包**（`docs/plan/intake.md`）：情报位置、机制 / 数值 / 数据表清单、待确认问题；
   S0 核对期间持续更新，结论分流到机制、术语与 stub。
3. **开发计划**（`docs/plan/scenario_dev_plan.md`）：阶段状态、决策记录、待决事项。
4. **数值占位**（`docs/plan/stubs.md`）：`STUB` / `TBD` 的位置、来源、替换条件与基线影响。
5. **剧本仓文档**：`docs/mechanism.md`（机制）、`docs/glossary.md`（术语与口径）、
   `docs/tests_overview.md`（测试一览）——机制改动、口径决定、测试增删分别更新对应文档。
6. **handoff**（`docs/handoff.md`）：对话上下文很长时，可在当前任务完成的适当时刻提示用户向新
   上下文交接；用户同意后生成一段简短的交接提示词放在该文档里，之后可自行开新对话交接，或提醒
   用户辅助操作。

## 代码结构

**形态：workspace 五 crate，按终端业务形态划分。** `scenario-core` 是**宿主稳定面**
（机制 + 样例训练员 + 装配，下游 Web / Android 只依赖它）；`sim`（离线模拟）与 `ai`（在线决策）
是两条业务线 lib，**终端入口由 `cli` 承载**；`tools` 是外围工具面。业务 crate 之间不允许反向依赖
（sim / ai 不得依赖 cli）。重依赖各归其位，**在对应业务真正用上前不要往 `Cargo.toml` 加依赖或 feature**。

**各 crate 内部按功能区拆文件**（一目录一职责）：改机制时先落对应模块，
**不要把新逻辑堆回 `lib.rs`**。

| crate / 文件 | 职责 |
|---|---|
| `crates/scenario-core/src/lib.rs` | 模块声明 + 对外重导出 + 常量（**不含实现**） |
| `crates/scenario-core/src/game/mod.rs` | 局面 `ScenarioGame`（字段 / `Deref` / 建局 / 快照 `ScenarioView`）+ 装配 `build_game` |
| `crates/scenario-core/src/game/stage.rs` | 阶段机 `Stage`（含 `StageId` 投影） |
| `crates/scenario-core/src/game/action.rs` | `Action` + `Display` + `ActionEnum` + 动作结算入口 |
| `crates/scenario-core/src/game/traits.rs` | 契约 impl（`Game` / `TrainingValueSource` / `FlatSearchGame`） |
| `crates/scenario-core/src/{data,config}.rs` | 剧本资产 schema / 剧本配置段（覆盖层语义） |
| `crates/scenario-core/src/trainer.rs` | 样例训练员（真实剧本换成自己的手写 / 搜索训练员） |
| `crates/scenario-core/src/testing.rs` | 测试定位件（仓根 / 数据根；仓内成员通用，不承诺稳定） |
| `crates/scenario-sim` | 离线模拟业务：跑一局 / 批量跑局 / 基线裁判 / 测卡测量函数；只出 lib，入口由 `cli` 承载（骨架，依赖待落地再登记） |
| `crates/scenario-ai` | 在线决策业务：帧段解析 / Session 接线 / 决策循环；只出 lib，入口由 `cli` 或宿主承载（骨架） |
| `crates/scenario-cli` | 终端入口：bin `scenario`（离线跑局，切 `scenario` / `random` / `manual` 训练员）；在线外壳与手动训练员也在此 |
| `crates/scenario-cli/src/manual.rs` | 无颜色手动训练员（人工核对机制；不引终端重依赖） |
| `crates/scenario-tools` | 外围工具：测卡 / 复盘；单 bin 多子命令（骨架） |
| `docs/` | 本仓文档骨架：索引 / 开发计划 / 接入包 / stub / 机制 / 术语 / 测试 / 交接 |

- **契约 trait impl 不可拆文件**：一个 trait 的 `impl` 必须整体落在同一文件（`Game` 的 22 个方法
  只能随它一起放 `traits.rs`）；可拆的只有 inherent `impl`。
- **可见性**：crate 内跨模块调用的方法标 `pub(crate)`；跨 crate 消费的面标 `pub`（宿主稳定面 =
  core 的装配 / `ScenarioView` / 训练员）。
- **阶段自持**：内部强类型 `Stage`，对外只投影 `StageId`（显式赋号，**改号 = 旧种子失效**）。
- **不碰引擎**：接入新剧本时引擎仓 `git diff` 为空；确需新能力时回引擎仓按「带默认实现、非破坏」
  加钩子，并在提交说明点明「默认行为等价」与否。
- **不预置剧本形态**：模板只留接线与骨架；阶段 / 事件 / 训练 / 比赛 / 资源等强依赖剧本的实现，
  先在 `docs/plan/scenario_dev_plan.md` 列为「待确认项」，机制核对后再自建，避免把猜测固化成代码。

## 数据与配置

- **统一数据目录 `data/`（剧本仓唯一发布库，拷仓即跑）**：剧本资产 `scenario.json` +
  剧本配置默认层 `scenario_config.toml`；建仓时把引擎仓 `gamedata/` 的六张静态表与
  `ai_config.toml` 拷入本目录自持。引擎数据升级时重拷并重录基线。
- bin 的数据根是**代码级常量**（`crates/scenario-cli/src/main.rs` 的 `DATA_ROOT`，跑局从仓根发起），
  不暴露在命令行。引擎覆盖层约定保留：工作目录放同名 `ai_config.toml` / `scenario_config.toml`
  存在才合并（字段级）。
- lib 侧不写死路径：`ScenarioData::load` / `ScenarioConfig::load*` / `Data::init` 的根仍由调用方
  注入（bin / 测试 / 宿主各自决定指向）。
- 配置层未知键**严格报错**；剧本专属配置段归剧本仓，不进引擎配置。

## Rust 编码规范

### 错误处理
1. `Result` 优先；可用 `Result` 的场合不用 `unwrap`。
2. 取 `Option` 内部值时优先 `ok_or_else` 转 `Result`；越界报错，不静默取末档。
3. 仅测试 / 临时代码允许 `unwrap` / `panic`。

### 代码结构
1. **新类型原则**：固定结构的数据建新类型，避免裸 Tuple。
2. **文档化注释**：新增函数 / 结构体须有 rustdoc 注释，函数注释 ≤10 行、文件头 ≤15 行；
   注释必须对应代码现状，历史修改写 `README.md` 状态行或剧本仓文档，不挂在注释上。
3. 变量命名简短；长词可用缩写。
4. **`Deref` 底座不得与契约同名**（`BaseGame` 的公开方法不得与 `Game` 契约方法同名）。

## 测试规范

1. **前期验证**：机制是否成立以**对话核对为主**（`docs/plan/intake.md` 的确认清单），
   自动化测试随业务落地逐步补齐，不用测试代替机制确认。
2. **分层**：本模板只保留**单元测试**（内联，私有细节 / 纯函数）并用 `assert`；机制集成测试、
   基线测试与冒烟测试由真实剧本仓在业务落地后补到对应 crate 的 `tests/`。
3. 测试进程的 cwd 在各 **crate 目录**：数据根 / 基线路径统一经 `scenario_core::testing`
   （`CARGO_MANIFEST_DIR` 上溯两级定位仓根）取。引擎 `umaai-core::testing` 的 `env!` 固化的是
   引擎仓路径，**跨仓不可用**。
4. 模板单测不依赖 `data/` 真实数据（配置与资产用内联文本解析）。
5. **Release 档**跑 binary；`cargo test` 统一加 `--offline`。

## 工具使用

- 日常 `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` /
  `cargo test --workspace --release` 加 `--offline`。
- 需要改 registry（`cargo add` / `fetch`）时**先请求提权**。
- **同一 target 目录一次只跑一个 cargo**（并发会互等锁，表现为卡死）。
- 本模板位于引擎仓内时用 `--manifest-path templates/umaai-scenario/Cargo.toml` 执行；
  复制出仓后从剧本仓根执行。

## Git 操作

1. **不自动 `commit` / `push`**：先确认用户已完成全部调整，再进入提交准备。
2. **提交准备**：同步更新 `README.md` 状态与剧本仓文档，由用户确认后再提交。
3. **提交范围**：涵盖工作树下由 Agent 与用户做的全部修改。
4. 提交说明需点明**默认行为是否等价**（引擎侧）；剧本侧数值位移须在剧本仓文档记口径分歧。
5. 遵循 Git 安全协议，避免破坏性操作。

## 复制模板清单

1. 目录整体复制到剧本仓，**不要**只复制 `crates/scenario-core`。
2. 替换 5 个 package 名与 bin 名中的 `scenario`（如 `ramen-core` / `ramen-cli` / bin `ramen`），
   同步改 `use scenario_core::…` 与 `Cargo.toml` 的 `scenario-core.workspace = true`。
3. 调整引擎依赖路径（开发期 path / 稳定期 git tag / 下游 `default-features = false`）。
4. 把引擎静态数据与 `ai_config.toml` 拷入 `data/`，替换 `scenario.json` / `scenario_config.toml`
   为自己的资产与配置。
5. 填自己的阶段、动作、结算、事件、训练员与 View 字段；补机制测试与固定种子基线。
6. 更新文档：`README.md` / `AGENTS.md` / `docs/` 骨架中的剧本名、数据说明、验收命令与链接；
   接入包、机制、术语、stub、测试一览按真实情况替换占位。
