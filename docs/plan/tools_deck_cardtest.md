# 组卡 / 测卡工具方案（P8′ 定稿）

> 2026-10-18 用户拍板。本文是「组卡工具 / 测卡工具」的单一事实源，取代此前聊天中的过程稿；
> 阶段归属见 [`miko_next_plan.md`](miko_next_plan.md) §9 的 **P8′**，未决项见 [`../notes/open_items.md`](../notes/open_items.md)。
>
> 一句话：**通用层只做「粗略剪枝 + 测量编排」，精确排序归剧本仓**；两个工具共享同一套
> 卡池 / 构成 / 配对跑批内核，`deck`（组卡）与 `cardtest`（测卡）是两个子命令。

---

## 1. 形态（已拍板）

| 项 | 定稿 |
|---|---|
| 通用层（本仓 `umaai-tools`） | 卡池筛选、静态面板分、构成枚举、**逐类型实测扫描**、多臂同种子配对跑批、配对统计、呈现 |
| 剧本层（剧本仓） | 必带卡、构成约束、卡池限定、**手写训练员**、精确排序装配与入口 |
| 子命令 | `umaai-tools deck`（组卡）、`umaai-tools cardtest`（测卡），与 `bench` / `probe` / `baseline` / `compare` 并列 |
| 剪枝形态 | **`deck` 子命令内的阶段开关 + 实现档开关**，不做独立子命令（理由见 §4.3） |
| 配对统计落点 | `umaai-core::stats`（跨算法复用、与游戏与算法无关） |
| 卡组/实验输入 | **自持 JSON 计划**，不读配置层数值（沿用 `compare` / `baseline` 纪律） |
| 具名卡组预设（`presets.decks`） | **延后**（等剧本仓跑出第一份可信榜单再做） |

---

## 2. 口径（已拍板）

| # | 口径 | 取值 |
|---|---|---|
| 1 | 卡池下限 | `cardId > 30250`（**不含 30250**）→ 30251–30321 共 **71 张** |
| 2 | 排除名单 `exclude` | 默认 `[30257]`（[友]塔可布莱恩，固有类型 23 未实现）；**可配置**，给了即整体覆盖 |
| 3 | 稀有度 / 档位 | 池内全为 SSR；默认取**满破档**（`idrank = cardId × 10 + 4`）；**不做突破档 0–4 扫描**（拍板），`--cards` 仍可显式给别的档位 |
| 4 | 单类型张数上限 `max_per_type` | **不设通用值**（剧本可 5 速）→ 参数化，默认不限，由剧本层声明 |
| 5 | 卡种数下限 `min_types` | 同上，默认不限；口径为**全卡组 7 类**（含友人 / 团队与必带卡，2026-10-24 扩，见 open_items 近期收口 #33）——拉面杯「种类 ≥4」、miko「≥5 种」等剧本门禁属此 |
| 6 | 参考卡组 | 默认取 GA 收敛的万能骨架（30289 / 30312 / 30317 / 30306 / 30304，速2耐1智2） |
| 7 | 剪枝档 | 默认 **`measured`（逐类型实测扫描）**；`static` 为快档/兜底；`none` 不剪枝 |
| 8 | 测量训练员 | 剧本层用**剧本特化手写逻辑**；本仓基础局档用契约随机训练员（仅供自测，结论不外推） |
| 9 | 落盘 | 只出 stdout（人读表 + 机读单行）；CSV / 图延后（`open_items` #15） |

---

## 3. 依据：决定设计的事实

### 3.1 数据面（`cardDB.json`，563 卡）

- 满破面板键：`youQing / ganJing / xunLian / deYiLv / initialJiBan / hintLevel / hintProbIncrease / saiHou / wizVitalBonus / failRateDrop / vitalCostDrop / eventEffectUp / eventRecoveryAmountUp / bonus[6] / initialBonus[6] / hintBonus[6]` + `uniqueEffectType/Param`。
- **没有**卡自带技能、技能稀有度、每卡事件 —— 静态分不能声称涵盖技能价值，只能经 PT / Hint 折算间接进模拟。
- `idrank = cardId × 10 + 突破档`（0–4），`cardValue[4]` = 满破；JSON 内无 `idrank` 字段。

### 3.2 新卡池（`cardId > 30250`，n = 71）

| 类型 | 张数 | 类型 | 张数 |
|---|---|---|---|
| 速 (0) | 17 | 根 (3) | 15 |
| 耐 (1) | 12 | 智 (4) | 12 |
| 力 (2) | 11 | 友人 (5) | 4 |
| | | 团队 (6) | 0 |

- 训练系合计 **67 张**；友人 4 张（30257 / 30276 / 30290 / 30305）。
- **固有未实现只剩 30257 一张**（类型 23），按口径 2 排除后，全池固有效果均为已实现类型（0/1/2/5/13/20/21）。
  这是改用新卡池的最大收益：全池口径下 43 张的系统性低估问题收敛为 0。

### 3.3 先验：GA 万能骨架全部落在新池内（实测核对）

| idrank | 卡 | 类型 | 满破面板（情/训/得/羁/赛后） |
|---|---|---|---|
| 302894 | 青春永驻 | 智 | 40 / 10 / 100 / 30 / 10 |
| 303124 | 千明代表 | 速 | 35 / 20 / 120 / 30 / 10 |
| 303174 | 待兼诗歌剧 | 速 | 30 / 20 / 120 / 35 / 10 |
| 303064 | 优秀素质 | 智 | 30 / 20 / 80 / 35 / 10 |
| 303044 | 名将怒涛 | 耐 | 40 / 15 / 120 / 35 / 10 |

构成 = 速2耐1智2（旧仓 GA 314 轮中占 96.2% 的骨架），固有全为类型 1。

### 3.4 成本与噪声（本仓基础局 + 契约随机训练员实测）

| 量 | 实测 | 用法 |
|---|---|---|
| 单局 | ~42 µs（1000 局 ≈ 42 ms） | 基础局档可跑百万局；剧本档按 1.3 ms/局量级折算 |
| 单臂噪声 | 终局评分 sd ≈ 778–818（均值 ~16.3k） | 定局数 |
| 配对收益 | 同类型单卡替换 Δsd = 173 / 231 / 308（削减 57%–78%） | 配对是必需品，不是优化 |
| 同档卡边际 | Δmean 仅 +23 ~ +27 分 | 1000 局/臂可分辨 ~25 分 |
| 槽位顺序 | 同卡组仅换前两槽顺序：Δsd = 264 | **顺序是变量**，工具必须钉死槽位约定 |
| 跨类型替换 | Δsd = 398 | 单卡对比必须同类型同槽位 |

搜索档参照：N=1024 时单决策点 3328 rollout / 21 ms → 若逐回合搜索约 1.6 s/局，
与基础局差 4 个数量级，故必须「便宜档粗排 + 贵档精评」。

### 3.5 搜索空间（新池、6 槽含 1 必带卡）

| 剪枝档 | 卡组数 | 基础局 100 局/副 | 手写档（1.3 ms/局）100 局/副 |
|---|---|---|---|
| 不剪枝 | 9,657,648 | ~11 小时 | ~14.5 天 |
| Top-5 | 53,130 | ~3.7 分钟 | ~1.9 小时 |
| Top-3 | 3,003 | ~13 秒 | ~6.5 分钟 |

逐类型扫描成本：5 类型 × ~13 卡 × 1000 局 ≈ 6.7 万局（基础局 ~3 秒 / 手写档 ~1.5 分钟）。

---

## 4. 通用层设计（P8a′）

### 4.1 模块与 API

```
crates/umaai-core/src/stats.rs
    pub struct Paired { n, mean, sd, se, t, ci95, wins, losses, ties }
    pub fn paired(a: &[f64], b: &[f64]) -> Result<Paired>   // 等长配对差分 b − a

crates/umaai-tools/src/deck.rs
    pub struct DeckSpec { card_id_min, exclude, rank, slots, fixed, max_per_type, min_types }
    pub struct CardRep  { card_id, idrank, card_type, name, panel }
    pub struct Panel    { youqing, ganjing, xunlian, deyilv, jiban, saihou, hint_level, score }
    pub fn pool(&DeckSpec) -> Result<Vec<CardRep>>            // 按 Data 过滤 + 满破档
    pub fn panel(rep) -> Panel                                // 静态面板分（分项 + 加权和）
    pub fn compositions(&DeckSpec) -> Vec<Composition>        // 构成枚举（受上限/下限约束）
    pub fn scan(&ArmsPlan) -> Result<ScanReport>              // 逐类型实测扫描（同种子配对）

crates/umaai-tools/src/arms.rs
    pub struct Spec { runs, seed }
    pub struct Arm { deck, values }                           // 逐局观测 + mean/sd/median/p10
    pub struct ArmResult { arm, paired, runs_for_goal }
    pub fn run(spec, decks, goal, measure: F) -> Report        // 闭包注入：measure(deck, seed) -> f64
    // 剧本仓把手写训练员包进 measure 即可复用同一套配对与统计口径

crates/umaai-core/src/stats.rs（续）
    pub fn normal_cdf / normal_quantile / two_sided_p          // 正态近似（不引分布表依赖）
    pub enum Correct { None, Bonferroni, Holm }                // 多重比较校正口径
    pub fn Correct::adjust(&[Paired]) -> Vec<Adjusted>         // 逐位对齐的 p / p_adj / 校正 CI / 判定

crates/umaai-tools/src/cardtest.rs                             // 测卡工具内核（P8c′）
    pub struct Spec { slot, candidates, runs, seed, metric, goal, allow_cross_type, correct }
    pub fn run(cfg, base, spec) -> Report                      // 固定基准卡组、只换一槽
    pub struct GreedySpec { candidates, runs, seed, metric, goal, allow_cross_type, max_steps, correct }
    pub fn run_greedy(cfg, base, spec) -> GreedyReport          // 逐槽贪婪：每步取最优且越过零者替换
    // 三条守卫：基准过短 / 候选跨类型（测的是构成效应）/ 候选造出重复卡组

crates/umaai-tools/src/deck.rs（续）
    pub fn measure(cfg, deck, seed, metric) -> f64             // 基础局档的测量函数
    pub fn rank(cfg, spec, training, rank_spec, scan) -> RankReport
    pub enum PruneMode { Static, Measured }                    // 代表卡口径
    pub enum Stage { Scan, Compose, Rank, All }                 // 流水线阶段（计划 JSON 也用它）
    pub struct Plan { stage, seed, … }                          // 自持实验计划（`--plan`，见 §4.6）
    pub fn load_plan(path) -> Result<Plan>
    pub fn assemble / order_by_type / measured_order / deck_types
```

分工：`stats` 是纯函数；`deck` 是卡池与构成语义；`arms` 是跑批编排（泛型，剧本仓可复用）。

### 4.2 流水线

```
阶段 1  scan     逐类型实测扫描
        固定参考卡组其余槽位，只替换目标类型槽位，逐候选同种子配对跑批
        → 每类型实测榜 → Top-K（默认 5）；报告口径写死「在参考卡组 R、训练员 T、N 局下」
阶段 2  compose  构成枚举 + 组装（受 DeckSpec 约束；method = enum | greedy）
阶段 3  rank     整体实测排序：每构成一副代表卡组 → 粗筛（少局）→ 精评（多局）→ 独立种子集 holdout
```

- **阶段 1 即交付「各类型最强卡」榜**（原需求的第一半），可单独跑。
- **参考卡组 = 训练卡序列 + 必带卡按槽位拼接**（`--fixed <槽=idrank>`）：必带卡只进组、不参与扫描；
  总长低于 **5 张**（引擎侧支援卡事件的人头越界下限，见 `docs/issues.md`）、或训练卡张数与空槽位数不匹配，都在入口点名拒绝。
- **入口三条守卫（都不静默）**：① 参考卡组里参与扫描的卡必须在池内，否则该类型没有「零差分」基准，
  直接报错（曾被 `--exclude` 静默抹掉，见 `docs/issues.md`）；② 与参考卡组**其他槽位同卡**的候选跳过并计数
  —— 那会造出重复卡组，游戏里组不出来，且引擎反查 `deck_index_of` 会静默命中第一张；
  ③ 全线零差分即报错（候选可能根本没注入，自证只保证可复现、不保证注入了）。
- **未出榜类型**（参考卡组里没有的类型，如默认骨架缺力 / 根）在报告与机读行里点名，不静默跳过。
- **阶段 3 的候选 = 一种构成一副代表卡组**（沿用旧仓 `bench_compositions` 口径，不做全空间穷举；
  新池全空间约 965 万副，126 种构成各出一副）；代表卡由剪枝口径给出（`static` 面板分 / `measured` 实测排名）。
- **`measured` 剪枝的缺榜类型回退静态序**：实测榜只覆盖参考卡组里出现过的类型，其余类型（如默认骨架缺力 / 根）
  没有榜——若留空，所有用到该类型的构成都会装配失败被静默跳过（初版即如此，126 副里丢了 105 副）；
  现在改为回退静态面板序，并在报告与机读行里点名「回退静态序类型」。
- **阶段 3 必须留 holdout**：选择集与验收集分离，防 winner's curse（旧仓 `validated_policy` 纪律）；
  选择种子与验证种子缺省不同（`--seed` 与 `--holdout-seed`），相同即拒绝。
- **排序分两级、不可跨层比 Δ**：粗筛与精评局数不同，是两个尺子——已精评的整段排在仅粗筛的之前，
  段内各按自己的 Δ 降序（初版把两段混排，属已修缺陷）。
- 静态档 `--prune static` 只用阶段 2 的代表卡选择，不做实测；`--prune none` 走全枚举（仅建议配 `greedy`）。

### 4.3 为什么剪枝是开关而不是子命令

1. **参数与校验复用**：`cli.rs` 的 `Raw::finish` 对每个子命令维护「陌生开关名单」（`stray()`）；
   拆成 `prune` 子命令要再抄一份卡池 / 种子 / 观测轴参数表与归属校验。
2. **中间产物共享**：`--stage scan` 落「各类型榜」，`compose` / `rank` 接着读，同计划文件同口径。
3. **剧本仓只装配一套 API**：剧本仓注入 `DeckSpec` + 手写训练员，不该为「静态 / 实测」分叉接口。

### 4.4 CLI 形态

```
umaai-tools deck
  --stage scan|compose|rank|all   阶段（默认 scan；rank = 排序，all = scan + rank 且复用本次扫描）
  --prune static|measured         代表卡口径（默认 measured；static 为快档）
  --prune measured|static|none    剪枝实现档（默认 measured）
  --card-id-min <N>               卡池下限（默认 30251，即 cardId > 30250）
  --exclude <id,id>               排除名单（默认 30257；给了即整体覆盖）
  --no-exclude                    清空排除名单
  --seed-deck <idrank,...>        参考卡组（默认 GA 骨架）
  --top-k <N>                     每类型保留候选数（默认 5）
  --slots <N>                     卡组总槽位（默认 = 5 张训练卡 + 必带卡数；低于 5 张拒绝）
  --fixed <槽位=idrank>           必带卡（可重复；只进参考卡组，不参与扫描）
  --max-per-type <N>              单类型张数上限（默认不限）
  --min-types <N>                 种类数下限（默认不限）
  --runs <N> --seed <S>           每臂局数与起始种子（阶段 1 / 3）
  --metric score|pt|five          观测轴（默认 score）
  --screen-runs/--fine-count/--holdout-seed/--holdout-count/--goal   rank 阶段的两级与验证参数
  --correct none|bonferroni|holm  多重比较校正口径（默认 none；只对 scan / all 有意义）
  --plan <路径>                   自持计划 JSON（见 §4.6）

umaai-tools cardtest
  --base <idrank,…>               基准卡组（必给，至少 5 张）
  --slot <N> | --greedy           单槽控变量 / 逐槽贪婪（互斥）
  --cards <idrank,…>              显式候选（必给，可含不同突破档）
  --greedy-steps <N>              贪婪步数上限（默认 5，需 --greedy）
  --allow-cross-type              放行跨类型候选（默认拒绝：那是构成效应）
  --runs / --seed / --metric / --goal   与 deck 同口径
  --correct none|bonferroni|holm  多重比较校正口径（默认 none；族 = 本批候选 / 每步组合）
```

### 4.5 统计与报告纪律

- 每臂：`n / mean / median / sd / p10`；配对：`Δmean / Δsd / SE / t / 95%CI / 胜·负·平`。
- **默认只报 CI，不做二元「显著」判决**（多臂会踩多重比较）；`--correct` 可选 **Bonferroni / Holm**
  （正态近似，不引分布表依赖），开启后加「校正 CI / 校正 p / 越过零」三类列，**排序不变**、默认报告不加列。
  族 = 一次运行里互相比过的一组候选：`cardtest` 取本批候选（贪婪取每步的组合），`deck --stage scan` 取各类型榜。
- 报告必须写死上下文：参考卡组、训练员、种子集、局数 —— **不写「X 是强卡」**。
- 附带「按当前 Δsd 反推达成目标分辨率所需局数」。
- 异常局保留、不补采（旧仓 `validated_policy` 纪律）；配对键不一致即报错，不静默降级。

---

### 4.6 自持实验计划（`deck --plan`）

- **只给 `deck`**：`cardtest` 的输入本来就是显式卡组 + 候选，命令行即自持，不接计划文件。
- **`stage` 与 `seed` 必给**（计划自身要可复现），其余字段缺省取 `deck` 模块的常量，**不读配置层数值**。
- **实验变量只认计划**：同名 CLI 开关同时给即点名报错，不做「CLI 覆盖计划」的双来源合并；公共项
  （`--data-root` / `--width` / `--quiet`）不属实验变量，照旧可用。
- 严格键（`deny_unknown_fields`）：拼错的键必须被看见；`exclude: []` 与命令行 `--no-exclude` 同义（清空）。

## 5. 剧本层设计（P8b′，剧本仓）

不属于本仓交付，但接口在本仓冻结：

| 项 | 归属 | 说明 |
|---|---|---|
| 必带卡 | 剧本仓 | 如剧本友人卡 → `DeckSpec.fixed`（槽位 + idrank）；本仓只提供结构 |
| 构成约束 | 剧本仓 | `max_per_type` / `min_types`（拉面杯「种类 ≥4」属此） |
| 卡池限定 | 剧本仓 | 剧本专属卡 / 限定池 |
| 手写训练员 | 剧本仓 | `Trainer` 实现；本仓基础局档只用契约随机训练员 |
| 入口 | 剧本仓 | 复用 `umaai-tools` 的 `deck` / `arms` 库函数（样例面，按 git rev 钉住，与复用 `umaai-cli` 同例） |

---

## 6. 守门与验收

- **不进 L4 基线**：本工具属编排 / 统计面，基线只守模拟数值位移；工具自身守门走 L3 工具冒烟。
- L1：卡池筛选边界（下限 / 排除 / 满破档）、构成枚举的合法性与计数、配对统计的退化样本、
  正态原语与多重比较校正（Bonferroni / Holm 的手算档位与单调性）、
  计划自检（`stage` / `seed` 必给、槽位越界、局数为 0、阶段与校正不搭）、
  「候选必须与基准同类型」与贪婪可放组合（同类型槽位 / 重复卡组 / 跨类型开关）。
- L3（`crates/umaai-tests/tests/tools_smoke.rs`）：`deck --stage scan` 端到端可跑（含参考卡零差分自证）；
  同参数两次逐位一致（耗时字段不计入判据）；`--exclude` 三态、构成计数（126 / 101 / 21）、
  必带卡拼接、未出榜类型与重复候选计数、过短卡组 / 被排除的参考卡 / 不存在的必带卡 / 越界突破档等守卫；
  计划驱动 compose / scan（路径回显 + 变量生效 + 同计划可复现 + 与 CLI 开关互斥 + 严格键）、
  校正口径（缺省不出校正列、校正 CI 不小于未校正）、逐槽贪婪（逐步机读行、停止原因、组合留证、
  重复候选 / 零步数 / 无可放组合守卫）。
- 文档：`docs/tests_overview.md` 随实现更新；`changelog` / `issues` / `project_context` 在提交准备阶段统一更新。

---

## 7. 不做的事（减法）

1. 不做全卡池穷举（新池也 965 万副）；不做 GA / 优化器（算法探索归独立 workbench，见 plan §8）。
2. 不在本仓实现剧本数值 / 手写训练员；不做剧本特化呈现。
3. 不做 CSV / 归档 / 趋势图（列契约按真实下游冻结，`open_items` #15）。
4. 不新增依赖（统计自持；不引优化 / 统计库）。
5. 不做 `presets.decks`（延后）。
6. 不把静态面板分当强度结论（它只用于排序展示与兜底）。
7. 不做突破档 0–4 扫描（拍板）：默认全满破；`--cards` 仍可显式给别的突破档 idrank。

---

## 8. 阶段与交付物

| 阶段 | 交付 | 状态 |
|---|---|---|
| **P8a′** | `umaai-core::stats` 配对原语 + `umaai-tools` 的 `deck` / `arms` 内核 + `deck` 子命令（`scan` / `compose` / `rank` / `all`）+ 单元与工具冒烟 | ✅ 已落地（含 `--plan` 自持计划 JSON、`--correct` 校正口径） |
| **P8b′** | 剧本仓复用装配（必带卡 / 构成约束 / 手写训练员 / 精确排序）+ 第一份可信榜单 | 待剧本仓 |
| **P8c′** | `cardtest` 子命令（固定基准卡组 + 变一槽 + 显式候选；含跨类型与重复卡组守卫） | ✅ 已落地（含逐槽贪婪与多重比较校正；**突破档 0–4 扫描经拍板不做**，默认全满破） |

---

## 9. 风险与待决项

| # | 项 | 状态 |
|---|---|---|
| 1 | 结论依赖剧本：基础局档测得的卡强度对拉面 / 温泉不成立 | 已知，报告口径绑定上下文 |
| 2 | 训练员依赖：换训练员 / 换卡组 / 换预算都会换结论 | 已知，报告必须标注训练员与卡组 |
| 3 | 阶段 1 的排名依赖参考卡组（上下文相关） | 报告写死参考卡组；可选两轮迭代（用首轮结果重组参考卡组） |
| 4 | 逐类型扫描的槽位选择：某类型张数 > 1 时选哪个槽位 | 默认取该类型首个槽位；多张类型的槽位差异进 open_items |
| 7 | 同卡双持（同一张卡占两个槽位）无法评测 | 该候选属重复卡组，扫描直接跳过计数；引擎侧现在**构造期就拒绝**重复卡组（`BaseGame::new` 按 `card_id` 判重），要评「同卡双持」必须另开口径 |
| 8 | 引擎侧卡组长度没有下限 | 少于 5 张会越界，真实触发点是**支援卡事件生成里硬编码的人头上界 6**（不是人头分配）；工具入口点名拒绝，引擎侧的下限校验另立（见 `docs/issues.md`） |
| 5 | `cardtest` 完整形态（贪婪 / 突破扫描 / 多重比较校正） | 已定稿：逐槽贪婪与多重比较校正已落地；**突破档 0–4 扫描不做**（2026-10-18 拍板，默认全满破；`--cards` 仍可显式给别的突破档 idrank） |
| 6 | 具名卡组预设 `presets.decks` | 延后（`open_items`） |
| 9 | 自持实验计划 JSON 的列契约 | 已落地（`deck --plan`）：阶段 / 种子 / 卡池 / 参考卡组 / 必带卡 / 局数 / 观测轴 / 校正口径 / 排序与验证参数；`cardtest` 不接计划 |
