# miko_next 重建方案 v2（独立新仓 + 选择性移植）

> 本文取代 v1 方案（`umaai-rs/.trae/documents/miko_next_rebuild_plan.md`）。v1 作为机制清单与附录参考随仓保留于
> [`docs/plan/v1_plan_reference.md`](v1_plan_reference.md)。
> 本文记录 2026-10-14 定稿决策与 2026-10-15 的层界修订；未决项集中在 [`docs/notes/open_items.md`](../notes/open_items.md)。

---

## 0. 决策基线（已拍板）

| # | 决策 |
|---|---|
| 1 | 另起**空仓** `miko_next`，从零初始化；**不在旧仓原地重构** |
| 2 | 旧仓 `umaai-rs`（含其 `miko_next` 分支）**暂不动**；`umaai-rs-ref`（master 只读工作树）作移植源与行为基准 |
| 3 | 新仓**独立全新 git 历史**；完工后整体并入 `umaai-rs` 的 `miko_next` 分支 |
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
| 15 | 核心层依赖口径：**轻量、跨平台、通用**的库（`serde` / `serde_json` / `thiserror` / `rand` 0.9 / `int-enum`）；禁终端 / 网络 / 计算重依赖与平台相关件。呈现（着色 / 表格 / 日志 / 交互）只在 `umaai-cli` |

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
| `umaai-core` | 纯 schema/DTO（gamedata 静态数据类型）、数据加载（`Data` 单例 / 路径注入 / 校验）、零依赖纯函数工具（**通用统计原语**、**CRN 种子表**）、RNG 三流、统一错误 |
| `umasim` | 带行为的领域类型（`Uma` / `FriendState` / `BaseGame` / `simulator` …）、全部契约 trait（`Game` / 剧本钩子 / 搜索 / 策略）、领域算法（`flat_search` / `trainer/*` / `neural/*`）、搜索与 NN 私有数据形态 |
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

**不移植**：`game/{ramen,onsen}` 全部剧本实现（留 seam + 空模板）。

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
| **P3′** | 搜索框架泛化：flat_search / searchable / result / terminal（复用 core 的统计原语与 CRN 种子表） | — |
| **P4′** | 引擎 `umaai`：`Runner` / `Frontend` / `Config` + 单场景协议 + record / luck | — |
| **P5′** | `umaai-cli`：呈现全入 cli；`BasicGame` 跑满回合冒烟 | — |
| **P6′** | `umaai-tools`：探针 / bench / baseline runner + `--update` | — |
| **P7′** | 空剧本模板 + 文档（机制 / 契约 / 术语）+ 验收 | — |

---

## 10. 验收标准

- workspace 仅含 6 个 crate，核心 `git diff` 在接入新剧本时为空
- 核心 / 引擎 `cargo check` / `test` 全绿，`clippy` 无 warning
- `umaai-core` / `umasim` 的 lib 中零终端依赖；终端依赖只在 cli；dev 重依赖只在 tools
- `umaai-core` 只依赖白名单库（`serde` / `serde_json` / `thiserror` / `rand`）；数据载入无日志副作用
- `BasicGame` 固定种子可复现并跑完全部回合
- baseline 可回归、`--update` 可回写
- 核心枚举 `non_exhaustive`、serde 默认值齐全，新增钩子均带默认实现
- 空剧本模板可在不改引擎前提下 `cargo check` 通过
- `src` 无超长文件（内联测试只留私有细节）

---

*未决项见 [`docs/notes/open_items.md`](../notes/open_items.md)。*
