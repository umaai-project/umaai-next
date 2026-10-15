# miko_next 重建方案（供审阅）

> 本文合并《umaai-rs 技术方案报告 V1.0》与 P0 实测结果。按项目要求做了两处调整：
> **(1) 本仓不是上游 umaai / umasim 项目的移植**——仅借鉴机制思想，**不照搬其实现与架构**（见 §1.3）；
> **(2) 契约层命名为 `umaai-core`（核心层）**，RNG 等零重依赖的基础件直接放入核心层（见 §3.1）。
> 当前分支 `miko_next`（重建分支）；旧实现保留于 `master` 与 `*_backup` 分支。

---

## 0. 摘要

把 umaai-rs 重构成「**通用引擎 + 可插拔剧本**」：引擎不认识任何具体剧本，剧本是插在一组 trait 上的实现。下游新剧本放**独立仓库**，只依赖一个零重依赖的契约面（trait + 基础类型），在自己入口完成装配。

到 10-28 只做与具体剧本无关的地基：域类型、钩子 trait、引擎主循环、RNG、数据加载、golden 基线、空剧本模板——不会因剧本细节返工。28 日后只填「剧本层」：数值表、剧本状态、动作映射、策略、协议字段。

新剧本**不需要从零迭代**：引擎 / RNG / 状态机 / 搜索框架 / golden / 通道约占 70% 工作量可直接复用；新写的是剧本规则与策略，并对策略本身做调参。

连通游戏走既有 C# 插件 ↔ urafile/JSON ↔ Rust 通道；引擎通过 `Frontend` 回调输出，**自己绝不打印**。

**三大风险**：① keep 文件的依赖闭包牵连剧本类型；② 多仓下不能用「枚举集中注册」，要改成「外部 impl + 入口自组装」；③ 上游合规——只重写算法，不拷源码、不照搬架构。

---

## 1. 背景与目标

### 1.1 为什么

玩家同一时间只玩一个剧本，旧架构却维护「多剧本并存 + 运行时按 scenario 分发」，导致：

- 通用模块里内嵌剧本分支（「通用的壳、剧本的芯」），典型如 `basic.rs` 的 `training_basic_value()` 直接读 `ONSENDATA`
- 呈现层（`HumanReadableSink`/`StdoutJsonSink`）压在**纯计算 crate** 里，下游（手机版、DLL 版）被迫依赖终端库
- 183 处内联测试、无独立 `tests/`，调参没有可复现基线

### 1.2 目标

- 引擎与剧本彻底解耦，核心默认只含无剧本的 `BasicGame`
- lib 化，给手机版 / DLL 版 / Web 等下游提供稳定公共 API
- 固化 golden 基线，支撑后续调参与回归
- 为新剧本提供「填空式」接入路径，28 日拿到资料即可开工，不碰引擎

### 1.3 与上游的关系（合规与边界）★

- 本仓**不是**上游 Kotlin 项目的移植，**不二进制依赖、不源码拷贝**；上游仅作**机制参照**。
- **只借鉴思想，不照搬架构**：例如上游的「枚举集中注册 + 元数据枚举」模式在本仓**不适用**（多仓下外部剧本无法被枚举），我们改为「trait + 外部 impl + 入口自组装」；钩子清单也只作为**机制覆盖备忘**（附录 B），接口命名与切分由本仓按自身需求定义（P1 定稿）。
- 建立「机制对照表」：上游每次大改，评估是否需同步**机制**（非代码），以独立文档跟进。

---

## 2. 总体架构

### 2.1 分层

自上而下四层，另有独立的新剧本仓面向契约编程：

| 层 | crate / 形态 | 职责 |
|---|---|---|
| 前端层（可替换） | `umaai-cli`(print) · 手机版 · 嵌入 DLL 版 · Web | 实现 `Frontend` trait，或消费 JSON 事件流 |
| 引擎层 | crate: `umaai` | `Runner` 主循环 · 协议解析 · `Config` · 决策编排 · luck/record（无终端 / IO 重依赖） |
| 计算层 | crate: `umasim` | 规则基座 · 搜索框架 · `Trainer`/`Evaluator` · 通用输出（纯计算，无终端依赖） |
| **核心层 ★ 新增** | crate: `umaai-core` | 契约 trait + 基础类型 + **RNG 三流**（零重依赖） |
| 新剧本仓库（独立） | 外部仓 | `impl` 各钩子；剧本 State · 数值表 JSON · 自己的 `Trainer` · 自己的 bin/lib |

辅助：`umaai-tools`（探针 / 基准 / golden runner，`publish = false`）；`umaai_review` 移出 workspace members，代码留盘搁置。

### 2.2 workspace 成员

```
[workspace]
members = [
  "crates/umaai-core",     # 核心层：契约 trait + 基础类型 + RNG
  "crates/umasim",         # 计算层
  "crates/umaai",          # 引擎 lib
  "crates/umaai-cli",      # print 前端 bin
  "crates/umaai-tools",    # 探针/基准/golden（publish = false）
]
exclude = ["crates/umaai_review"]
```

### 2.3 依赖铁律

- **单向依赖**：前端 → 引擎 → 计算 → 核心；禁止反向与环依赖
- **核心零重依赖**：`umaai-core` 只允许 `serde`（derive）这类轻量库；不碰 `colored`/`ndarray`/`tokio`/`crossterm`
- **终端依赖只在 `umaai-cli`**：`colored`、`comfy-table`、`indicatif`、`inquire`、`crossterm`、`flexi_logger` 不得进入 lib
- **dev 依赖只在 `umaai-tools`**：`criterion`、`ndarray`、`csv`、`pprof` 不进 shipping 产物
- **下游新仓只面向契约 trait 编程**，禁止直接引用具体剧本类型

> 为什么核心单独成 crate：新仓若直接依赖 `umasim`/`umaai`，会被拖入计算与呈现依赖，版本耦合、编译变慢，解耦形同虚设。独立核心包还能让「引擎升级」与「剧本开发」各走各的版本号。

---

## 3. 各层职责

### 3.1 umaai-core（核心层，新增，最核心）★

- **基础类型**：`Status`（五维 + 体力/干劲等）、`Turn`、`ScenarioId`、`PersonId`、`ActionId`、`Score`、事件枚举、错误类型（`EngineError`）
- **元数据**：`ScenarioMeta { turn, level_up_turns, guest_member, has_second_training_status, ... }`（结构体，**非枚举**）
- **RNG 三流**：`rng.rs` **逐字迁入**（核心资产，保证可复现；零重依赖，适合放核心层）
- **钩子 trait**：`ScenarioRules`、`ScenarioLifecycle`、`TrainingValueSource`、`FlatSearchGame`、`ActionIndex`
- **策略 trait**：`Trainer<G>`、`Evaluator<G>`、`RolloutBackend`、`Policy<G>`
- **输出 trait**：`Frontend`（info/error/decision/record 回调）、`DecisionSink`
- 纪律：**全部钩子提供默认空实现**；跨边界类型 `#[derive(Serialize, Deserialize, Debug, Clone)]`；枚举 `#[non_exhaustive]`

### 3.2 umasim（纯计算层）

- 规则基座：`game/{uma,support_card,inherit,simulator}`、`base/{person,action}`、通用枚举（`TurnStage`/`PersonType`/`FriendState`）
- 搜索框架：`flat_search`/`searchable`/`result`/`seeds`/`terminal`
- 策略与评估：`trainer/*`（去剧本特化）、`neural/{evaluator,value_output}`
- 通用输出：`output/{diagnostic,decision_log}` + `DecisionSink` 的实现
- **不含任何 `println!`、颜色、进度条、文件路径硬编码**（文件 IO 由上层注入）

### 3.3 umaai（引擎 lib）

- `Runner`：主循环，签名 `Runner::run(cfg: Config, frontend: &mut dyn Frontend) -> Result<Summary>`
- 协议：`protocol/urafile.rs`（通道基座）+ **单场景解析**（去 scenarioId 12/14 分发）
- 在线记录链路：watch → record → emit；luck 跟踪、`decision/{record,zip_export}`、`plot/`
- `Config`（serde）：下游不经 CLI 即可构造
- 把内部状态变化翻译成事件回调给 `Frontend`，自己不打印

### 3.4 umaai-cli（print 前端 bin）

- 着色、表格、进度、交互（`inquire`）、日志、windows ico、release 结束暂停
- 实现 `Frontend`，把事件渲染成人读文本 / 表格
- 只服务 `BasicGame` 冒烟与本机调试；**不含任何剧本名分支**，特殊展示一律以「标签/模板键 + 数据」驱动

### 3.5 umaai-tools（测试工具，publish=false）

- 收拢探针 bin：`luck_probe`、`luck_replay`、`turn_inspect`、`rest_pair_probe`（通用化后改名）
- golden runner：固定 CRN 种子跑 rollout，比对期望分值，`--update` 回写基线
- 基准：`criterion`、配对策略 bench
- 通过各 lib 的 `#[doc(hidden)] pub mod testing`（testkit）访问内部状态

### 3.6 新剧本仓库（28 日后）

`state.rs` 剧本专属状态；`rules.rs` impl `ScenarioRules`；`lifecycle.rs` impl `ScenarioLifecycle`；`search.rs` impl `FlatSearchGame` + `ActionIndex`；`trainer.rs` 手写 / NN 策略；`data/*.json` 训练表、事件、NPC、配方；`main.rs`/`lib.rs` 组装并调 `Runner::run`。

---

## 4. 各部分怎么连接

### 4.1 一次运行的调用链

1. 前端 bin / 游戏插件构造 `Config`（serde，或解析 urafile）
2. `Runner::run(config, &mut frontend)` 初始化：RNG 三流 + `SimulationState` + `ScenarioMeta` + `lifecycle.before_simulation`
3. 循环 `turn = 1 ..= meta.turn`（`BasicGame`）：`lifecycle.before_action` → `Trainer` 决策（调搜索框架，含 `FlatSearchGame` 候选、`Evaluator`/`RolloutBackend` 评估、`ScenarioRules` 钩子参与计算）→ 执行动作更新状态 → 产出事件 → `lifecycle.after_action` / `on_turn_end`
4. 结束：`lifecycle.after_simulation` → `Summary`（评分/分值/记录）→ 前端 / 回传游戏

> 回合数（2026-10-14 定）：**共 78 回合，内部 0–77**（`BasicGame::max_turn() = 77`；游戏中显示为第 1–78 回合）。报告中「78」为回合总数、早期方案「77」为 0-based 末回合索引，两者不矛盾。

### 4.2 装配关系（关键：引擎不「发现」剧本）

引擎只持有 `dyn ScenarioRules` / `dyn ScenarioLifecycle` / `G: FlatSearchGame` 等抽象；具体实现由最外层入口注入：

```rust
let cfg     = Config::from_urafile(bytes)?;   // 或 serde 加载
let rules   = MyScenarioRules::new();
let life    = MyLifecycle::new();
let trainer = MyTrainer::new(MyEvaluator::load()?);
let mut fe  = MyFrontend::new();              // print / 手机 / DLL 各自实现
let summary = Runner::new(rules, life, trainer).run(cfg, &mut fe)?;
```

### 4.3 数据怎么流

| 流 | 内容 | 载体 |
|---|---|---|
| 游戏 → 引擎 | 初始数据、协议、种子 | urafile / JSON / Config |
| 引擎内部 | 状态、候选、评分 | Rust 类型 + RNG |
| 引擎 → 前端 | 决策、记录、状态变化、错误 | `Frontend` trait 回调 + 可选 JSON 行流 |
| 引擎 → 存档 | 回放、记录、图 | record / zip / plot（路径由上层注入） |

### 4.4 两种对接模式并存

- **进程内 trait 回调**（Rust 前端、DLL）：零序列化开销，类型安全
- **JSON 事件流**（跨语言/跨进程/手机/Web）：每行一个事件，弱耦合、易录制回放

两者由同一套事件类型驱动，前端按能力二选一，**业务逻辑零分叉**。

---

## 5. 怎么连通游戏

### 5.1 通道现状

游戏侧通过 C# 插件与 Rust 程序交互；既有 urafile 通道与 `scenarioId`（旧：12=温泉、14=拉面）。Rust 侧 `protocol/urafile.rs` 负责解析；旧 `protocol/mod.rs` 按 `scenarioId` 分发——本轮改为「**单场景直连 + 元数据自描述**」。

### 5.2 部署形态

| 形态 | 连接方式 | 说明 |
|---|---|---|
| 桌面 print 版 | 子进程 stdin/stdout + urafile/JSON | 现状路径，引擎 → CLI 渲染 |
| 嵌入 DLL 版 | C FFI：导出 `*const c_char` JSON 接口 | 引擎被 C# 直接加载，回调经 JSON 或函数指针 |
| 手机版 | Rust core → mobile（FFI）+ 原生 UI | 只依赖 `umaai` lib + `Frontend`，无终端库 |
| Web 版 | wasm + JS 回调 | 纯计算部分可编译 wasm，事件走 JS |

### 5.3 FFI 边界设计原则（DLL/手机）

- 边界只传 UTF-8 JSON / C 字符串与明确长度的字节缓冲，**不传 Rust 裸指针结构体**
- 提供 4 个稳定符号：`engine_create(config_json)`、`engine_step`/`run`、`engine_poll_event(buf,len)`、`engine_free(ptr)`
- 所有权：谁分配谁释放，统一 `engine_free`，跨语言内存边界唯一
- `panic` 必须在 FFI 边界 `catch_unwind`，错误转 JSON `EngineError` 返回，**绝不让 unwind 跨语言**
- 线程模型固定：单引擎实例单线程调用，或内部加锁，避免 C# 侧重入

### 5.4 与 C# 端对齐的字段清单（协议契约）

- **剧本标识与版本号**（替代硬编码 `scenarioId`，建议字符串 id + schema version）
- 初始状态：角色、五维、体力、干劲、回合数
- 训练/设施定义：id、类型、等级、基础值、参与 NPC
- NPC/支援卡：id、配置、羁绊/友情状态
- 目标/比赛：回合、等级、期望、胜负
- 事件表：触发条件、选项、效果
- RNG 种子来源（是否由游戏注入以保证可复现）
- 回传内容：决策、记录、评分、错误的 schema

> 建议协议自带 `schemaVersion` 与 `featureFlags`，新旧版本可灰度共存。

---

## 6. 兼容性

### 6.1 平台与工具链

- Rust edition 固定（建议 2021，稳定后评估 2024）；用 stable 工具链，CI 钉死 `rust-toolchain.toml`
- 目标平台：Windows（主要，含 msvc）、Linux、macOS；手机（iOS/Android via FFI）、wasm 作为后续
- 平台相关代码隔离：windows ico、release-pause、`crossterm` 全部收敛在 `umaai-cli`，核心跨平台**零 `cfg(windows)`**

### 6.2 与上游版本的兼容

上游为 Kotlin 且持续更新。本仓不二进制依赖、不源码拷贝，仅作算法与机制参照（见 §1.3）。

### 6.3 API 向后兼容规则（SemVer）

- 核心 crate 严格语义化版本 `MAJOR.MINOR.PATCH`
- 公开枚举一律 `#[non_exhaustive]`；结构体新增字段给 `#[serde(default)]`
- trait 新方法**必须带默认实现**才能在非 major 版本增加
- `umaai-core` 与 `umaai` 版本解耦：新仓锁核心版本，可滞后引擎升级

### 6.4 序列化兼容

- 跨边界统一 JSON（serde），字段 `snake_case`，保留 `#[serde(rename_all)]` 一致策略
- 读写都容忍未知字段（对外协议**不用** `deny_unknown_fields`）
- golden 基线文件即协议样例，兼作兼容性回归

---

## 7. 多语言

### 7.1 人类语言国际化（中 / 日 / 英）

- **引擎不产生人读文案**：只输出稳定的「消息键 + 参数」，如 `event.training.success { level: 3 }`
- 文案表外置：`locales/{zh-CN,ja-JP,en}.json`，前端按语言查表；缺键回退英文，再回退消息键本身（永不空白）
- 统一 UTF-8：文件读写、FFI、stdout 全部 UTF-8；Windows 控制台代码页由 cli 层处理，核心不感知
- 语序差异：用具名占位模板 `{name}`，不做字符串拼接
- 复数/性别/敬语：预留 plural 规则与上下文参数；角色名建立术语对照
- 数值/日期格式化交给前端；字体与宽度（全角/半角）仅 cli 表格层处理

### 7.2 编程语言互操作（Rust ↔ C# / Kotlin / Swift / JS）

- 边界只用 C ABI + UTF-8 JSON（见 §5.3），不暴露 Rust 名修饰、trait 对象、泛型单态化类型
- C#：`[DllImport]` + `MarshalAs(LPUTF8Str)`，或走子进程 stdio；封装安全的托管包装层
- Swift / Kotlin：各自 FFI（cdylib）或 wasm，同样只消费 JSON
- 版本协商：握手交换 `schemaVersion` 与 `featureFlags`，能力不匹配降级为最小公共集
- 事件 schema 语言中立：统一 `{"type":"...", ...}`

---

## 8. 数据与配置驱动

### 8.1 配置分层

- **通用配置**：回合、RNG 种子、搜索深度/宽度、输出选项
- **剧本配置**：训练表、事件、NPC、配方、专属机制开关——放剧本仓 `data/*.json`
- `Config` 可被 serde 直接构造，也可由 urafile 映射，CLI 参数只是它的一个来源

### 8.2 表驱动的边界

- **适合进表**：数值、加成、权重、事件、配方、NPC 出现表、等级提升回合
- **适合写 Rust**：动作空间与阶段映射、复杂状态转移、类型安全机制、热点性能路径
- 原则：**能用表不写码；必须写码则收敛在剧本层，绝不回流引擎**

### 8.3 校验

启动时做 schema 校验 + 语义校验（id 引用完整性、数值范围、回合覆盖）；失败定位到「文件 + 条目 + 字段」；表错误不允许静默成错误结果。

---

## 9. 后期如何加入剧本逻辑与机制

### 9.1 接入步骤（28 日后，填空式）

1. 用 `umaai-core` 的模板新建剧本仓（自带目录与空 impl）
2. 填 `ScenarioMeta`（回合数等）
3. 导入训练/事件/NPC 数值表 `data/*.json`
4. impl `TrainingValueSource`（替代旧的硬编码数值表依赖）
5. impl `FlatSearchGame` + `ActionIndex`：动作空间、阶段映射、动作 → 索引
6. impl `ScenarioRules`：只 override 需要的钩子，其余走默认空实现
7. impl `ScenarioLifecycle`：局初始化、回合末结算等
8. 选/写一个 `Trainer`（手写 / MCTS / NN），必要时给 `Evaluator`、`RolloutBackend`
9. 写 `Frontend` 或复用 CLI / JSON 事件流
10. 入口装调 `Runner::run`；补 golden 基线与集成测试

### 9.2 剧本接入清单（Definition of Done）

- 引擎与核心 crate **零改动**即可编译该剧本（核心 `git diff` 为空）
- 全部跨边界数据走契约类型 / JSON，无新增裸依赖
- 固定种子可复现，golden 基线落库并通过
- 无人读文案写死在引擎，文案走 i18n 键
- CLI 无剧本名分支
- 文档：机制说明 + 数值表来源 + 协议字段对照齐全

### 9.3 新增「机制」而不只是数值

1. 先判断能否用现有钩子 + 表组合实现（优先）
2. 确需新钩子 → 在核心 crate **新增带默认实现的方法**（非破坏性，MINOR 版本）
3. 引擎在主循环固定埋点调用该钩子（默认空），旧剧本不受影响
4. 新剧本 override；补 golden 与文档

### 9.4 默认空实现的重要性

所有钩子默认返回「无效果 / 中性值」，新剧本只写自己关心的少数方法。这是这套契约能支撑多剧本的关键设计纪律。

---

## 10. 手写逻辑新剧本是否需要重头迭代

**结论：不需要从零开始。** 地基、随机、状态机、搜索、通道、测试链路全部复用；真正新写的是「剧本规则 + 策略」，需要反复迭代的主要是**策略质量调参**。

### 10.1 复用 / 新写矩阵

| 模块 | 复用 | 新剧本要做的 |
|---|---|---|
| RNG 三流 | 复用 | 无 |
| 状态机 / 回合骨架 / 枚举 | 复用 | 仅声明 meta（回合数等） |
| 搜索框架 `flat_search` | 复用 | impl 自己的动作 / 阶段映射 |
| `Evaluator` / rollout 抽象 | 复用 | 提供评估实现或动作索引 |
| Runner / 协议 / record / luck | 复用 | 提供协议字段映射 |
| Frontend / JSON 事件 / i18n | 复用 | 文案表 + 标签 |
| golden / bench / tools | 复用 | 录自己的基线 |
| 剧本 State | 新写 | 专属状态结构 |
| 数值表 / 事件表 | 新写 | 从游戏资料整理进 JSON |
| `ScenarioRules` / Lifecycle | 新写 | 只填相关钩子 |
| 手写 `Trainer`（决策策略） | 新写 | 需要调参迭代 |

### 10.2 哪些东西真的要「迭代」

- **手写策略的决策质量**：规则写法、权重、搜索预算需要多轮 rollout 对比——迭代重点
- **数值口径对齐**：与游戏实际行为对表，靠 golden 收敛
- 引擎行为靠基线锁死，不在新剧本迭代中反复变动

### 10.3 防止「假复用、真重写」

若新玩法实体 / 回合结构与现有世界观差异过大，说明基座本质是「**养成类剧本框架**」，应承认边界，而非硬改引擎迁就。判定方法：28 日资料一到，先纸面走「一局完整生命周期」（实体 → 每回合动作 → 状态流转 → 玩家决策 → 评分 → 跨局继承）。哪一步现有枚举表达不了，即标记需泛化或判定超界。**结论要早下。**

### 10.4 手写 vs NN 策略的迭代差异

- **手写**：可解释、起步快、无训练数据要求；调参靠规则与 golden，适合冷启动
- **NN**：需样本与训练管线（旧 `ramen_nn` 已删，需重建），上限高但前期重
- 建议路径：手写策略冷启动 → 沉淀 rollout / 对局样本 → 再评估是否上 NN；二者共用同一 `Trainer` seam，可平滑替换

---

## 11. 怎么维护

### 11.1 分支与提交

- 所有重建提交只落 `miko_next`，不碰 `master` / 备份分支；分阶段小步提交，每步 `cargo check` 保绿
- 分支角色：`master` = 旧实现对照；`miko_next` = 重建；各 `*_backup` = 只读备份
- 评审用 `git diff master --stat`；旧实现参照用 `git worktree` 在 workspace 外只读检出（**勿在其中构建**）
- 远端已确认存在 `ramen_backup`（与 `master` 同 commit），本地未拉，需要时 `git fetch`

### 11.2 CI 与质量门禁

最小流水线：`fmt --check`、`clippy -D warnings`、`check`（各平台）、`test --lib`、golden 回归；性能敏感路径加基准，PR 展示关键基准变化；覆盖率不卡线，但核心（RNG/规则/搜索）必须有用例。

### 11.3 文档维护

单一事实源：架构、契约、机制、术语各一份，废弃文档及时归档（`archive/`）；契约每次改动同步更新附录 B 与变更日志；新剧本仓自带 README + 机制说明 + 数值表来源 + 协议对照。

### 11.4 依赖与版本维护

定期 `cargo update`（小版本）+ 安全公告；大版本升级走独立分支；重依赖新增需评审「能否只放 cli/tools」；核心 crate 发布走 crates.io 或私有 registry，供新仓锁版本。

### 11.5 环境注意（来自现状核实）

- 本机 cargo 需 `dangerouslyDisableSandbox: true`（沙箱拦截 `D:\Rust\.cargo` 全局缓存）
- `cargo test -p umasim` 整包会退 101（`bin/policy_pair_bench.rs` 硬依赖 `experiments/validated_policy/final-check.json`）；过渡期用 `--lib`，P6 修路径依赖后恢复全包

---

## 12. 出 Bug 怎么修

### 12.1 标准定位流程

1. **复现**：拿到输入（urafile/Config）+ RNG 种子 + 期望 vs 实际
2. **分层归类**：协议解析 / RNG / 规则计算 / 搜索决策 / 呈现 / FFI 序列化
3. **收窄**：固定种子重放（`luck_replay` 等），定位首个偏离的回合 / 动作
4. **对照旧实现**：以 `master` / 参照树行为为基准，确认是回归还是设计差异
5. **最小修复 + 回归用例**：先补能复现该 bug 的 golden / 单测，再改代码

### 12.2 各类故障的抓手

| 现象 | 优先怀疑 | 工具 / 做法 |
|---|---|---|
| 结果不可复现 | RNG 种子 / 流被改动、共享可变状态 | 固定种子重放；查 rng 三流 clone |
| 分值整体偏移 | 数值表 / 口径、serde 默认值 | golden 逐回合比对；查表校验 |
| 偶发崩溃 | 越界、unwrap、整数溢出 | 收紧索引；用 `ActionIndex`；回放边界数据 |
| 决策异常 | 搜索候选 / 阶段映射、评估器 | `turn_inspect` 探针；打印候选（testkit） |
| 连接游戏出错 | 协议字段、UTF-8、内存所有权 | 抓原始 JSON；schema 校验；FFI 日志 |
| 仅 Windows 异常 | 控制台代码页 / 路径 / ico | 问题收敛 cli；核心先在 Linux 验证 |
| 性能回退 | 搜索预算、批量后端 | criterion 对比；二分提交找回归点 |

### 12.3 二分与回放

行为回归：`git bisect` 找引入提交；状态回归：golden 快照逐回合 diff，第一个不同的回合即故障现场；游戏侧：先用真实输入本机离线重放，避免游戏内反复试。

### 12.4 FFI / 跨语言特殊处理

崩溃在游戏内而 Rust 侧不复现：优先查内存（谁分配谁释放）、线程重入、panic 未捕获；全部 FFI 入口保证不 unwind，错误经 `EngineError` JSON 回传；加一层边界日志（截断长度、版本握手）。

---

## 13. 现状核实与 P0 实测 ★

### 13.1 分支状态

- 当前分支 `miko_next`，与 `master` 零差异（0/0），worktree 干净——重建在此分支原地进行
- 参照树：`git worktree add F:/UmaAI_Active/umaai-rs-ref master`（只读，勿在其中构建）
- 旧实现备份：`master`、`ramen_workbench`、`onsen_backup`、`ramen_workbench_alter` 等；远端另有 `ramen_backup`

### 13.2 规模

| 位置 | 规模 / 说明 |
|---|---|
| `crates/umasim/src/game/ramen/` | 12 文件 / 约 15.1k 行 |
| `crates/umasim/src/trainer/` | 14 文件 / 约 10.0k 行 |
| `crates/umasim/tools/data_collection/` | 11 文件 / 约 10.4k 行 |
| `crates/umaai_review/src/` | 16 文件 / 约 7.2k 行 |
| `crates/umaai/src/main.rs` | 406 行（引擎驱动仍在 bin） |
| `crates/umaai/src/lib.rs` | 6 行（仅 `pub mod`，lib 为空壳） |
| `crates/umaai/src/protocol/mod.rs` | 888 行（含末尾测试；已核实为**解析 + 测试**，非「解析+呈现」混杂） |
| 内联测试 | 103 个 `#[cfg(test)]` + 80 个 `mod tests`（合计 183）/ 82 文件；无独立 `tests/` |

> 口径注记（2026-10-14 复核）：本表行数为**非空行**（`wc -l` 减空行）。复现：`game/ramen/` 15100、`trainer/` 9981、`data_collection/` 10446、`umaai_review/src/` 7206（测量 commit `6f706ac`）。`main.rs` 行数与「内联测试」计数已按当前代码订正。

### 13.3 耦合形态

「**通用的壳、剧本的芯**」——通用模块内嵌剧本分支，详见附录 A。

---

## 14. 风险与对策

| # | 风险 | 触发场景 | 对策 |
|---|---|---|---|
| 1 | 依赖闭包爆炸 | keep 文件牵连剧本类型，P2 大面积返工 | P0 已量化闭包到「文件+符号+行号」（附录 A），形成任务表再动手 |
| 2 | 多仓注册矛盾 | 外部新仓无法被枚举 | 核心只用 trait，剧本自 impl + 入口自组装；引擎不持剧本清单 |
| 3 | **上游合规** | 误拷源码 / 照搬架构 / 协议传染 | 只重写算法不拷码；不照搬枚举注册等架构；保留出处 license |
| 4 | `basic.rs` 命门 | `TrainingValueSource` 设计偏则全歪 | P2 先出该抽象的小设计评审再落码 |
| 5 | NN 评估器非通用 | 硬编码 `OnsenAction` 动作索引 | 抽到 `ActionIndex` trait；剧本给映射，否则降为剧本 impl |
| 6 | 呈现层下压 | sink 在 umasim 拖下游 | sink 上移 `umaai`/`umaai-cli`，umasim 只留 `DecisionSink` trait |
| 7 | 通用性无对照 | 重写的壳缺测试 | 优先搬 RNG/inherit/simulator 等不依赖剧本的通用测试 |
| 8 | 历史断裂 | 新码与旧实现无连续对照 | `master` / 备份保留行为基准，重写以旧实现为准 |
| 9 | 协议未定稿返工 | 28 日前臆造字段 | 只定 trait 与 `schemaVersion`，字段留适配层 |
| 10 | 误动旧分支 | 提交落错分支 | 提交仅落 `miko_next`，分支保护与评审 |
| 11 | 测试退 101 | 硬编码相对路径 | P6 消除路径隐式依赖，路径由上层注入 |
| 12 | i18n / 编码问题 | Windows 代码页、语序、缺译 | 引擎只出键；UTF-8；缺键回退；表格层处理宽度 |

---

## 15. 路线图

### 15.1 现在 → 10-28（只做与剧本无关的地基）

| 阶段 | 交付 |
|---|---|
| **P0** | 冻结基线、参照树、依赖闭包量化、harvest 有用资产（数值表/公式/夹具/golden） |
| **P0.5** ✅ | 已完成：游戏机制拆为 `game_rules`（规则）+ `game_implementation`（实现细节），术语表（`glossary` / `game_glossary`）刷新，一并迁入 `.trae/documents/game/`；缺口核对表已归档（`archive/game_mechanics_gap_review.md`） |
| **P1** | 搭骨架：核心/计算/引擎/cli/tools + 各 seam 签名（含默认空实现） |
| **P2** | umasim 规则基座：rng/uma/support_card/inherit/base；`TrainingValueSource`、`ScenarioRules` 空默认 |
| **P3** | 搜索框架：批量 rollout trait 化、去 `FROZEN_DIM_KEYS`；定 `ActionIndex` / neural 取舍 |
| **P4** | 输出 + 在线记录通用化；`cargo check -p umasim` 全绿 |
| **P5** | 通道层：urafile/decision/plot 进引擎 lib；呈现整体入 cli；单场景直连；review 移出 members |
| **P6** | `umaai-tools`：探针/bench/golden + `--update`；修路径依赖；预留「策略×世界」接口（不预建 PBT/粒子滤波） |
| **P7** | 收尾：`BasicGame` 跑满回合冒烟 + 新剧本接入 seam 说明 + **空剧本模板** |

### 15.2 10-28 之后（拿到剧本资料）

纸面走完整生命周期，判定是否在「养成类框架」边界内 → 与 C# 端冻结协议字段与 `schemaVersion` → 基于空模板新建剧本仓，按 §9.1 十步填充 → 手写策略冷启动 + golden 调参；沉淀样本后再评估 NN → 上线灰度，事件流录制护航。

---

## 16. 验收标准

- workspace 仅含核心/计算/引擎/cli/tools；`umaai_review` 已 exclude
- 核心与引擎 `cargo check` / `test --lib` 全绿，`clippy` 无 warning
- lib 中零终端依赖，终端依赖只在 cli，dev 重依赖只在 tools
- `BasicGame` 固定种子可复现并跑完全部回合
- golden 基线可回归、`--update` 可回写
- 核心枚举 `non_exhaustive`、serde 默认值齐全，新增钩子均带默认实现
- 空剧本模板可在不改引擎前提下 `cargo check` 通过
- 游戏通道（urafile/FFI/JSON）冒烟通过，版本握手机制可用
- 文档：架构 / 机制 / 契约钩子 / 术语 / 维护与排障指南齐备

---

## 17. 开放项

1. ~~**旧实现对照源**~~（已定）：以 `master` 为对照，建只读 worktree 参照树；实际备份分支为 `ramen_workbench`/`onsen_backup`/`ramen_backup`
2. **新剧本 seam 形态**：本轮只留空 trait 接口，等新协议（C# 端）定稿再填
3. **`luck_record` 归属**：luck 属在线记录链路，但其配置与剧本段混写，P0 一并判定
4. **`umaai-tools` 命名与 testkit 暴露方式**：默认 `umaai-tools`
5. **`neural_net_evaluator` 取舍**：非通用（硬编码 `OnsenAction` 索引）→ 抽 `ActionIndex`，否则降为剧本 impl
6. **sink 上移方案**：`HumanReadableSink`/`StdoutJsonSink` 位于 `umasim/output/sink.rs` → 上移 `umaai` 还是 `umaai-cli`
7. ~~**回合数口径**~~（已定）：共 78 回合（内部 0–77，`max_turn() = 77`）；「78」= 回合总数、「77」= 0-based 末回合索引

---

## 附录 A：P0 依赖闭包结果（实测）

**基线**：`cargo check --workspace` 通过（exit 0）。仅 3 个 warning（`luck_probe.rs:30`、`rest_pair_probe.rs:14`、`rest_pair_probe.rs:93`），均在 DROP 文件。

**计数口径**：模式 `Ramen|Onsen|ONSENDATA|RAMENDATA|FROZEN_DIM_KEYS|scenario_buff|RamenState|RamenStatus`。`umasim/src` 共 2640 处 / 64 文件；`umaai/src` 共 388 处 / 18 文件。

### A.1 可直接保留（CLEAN，零剧本引用）

`game/inherit.rs`、`game/uma.rs`、`game/support_card.rs`、`game/simulator.rs`、`game/mod.rs`、`game/base/{person,action}.rs`、`neural/{evaluator,value_output}.rs`、`output/diagnostic.rs`、`umaai/decision/{luck_score,zip_export}.rs`、`umaai/plot/{mod,svg}.rs`、`umaai/protocol/urafile.rs`

> `rng.rs` 唯一命中（`rng.rs:75`）是**文档注释**，非代码 → 实际 CLEAN。

### A.2 重写热点（按纠缠度，附行号）

| 文件 | 引用数 | 主要符号 / 行号 | 处置 |
|---|---|---|---|
| `search/flat_search.rs` | 163 | `FlatSearch<G: FlatSearchGame = OnsenGame>`（L155）；`batch_rollout: Option<Arc<dyn RamenBatchRollout>>`（L202）；`RamenBatchTable`（L281）；`trait RamenBatchRollout`（L287）；`impl FlatSearch<OnsenGame>`（L860） | 泛型默认改中性；batch trait 去 Ramen 命名 |
| `output/turn_flow.rs` | 43 | `RecordingTrainer<T: Trainer<RamenGame>>`（L171）；`RamenStage::{Train,RamenSelect}`（L104/111/182）；读 `RAMENDATA` 取配方（L116） | 泛化到 `Game` trait；候选渲染移入 seam |
| `search/searchable.rs` | 40 | `impl FlatSearchGame for OnsenGame`（L96）、`for RamenGame`（L127） | **删两个 impl，保留 trait** |
| `gamedata/config.rs` | 29 | `scenario: String`（L587）；`onsen_order`（L610）；`ramen_region_strategy`（L637）；`ramen_trainer_policy`（L645）；枚举（L833/L851） | 拆通用段 / 剧本段 |
| `umaai/protocol/mod.rs` | 29 | `ParsedGame::{Onsen,Ramen}`（L425-435）；`extract_scenario_id`（L441）；`parse_game_by_scenario`（L456，id 12/14） | 改单场景直连 |
| `neural/neural_net_evaluator.rs` | 26 | `action_to_global_index_v1(&OnsenAction)`（L89-101）；`impl Evaluator<OnsenGame>`（L454） | **非通用** → 抽 `ActionIndex` |
| `output/reason.rs` | 13 | 生产类型钉死 `RamenSearchOutput`/`RamenTerminalStats`（L30/266/331） | 泛化 terminal 类型 |
| `game/base/mod.rs` | 13 | 注释（L51/64/149）+ 测试（L315-316、L666-690） | 清注释、改测试 |
| `search/mod.rs` | 8 | 再导出 `RamenBatchRollout/RamenBatchTable`（L23）、`FROZEN_DIM_KEYS/RamenTerminal*`（L24）、`RamenSearchOutput`（L32） | 去剧本导出 |
| `search/result.rs` | 7 | `SearchOutput<A = OnsenAction, D = NoTerminalStats>`（L199）；`impl SearchOutput<OnsenAction>` + `export_sample(&OnsenGame)`（L334/345） | 默认泛型改中性；export 泛化 |
| `umasim/utils.rs` | 5 | `OnsenOrder`（L23）；`validate` 校验 `scenario ∈ {basic,onsen,ramen}`（L474-476）；`RamenRegionStrategy::Fixed`（L487） | 去剧本校验 |
| `game/traits.rs` | 4 | `is_scenario_card`（L52）；`record_absent_person` 默认空、拉面 override（L230）；view 的 `scenario` 字段（L532-541） | trait 化 |
| `output/{decision,decision_log,sink}.rs` | 3 / 3 / 2 | 基本是 `scenario_extra` 通用钩子 + 注释 | 轻微改名 |
| `game/base/basic.rs` | 2 | `ONSENDATA` 导入（L26-31）、`training_basic_value`（L611-613） | `TrainingValueSource` |
| `gamedata/event.rs` | 2 | 低，待细化 | — |

### A.3 已核实结论

- `protocol/mod.rs` 非「解析 + 呈现混杂」：`println!` 全在末尾 `#[cfg(test)]`，无 `colored`/`comfy_table` 依赖
- **呈现层确实在 `umasim`**：`output/sink.rs` 定义 `DecisionSink`、`HumanReadableSink`（L88）、`StdoutJsonSink`（L195），经 `output/mod.rs:25` 再导出，由 `umaai/src/main.rs:25` 引用
- `umasim` 自身是 bin+lib：`#![cfg(feature = "cli")]` 门控 `main.rs`（L8）、`gamedata/mod.rs`、`utils.rs`

### A.4 剩余待办

- `gamedata/event.rs`、`output/{decision,decision_log,view}.rs` 细目（低耦合）
- `output/reason_color_check.rs`（测试支撑文件）归类
- `neural_net_evaluator` 取舍、sink 上移方案（见 §17）

---

## 附录 B：机制覆盖清单（借鉴思想，非接口照搬）★

> 本清单来自上游同类实现的**机制覆盖面**备忘，**不作为本仓接口定义**。trait 命名、切分与签名由本仓按自身需求在 P1 定稿；能由「表 + 现有钩子」表达的，优先不加钩子。

**计算类机制（拟归入 `ScenarioRules`）**

- 剧本训练加成、比赛加成、比赛末附加加成、比赛附带效果
- 失败率下降、体力消耗下降、得意率提升、出场率提升、启发频率提升、全体支援启发
- 提供 / 改写训练项、额外参与人数、训练关系加成、强制启发数、洗牌修正
- 休息预测、剧本专属动作与参数预测、普通比赛封锁、回合更新、加状态联动
- 基础值改写（`modify_base_status` 类）

**生命周期机制（拟归入 `ScenarioLifecycle`）**

- 局开始前 / 初始状态 / 动作前 / 预测前 / 动作后 / 回合末 / 局结束后（7 个埋点）

**元数据（拟归入 `ScenarioMeta`）**

- 剧本 id / 显示名（走 i18n 键）、训练数据引用、回合数、等级提升回合、训练自动升级、是否有嘉宾、是否有第二训练状态、联动角色

---

## 附录 C：术语表

> 完整术语表已拆分为两份：[glossary.md](game/glossary.md)（项目术语）+ [game_glossary.md](game/game_glossary.md)（游戏术语）；通用机制见 [game_rules.md](game/game_rules.md) / [game_implementation.md](game/game_implementation.md)。下表仅为本方案高频词速查。

| 术语 | 含义 |
|---|---|
| 引擎（`umaai`） | 主循环、协议、决策编排与记录；不做呈现 |
| 计算层（`umasim`） | 规则基座、搜索、策略与评估的纯计算 |
| 核心层（`umaai-core`） | 引擎与剧本之间零重依赖的 trait + 基础类型 + RNG |
| 剧本（scenario） | 插在契约上的具体玩法实现 |
| seam | 可替换 / 可插拔的接缝点（trait 或泛型边界） |
| RNG 三流 | 核心随机体系，按用途分流，保证可复现 |
| `Frontend` | 输出回调 trait：print / 手机 / DLL / Web 各自实现 |
| golden 基线 | 固定种子下的期望结果文件，用于回归与调参 |
| testkit | `#[doc(hidden)]` 的内部测试访问模块 |
| urafile | 游戏 C# 插件与 Rust 之间的数据通道 / 文件格式 |
| `ActionIndex` | 剧本动作到全局索引的映射，替代硬编码动作空间 |
| `BasicGame` | 无剧本的基座局，用于引擎冒烟与默认构建 |

---

*状态：已并入《技术方案报告 V1.0》；P0 实测结果在附录 A。待定：`neural_net_evaluator` 取舍、sink 上移方案、协议字段冻结（28 日后）。*