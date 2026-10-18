# 剧本仓接入指南

本仓是**引擎仓**（`umaai-next`）；**剧本＝独立仓**，以依赖引用本仓的 `umaai-core` / `umasim` / `umaai`，
不在本仓加剧本 crate、不 fork 引擎。空模板见 [`templates/umaai-scenario/`](../../templates/umaai-scenario/README.md)，
契约逐项清单见 [`contracts.md`](contracts.md)。

## 1. 建仓

1. **复制模板 + 改名**：`templates/umaai-scenario/` → 自己的剧本仓，crate 名改 `umaai-<剧本>`（如 `umaai-ramen`）。
2. **依赖三档**：开发期 `path`（并列目录）→ 稳定期 git `tag`（同一 repo、同一 tag）→ 下游宿主
   `default-features = false`（剪掉并行后端与文件监听）。模板 README 有现成写法。
3. **数据与配置都由宿主注入**：引擎数据根（通用表 + 默认配置）与剧本数据**各自独立**、位置按终端定，
   引擎与剧本都不写死路径；引擎静态表走 `umaai_core::DataSource`，剧本数据由剧本侧自持。
4. **交付物**：**lib 为主**（剧本类型 + 装配 API + 自实现 `Frontend` 参考件），**bin 为辅**
   （主线自测外壳，放仓根）；终端 / Web / Android 各自装配自己的入口。

## 2. 实现清单

| 面 | 要实现的 | 参考 |
|---|---|---|
| 局面 | `Game`（22 个必备方法）+ `TrainingValueSource` | `contracts.md` §1 |
| 人头 / 动作 | `Person`（模板直接复用 `BasePerson`）/ `ActionEnum` | `contracts.md` §2 |
| 决策 | `Trainer`；要进搜索就实现 `FlatSearchGame` | `contracts.md` §3 |
| 前端 | `Frontend`（全默认空实现，按需覆盖）；终端可复用 `umaai-cli::PrintFrontend`（样例面） | `contracts.md` §4 |
| 装配 | `Runner`（离线一局）/ `Session`（帧驱动，宿主喂帧） | `contracts.md` §5 |

## 3. 五条纪律

1. **阶段自持**：内部强类型枚举，对外只投影 `StageId`（显式赋号，**改号＝旧种子与基线失效**）；
   阶段枚举无数据、无引用、可 `Copy`；局面里不存「下一步是什么」，流转只由 `next()` 依数据算出。
2. **三条随机轴**：局面流 / 事件流从 `BaseGame` 取出（`take_rule_streams` 成对放回），策略流本局面自持，
   **决策流只给 `Trainer`**；每进入新回合按 `(master, 回合)` 重新派生——同回合的规则随机与决策抽签消耗
   无关，**CRN 配对由规则层自身承担**。
3. **主种子必注入**：`set_rule_master` 是唯一注入点（`Runner` / `Session` 与搜索分支自动注入）；
   未注入就推进会**报错**，不要加回退分支。
4. **搜索分支只经 `fork_for_rollout`**：克隆 + 注入 rollout 种子；任何**不由传入 rng 驱动**的流都必须
   在这里重置，否则可复现性与 CRN 一起失效且**不会报错**。
5. **View DTO（形态 A）**：剧本资源给前端走「纯数据 View + 单一取数口」，只放引擎不知道的字段、
   快照语义（幂等、无副作用）、`non_exhaustive`、**不进契约 trait**。取值与序列化定稿：**值快照**
   （owned、`Clone`），序列化由剧本自己 `serde` derive 提供，引擎不背书传输格式。

## 4. 验收口径

- `cargo check` 通过，**不改引擎任何一行**；引擎仓 CI 用 `--manifest-path templates/umaai-scenario/Cargo.toml` 守模板这一档。
- 接入新剧本时引擎仓 `git diff` 为空；确需新能力时回引擎仓按「带默认实现、非破坏」加钩子，禁止 fork。
- 固定种子可复现：同一 `master` 下两次运行 / 两支分支逐位一致。

## 5. 常见坑（本仓踩过的）

- **`Deref` 底座与契约不得同名**：同名方法会静默落到基座版（编译过、行为错位）——基座入口叫
  `init_rule_streams`、契约叫 `set_rule_master`。
- 忘了派生策略流 → 推进时报「策略流缺失」；只在 `set_rule_master` 与回合边界重置。
- 配置层未知键**严格报错**（拼错必须被看见）；剧本专属配置段归剧本仓，不进引擎配置。
- 「按阶段重播种」这套外挂 CRN **已删除**：配对由每回合派生承担，不要再引入第二套对齐机制。
- 搜索入口对 `SUPPORTS_TRUNCATED_LEAF = false` 的剧本**直接拒绝** `max_depth > 0`，不静默降级。
