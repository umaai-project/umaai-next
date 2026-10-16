# miko_next 重建方案 v2（独立新仓 + 选择性移植）

> 本文取代 v1 方案（`umaai-rs/.trae/documents/miko_next_rebuild_plan.md`）。v1 作为机制清单与附录参考随仓保留于
> [`docs/plan/v1_plan_reference.md`](v1_plan_reference.md)。
> 本文记录 2026-10-14 定稿决策、2026-10-15 的层界修订、2026-10-16 的独立仓改判与 2026-10-17 的 P4′ 执行定稿（见 §9）；未决项集中在 [`docs/notes/open_items.md`](../notes/open_items.md)。

---

## 0. 决策基线（已拍板）

| # | 决策 |
|---|---|
| 1 | 另起**空仓** `miko_next`，从零初始化；**不在旧仓原地重构** |
| 2 | 旧仓 `umaai-rs`（含其 `miko_next` 分支）**暂不动**；`umaai-rs-ref`（master 只读工作树）作移植源与行为基准 |
| 3 | 独立仓 **`umaai-next`**（本地目录暂用 `miko_next/`），**独立全新 git 历史**；**2026-10-16 改判**：不再预设并入 `umaai-rs`，收口方式推迟到 P7′ 验收后按「新引擎是否已能替代旧实现」再定 |
| 4 | 移植方式：**混合** —— 通用件**复制**、关键件**重写** |
| 5 | 核心层命名 **`umaai-core`**；零重依赖件入核心层（RNG 三流 / 统一错误 / 纯函数工具） |
| 6 | 文档目录 **`docs/`**（工具无关）；**skills 暂不移植**，不建 `.trae/`；`AGENTS.md` 留仓库根 |
| 7 | 数据：**单一数据根** `gamedata/`（平铺 + `manifest.json` 校验入口）；剧本数据外置；加载收敛到单一 `data` 模块；**路径由宿主注入** |
| 8 | `Data` 单例：**值类型 + `OnceLock` 全局便捷**（`init` / `get`）；**不做热更新**；`init` 幂等 |
| 9 | 测试：单元 `assert`、集成 print+Agent、baseline 自动判定；跨 crate 集成测试放**专用 crate `umaai-tests`** |
| 10 | 更新器：放弃独立 GUI 形态，改由入口层（`umaai-cli`）在启动时提示 / 子命令更新数据；**不做进程内热更**；细节待定 |
| 11 | 算法探索 workbench（GA / NN）：**待定**（倾向独立项目） |
| 12 | 工具链统一 **stable**（钉死版本）；rustfmt 只用稳定选项，放弃 nightly 专属风格 |
| 13 | **层界（2026-10-15 定）**：`umaai-core` 只放**纯 schema/DTO + 数据加载 + 零依赖纯函数工具 + RNG + 统一错误**；**带行为的领域类型、全部契约 trait、领域算法归 `umasim`** |
| 14 | **归属判据**：进 core 的门槛是「**跨算法复用且与游戏、与算法无关**」；「零依赖」只是必要条件，不构成理由 |
| 15 | 核心层依赖口径：**轻量、跨平台、通用**的库（`serde` / `serde_json` / `thiserror` / `rand` 0.9 / `int-enum` / `log` 门面）；禁终端 / 网络 / 计算重依赖与平台相关件。呈现（着色 / 表格 / 交互）只在 `umaai-cli`；诊断与日志走核心层 `diag` 机制 + `log` 门面 |
| 16 | **剧本仓形态（2026-10-17 拍板）**：剧本＝**独立仓**，以依赖引用本仓 `umaai-core` / `umasim` / `umaai`（开发期 `path`、稳定期 git `rev`）；**不 fork 本仓、不在本仓加剧本 crate**；剧本专属类型 / 规则 / 策略 / 协议段 / 配置 / 数据全在剧本仓；**剧本仓自带 bin**（入口装配点），本仓不反向依赖外仓 |

---

## 1. 目标

把项目重构成「**通用引擎 + 可插拔剧本**」：引擎不认识任何具体剧本，剧本插在一组 trait 上；
下游新剧本放**独立仓库**，只依赖零重依赖的契约面（trait + 基础类型），在自身入口完成装配。

- 引擎与剧本彻底解耦，默认只含无剧本的 `BasicGame`（归 `umasim`）
- lib 化：给手机版 / DLL 版 / Web 等下游提供稳定公共 API
- 引擎通过 `Frontend` 回调输出，**自己绝不打印**
- 固化基线，支撑调参与回归
- 为新剧本提供「填空式」接入路径，不碰引擎

---

## 2. 仓库与目录

仓名 **`umaai-next`**（与 `umaai-rs` 并列的独立仓，见 §0 决策 3）；本地目录暂用 `miko_next/`，下树按本地目录名列出。

```
miko_next/
├─ Cargo.toml                 # workspace（6 members，resolver = 3）
├─ rust-toolchain.toml        # stable（钉死版本，如 1.99.0）
├─ rustfmt.toml               # 稳定工具链可用的格式规则
├─ .gitignore
├─ .github/workflows/ci.yml   # fmt / clippy / test 门控
├─ AGENTS.md
├─ crates/
│  ├─ umaai-core/   # 核心层：纯 schema/DTO + 数据加载 + 纯函数工具 + RNG 三流 + 统一错误
│  ├─ umasim/       # 计算层：带行为的领域类型 / 契约 trait / 搜索与策略算法
│  ├─ umaai/        # 引擎 lib：Runner / 协议 / Config / 决策编排 / record / luck
│  ├─ umaai-cli/    # print 前端 + 入口层功能（更新等）
│  ├─ umaai-tools/  # 探针 / 基准（publish = false）
│  └─ umaai-tests/  # 集成 / 端到端 / baseline（publish = false）
├─ docs/
│  ├─ plan/         # 方案（含 v1 参考）
│  ├─ game/         # 游戏机制：规则 / 实现细节 / 术语 / 固有效果
│  ├─ notes/        # 开发笔记、待办
│  ├─ changelog.md
│  └─ issues.md
└─ gamedata/        # 单一数据根（只读输入；剧本数据不进本仓）
```

### 剧本仓形态（外仓，2026-10-17 定）

- **形态**：独立仓，不在本 workspace；只依赖 `umaai-core` / `umasim` / `umaai`（`umaai-tests` / `umaai-tools` 为 `publish = false`，外仓拿不到）。
- **内容**：剧本领域类型（`Game` / `ActionEnum` / `Person`）、剧本专属协议段解析、persons 构造与阶段自持（内部强类型枚举 + 中间状态在局面里，对外只投影 `StageId`）、决策（`Trainer` 或 `FlatSearchGame`）、剧本专属配置段与数据、测试夹具（真机帧）。
- **依赖方式**：开发期 `path`（并列目录），稳定期 git `rev` / tag 钉住；若将来走 crates.io 需先确认包名占用。
- **入口**：剧本仓自带 bin（`Session` 帧驱动 + 自实现 `Frontend`），本仓 CLI 不出剧本产物；**2026-10-17 定稿**：`umaai-cli` 已拆为可依赖的 lib + 瘦 bin，剧本仓 bin 可直接复用其呈现件（`PrintFrontend` / `render` 原语），pub 面不承诺稳定、按 git `rev` 钉住。
- **纪律**：接入新剧本时本仓核心 `git diff` 为空；确需新能力时回本仓按「带默认实现、非破坏」加钩子，禁止在剧本仓 fork 引擎。

---

## 3. crate 骨架与依赖铁律

- **单向依赖**：`umaai-cli` → `umaai` → `umasim` → `umaai-core`；`umaai-tools` / `umaai-tests` 依赖前三者，不进发布
- **核心零重依赖**：`umaai-core` 只允许 `serde`（derive）/ `serde_json` / `thiserror` / `rand` / `int-enum` 这类轻量跨平台库
- **终端依赖只在 `umaai-cli`**：着色 / 表格 / 进度 / 交互 / 日志
- **dev 重依赖只在 `umaai-tools`**：criterion / ndarray / pprof 等
- **下游新仓只面向契约 trait 编程**，禁止直接引用具体剧本类型

### 层界与归属（2026-10-15 定）

进 `umaai-core` 的门槛：**跨算法复用，且与游戏、与算法无关**（「零依赖」只是必要条件）。

| 归属 | 内容 |
|---|---|
| `umaai-core` | 纯 schema/DTO（gamedata 静态数据类型）、数据加载（`Data` 单例 / 路径注入 / 校验）、零依赖纯函数工具（**通用统计原语**、**CRN 种子表**）、RNG 三流、统一错误、诊断输出（`diag!` 宏 / 运行时开关 / 抑制 guard） |
| `umasim` | 带行为的领域类型（`Uma` / `FriendState` / `BaseGame` / `BasicGame` …）、阶段位置标识（`StageId`，剧本自定数字；基础局阶段机 `BasicStage` 是基础局私有）、全部契约 trait（`Game` / 剧本钩子 / 搜索 / 策略）、领域算法（`flat_search` / `trainer/*` / `neural/*`）、搜索与 NN 私有数据形态 |
| `umaai-cli` | 终端呈现（着色 / 表格 / 日志 / 交互）；`explain` 类方法改为返回数据或下沉本层 |
| 剧本仓 | 剧本专属类型 / 规则 / 策略 |

推论：**类型在 core、trait 在 umasim** 时，`impl Game for <core 类型>` 一律写在 `umasim`（定义 trait 的一侧），依赖方向仍是 `umasim → core` 单向；不同类型混放不会产生反向依赖。

---

## 4. 移植策略（复制 / 重写）

从 `umaai-rs-ref` 按文件复制到新仓，**就地改造**，每步 `cargo check` 保绿、小步提交。

**复制（通用件，零剧本引用）**：`rng.rs`、`game/{inherit,uma,support_card,simulator}`、
`base/{person,action}`、`neural/{evaluator,value_output}`、`output/diagnostic`、
`decision/{luck_score,zip_export}`、`plot/{mod,svg}`、`protocol/urafile.rs`、`gamedata` 静态数据。

> 落层按 §3「层界与归属」判据：`gamedata` 静态数据（纯 schema/DTO）与数据加载入 `umaai-core`；
> `game/*`、`neural/*` 等**带行为的类型**入 `umasim`（其 `explain` 等呈现方法剥离到 `umaai-cli`）。

**重写（关键件，契约 / 边界类）**：`umaai-core` 全部、`game/traits`、`game/base/basic`
（→ `TrainingValueSource`）、`search/*`（去 Ramen/Onsen 命名与默认泛型）、`output/*`
（呈现下沉 cli，计算层只留 trait）、`gamedata/config`（拆通用 / 剧本段）、`protocol`
（单场景直连）、`Runner` / `Frontend` / `Config`、`neural` 的 `ActionIndex` 抽离。

**不移植**：`game/{ramen,onsen}` 全部剧本实现（留 seam + 空模板）。搜索层的**批量 rollout 后端**（跨 rollout 攒批）、**leaf 截断估值**（`max_depth > 0`）与 **NN 样本导出**亦不移植：前两者延后到有真实需求与可验证实现时，后者随神经阶段（见 open_items #12–#14）。

---

## 5. 数据与配置

- **单一数据根** `gamedata/`（平铺）：`cardDB.json` / `umaDB.json` / `events.json` /
  `text_data_dict.json` / `constants.json` / `default_config.toml` + `manifest.json`（清单 + `schema_version`，唯一校验入口）
- **剧本数据外置**：`scenario_*.json` 不进本仓
- **加载收敛到单一 `data` 模块（落在 `umaai-core`）**：
  - 值类型 `Data`（`Data::load(root) -> Result<Data>`，一次载入 + 校验）
  - 全局便捷层 `Data::init(root)`（幂等：已初始化且 root 相同 → `Ok`；root 不同 → `Err`）/ `Data::get() -> &'static Data` / `try_get()`
  - 整表与领域快捷方法（`Data::events()`、`Data::system_event(key)`、`Data::event_prob(key)` …）
  - **不保留旧 `global!` 宏**，统一显式调用
  - **载入不写日志**（`log` 不入 core）：载入统计返回给调用方，由上层打印
- **路径由宿主注入**（方案 A 起步）；内部经极薄访问器取字节，**预留 `DataSource` 形状**（方案 B：目录 / 内嵌 / 内存多源可叠加）
- **配置载入**：默认层 `gamedata/default_config.toml`（唯一真源、字段齐全）+ 宿主注入的可选覆盖层（同构、字段全可选、段缺失整段沿用）；剧本专属配置段不进本仓；比赛等级表等具名预设按名取、缺失即报错
- **不做热更新**：更新 = 换磁盘文件 + 重启生效；`manifest.schema_version` 做数据/程序一致性校验
- **测试防重入**：test-only 持锁安装（`TEST_LOCK` 串行 + `Drop` 清理）；需要不同数据的测试走值类型（绕过全局）

---

## 6. 测试策略

| 层 | 位置 | 判定 |
|---|---|---|
| L1 单元（私有细节 / 纯函数） | `crates/<crate>/src/` 内联 | `assert` |
| L2 模块集成 | `crates/<crate>/tests/` | 结构化 `print` 输出 + Agent 检查 |
| L3 端到端 | `crates/umaai-tests/tests/` | 结构化 `print` 输出 + Agent 检查 |
| L4 baseline | `crates/umaai-tests/tests/baseline.rs` + 数据文件 | **自动判定**（比对后返回 `Err` / 非零退出码）+ `--update` |
| L5 不变量 | 随 L2 | `print` + Agent |

- **不用 `assert` 的层**：统一用 `testkit::Checker`（打印 + 聚合，失败返回 `Err`），保留 `cargo test` 失败信号
- **共享夹具经 `testkit`**（`#[doc(hidden)] pub mod testing`）暴露，禁止 `#[path = "..."]` 跨目录引入
- **不设行数硬限**；靠「内联只留私有细节」+ 评审控制膨胀
- **baseline（替代 golden）**：一基线一 JSON，输入/期望分离，手改即生效，`--update` 重录，带 `schema_version` + 生成元信息 + 每项容差
- **旧库 617 个测试不整体移植**：通用件测试随文件搬迁并 inline→`tests/`，剧本测试弃置，baseline 重录

---

## 7. 更新与分发（细节待定）

- 放弃独立 GUI 更新器形态；改由**入口层**（`umaai-cli`）承担
- 启动时默认**轻量检查 + 提示**（发现新数据 → 询问是否更新），显式 `umaai update` 做真正下载/覆盖
- **数据更新**（本项目 `gamedata`）：下载 → 覆盖 → **提示重启生效**
- **自身更新**：Windows 无法覆盖运行中 exe → 下载新 exe → rename 旧的 → 拉起新的 → 退出
- **可降级**：`--no-update` / 配置关闭 / 短超时 / 失败不阻断启动
- `ura_data`（小黑板数据）是否纳入：**待定**
- **不做进程内热更新**

---

## 8. 算法探索 workbench（待定）

三档归类：

| 档 | 内容 | 归属 |
|---|---|---|
| 引擎开发必需 | 探针 / bench / baseline runner | `umaai-tools`（随仓） |
| 算法探索 | GA 调参、NN 训练/数据管线、策略扫描 | **倾向独立项目/仓**（path 依赖本仓） |
| 运行时策略 | 客户端实际用的手写 / MCTS / NN 决策 | 引擎或剧本仓 |

判断准则：是否进发布产物 / 依赖面（契约 vs testkit）/ 依赖重量 / 迭代独立性。
本仓需为外部 workbench **预留 seam**（`Trainer` / `Evaluator` / `RolloutBackend` / `FlatSearchGame` 可外部 impl）。

---

## 9. 阶段划分

| 阶段 | 交付 | 状态 |
|---|---|---|
| **P0′** | 建仓：目录 + `git init` + workspace + 6 crate 空骨架 + toolchain/fmt/CI + `docs/` + `gamedata/` | ✅ 已完成 |
| **P1′** | `umaai-core`：RNG 三流（逐字移植）+ 统一错误 + gamedata 纯 schema/DTO + 数据加载（`Data` / 路径注入 / 校验）+ 零依赖纯函数工具（通用统计原语、CRN 种子表） | ✅ 已完成（配置层 `default_config.toml` / `GameConfig` 归 P4′；CRN 只落 `seed_at`） |
| **P2′** | `umasim` 基座：带行为的领域类型（uma / support_card / inherit / base，去剧本）+ 契约 trait（`Game` / `Trainer` / `TrainingValueSource`） | ✅ 已完成（剧本钩子 `ScenarioRules` / `ScenarioLifecycle` 经拍板延后到有具体剧本时，见 open_items #10；`simulator` 为旧仓空壳未移植） |
| **P3′** | 搜索框架泛化：flat_search / searchable / result / terminal（复用 core 的统计原语与 CRN 种子表） | ✅ 已完成（搜索评分为单轴口径，PT 偏好折算率归 `SearchConfig`；批量后端 / leaf 截断估值 / NN 样本导出延后，见 open_items #12–#14；`BasicGame` 接参考实现 + 随机训练员） |
| **P4′** | 引擎 `umaai`：`Runner` / `Frontend` / `Config` + 单场景协议 + record / luck | ✅ 已完成（P4a 配置层、P4b 前端契约与离线运行器、P4c 运气分与记录装配、P4d 协议通用段 / 终局帧 / 帧通道 / 帧驱动会话；记录落盘与入口层接线归后续阶段） |
| **P5′** | `umaai-cli`：呈现全入 cli；`BasicGame` 跑满回合冒烟（呈现件是否拆 lib 化以复用给剧本仓 bin，开工时按§2 定） | ✅ 已完成（拆为 lib 呈现件 + 瘦 bin；呈现原语 / 样例前端 / 入口层落地，基础局单局冒烟开箱跑满回合；见下方 P5′ 执行定稿） |
| **P6′** | `umaai-tools`：探针 / bench / baseline runner + `--update` | — |
| **P7′** | 空剧本仓模板（lib + bin，见 §2「剧本仓形态」）+ 文档（机制 / 契约 / 术语）+ 验收 | — |

### P4′ 执行定稿（2026-10-17 拍板）

| 项 | 定稿 |
|---|---|
| 配置形态 | 默认值只存在于数据根 `default_config.toml`（唯一真源，Rust 侧不留同名字段缺省）；覆盖层与默认层同构、字段全可选，段缺失整段沿用；未知键严格报错。比赛等级表用具名预设按名选择（`presets.<kind>.<name>` + 字段填名字），剧本专属配置段不进本仓 |
| 决策上下文捕获 | DTO 归引擎（`umaai::decision`），训练员经可选接缝 `DecisionSource` 提供；不提供则只记动作，基础局照跑。不在计算层加同类 DTO |
| 协议深度 | 完整档：`GameStatus` / `ScenarioProfile` 接缝 + 帧通道 + 通用 `baseGame` 解析 + 终局帧 + 基础局参考实现（放测试支撑）；**具体剧本段解析永远留剧本仓** |
| 通道 | 各剧本统一走 `.portable` 下的回合帧 / 终局帧，路径与文件名作代码常量、保留根目录注入点，不做平台探测；实时监听用文件通知库，**测试必须带显式超时**（见 open_items #16） |
| 记录落盘 | P4 只交付记录数据模型与运气分算法（纯数据、无 IO）；CSV / 归档 / 趋势图等落盘与列契约延后到有真实消费者时（见 open_items #15） |
| 事件接入 | 关键事件按 ID 分族入账（卡事件归模拟段模板、角色与友人事件按真实 ID）对齐已发生历史；帧事件按成功/失败两支重建为待处理事件、决策与结算同源，数值缺失只跳过该事件；**通用成功率属临时口径，待真实剧本校准**（见 open_items #17） |

### P5′ 执行定稿（2026-10-17 拍板）

| 项 | 定稿 |
|---|---|
| 呈现件形态 | `umaai-cli` 拆为 **lib 呈现件 + 瘦 bin**：`lib` 出工具件（`render`）与样例实现（`frontend`），本仓入口逻辑（`cli`）也在库里以便集成测试注入输出；bin 只收参数、报告失败、给退出码。剧本仓 bin 直接复用，**pub 面不承诺稳定**，按 git `rev` 钉住 |
| 定位 | 本 crate 是**控制台 App 的工具件 + 样例实现**：呈现原语与显示宽度/折行处理在这里做一次，剧本仓不必重复踩坑 |
| 分层纪律 | **只有 `render/table.rs` 接触表格库**（换库只改一处）；**只有 `render/term.rs` 读环境与探测终端**；**契约 trait 永不加呈现方法**（旧仓把呈现声明成 trait 方法是问题根源） |
| 表格渲染 | 选表格库（非 TUI 框架）：要的是行 / 列合并、按转义序列计宽、显式宽度策略；宽度由调用方给出，重定向与测试的输出才可复现 |
| 着色 | 三重门禁：编译期特性 `color`（默认开）+ `--no-color` / `NO_COLOR` + stdout 是否 tty |
| 局面呈现 | 引擎 `Frontend` 新增**局面摘要事件**（带默认实现、阶段闭合后下发），只带剧本无关字段；剧本专属资源信息由剧本侧自持 |
| 冒烟范围 | **只跑单局**（批量统计归 `umaai-tools`）；入口无子命令，等 P6′ 有 update / 探针再引入 |
| 剧本特化渲染 | 剧本自己的分布 / 资源面板**延后到 P7′**（等空剧本模板有真实消费者时定形），本仓只出与剧本无关的原语 |
| 选择器口径 | 入口只认基础局与契约参考训练员，**未知选择器报错并列出可用值**；剧本装配是剧本仓自带 bin 的事 |
| 默认马娘 | 默认配置改用**目标宽松**的马娘，使随机训练员也能跑满全部回合；测试夹具与默认层保持同值并加断言守门 |

---

## 10. 验收标准

- workspace 仅含 6 个 crate，核心 `git diff` 在接入新剧本时为空
- 核心 / 引擎 `cargo check` / `test` 全绿，`clippy` 无 warning
- `umaai-core` / `umasim` 的 lib 中零终端依赖；终端依赖只在 cli；dev 重依赖只在 tools
- `umaai-core` 只依赖白名单库（`serde` / `serde_json` / `thiserror` / `rand` / `int-enum` / `log` 门面）；数据载入无日志副作用
- `BasicGame` 固定种子可复现并跑完全部回合
- baseline 可回归、`--update` 可回写
- 核心枚举 `non_exhaustive`、serde 默认值齐全，新增钩子均带默认实现
- 空剧本模板可在不改引擎前提下 `cargo check` 通过
- `src` 无超长文件（内联测试只留私有细节）

---

*未决项见 [`docs/notes/open_items.md`](../notes/open_items.md)。*
