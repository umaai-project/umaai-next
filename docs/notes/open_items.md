# 待决事项（Open Items）

> 本文件跟踪方案 v2 尚未拍板的设计点。拍板后移入
> [`docs/plan/miko_next_plan.md`](../plan/miko_next_plan.md) 对应章节并从本表移除。

| # | 事项 | 现状 / 倾向 | 备注 |
|---|---|---|---|
| 2 | **baseline 数据文件位置** | 待定 | 候选：`crates/umaai-tests/baselines/` · `crates/umaai-tests/tests/baseline/` · 仓库根 `baselines/` |
| 3 | **更新器交互细节** | 待定 | 启动自动检查+提示 / 仅 `umaai update` 子命令 / 两者；`ura_data`（小黑板数据）是否纳入 |
| 4 | **算法探索 workbench 归属** | 倾向独立项目 | 独立仓 / 仓内单列 / 暂不进仓；已留 seam |
| 5 | **测试 crate 命名** | 暂 `umaai-tests` | 备选 `umaai-it` / `umaai-e2e` |
| 6 | **发布 exe 名** | 暂 `umaai-cli`（P5′ 定：bin 名保持不改） | 最终 `umaai.exe` 产物名在打包阶段定；注意与 lib crate `umaai` 的同名产物冲突 |
| 7 | **gamedata 数据版本策略** | 待定 | `manifest.json` 的 `schema_version` 语义、数据来源盖章方式 |
| 8 | **`umaai-rs-ref` 是否随进度更新** | 待定 | 现为 master 只读工作树，建议冻结不动 |
| 10 | **剧本钩子 trait 形态** | 已拍板延后 | `ScenarioRules` / `ScenarioLifecycle` 待有具体剧本时设计；扩充纪律：带默认实现、非破坏 |
| 11 | **无剧本基础局不带专用智能算法** | 已拍板 | `BasicGame` 仅为冒烟基线，无终端用户；智能算法（策略 / NN 特征 / 调参）归剧本仓，umasim 只交付框架与骨架（搜索泛型 / `ActionIndex` / evaluator 壳 / rollout 基础设施）。例外：`RandomTrainer` 作为契约参考实现与 CLI 调试件，P5′ 前产品化 |
| 12 | **搜索批量 rollout 后端** | 已拍板延后 | 跨 rollout 攒批（如整批送 GPU）的教师后端：仓内无实现者、无法验证。等真实需求出现再设计；内核闭包注入入口已留（`search_with_terminal`） |
| 13 | **leaf 截断估值（`max_depth > 0`）** | 已拍板延后 | 依赖神经层（`Evaluator` / 特征编码）；当前搜索入口对未声明支持的剧本硬拒，不静默降级为终局搜索 |
| 14 | **搜索层第二评分轴（原始分 / PT 分双轴）** | 已拍板不保留 | 搜索只留含 PT 偏好折算的**单轴**评分；需要原始分时作为终局观测维度输出（用户拍板） |
| 15 | **在线记录落盘（CSV / meta / 归档 / 趋势图）** | 已拍板延后到有真实消费者时 | 引擎只交付记录数据模型与运气分算法（P4c）；列契约需按真实下游（复盘工具 / 在线运行）冻结，归档与绘图属重依赖，归 `umaai-tools` / `umaai-cli` |
| 17 | **事件成功/失败口径** | 已定临时口径，待真实剧本与真机数据校准 | 帧事件按成功/失败两支重建、按常量表通用成功率分概率，决策与结算同源；模拟事件表没有「事件成功失败」维度，该口径会同时影响事件决策期望与运气分基线。调整只需改事件重建一处，详见 issues |
| 18 | **剧本特化呈现（分布 / 资源面板）的辅助渲染件** | 已拍板延后到 P7′ | 本仓只出与剧本无关的呈现原语（见 plan §9 的 P5′ 定稿）；剧本各自的分布面板除人头分布外还要显示剧本资源，形态取决于真实剧本，等 P7′ 空剧本模板按下真实需求定形，避免无消费者设计。定形时应是「剧本仓构造数据 + 调用本仓原语」，且不得做成契约 trait 方法 |
| 19 | **下游平台可用性改造** | 部分完成（帧通道 + 并行 + 数据源已落地） | 已完成：文件通道特性化 `file-channel`（帧形态常驻、监听可关）；并行特性化 `parallel`（`umasim` 提供 + `umaai` 透传，关闭即降级串行并剪掉 rayon，能力查询 `search::parallel_supported()`）；数据源 `DataSource`（目录 `DirSource` / 内存 `MemorySource`，收 JSON 值与 JSON 文本；配置按「不入来源」处理，叠加器不做）。待做：跨目标编译守门（暂无 wasm / Android 消费方）。详见 plan §9「下游可用性改造」与 §3「下游接入与契约分层」 |
| 20 | **剧本快照 DTO 的取值与序列化细节** | 已拍板延后到 P7′ | 形态已定（见 plan §2「给前端的剧本数据」）：剧本自持纯数据 View + 单一取数 API，不进契约 trait；**值快照 vs 借用视图**、**是否默认带序列化**留到首个真实剧本落地时定形 |

## 迁入本仓时已知的文档悬空引用

`docs/game/` 由旧仓原样迁入，其中指向旧仓 `archive/`、`replay_review.md` 等的相对链接在本仓不存在，属已知悬空（仅文本层面，不影响构建）。整理时机待定。

（原「测试规范指向的 `test_overview` 文档不存在」一项已闭环：改建 [`docs/tests_overview.md`](../tests_overview.md) 并把 `AGENTS.md` 的引用指向它。）
