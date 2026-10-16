//! 扁平蒙特卡洛搜索
//!
//! 对每个合法动作执行多次模拟，统计分数分布后选最优动作。两种资源分配：
//! 均匀分配（各候选同次数）与 UCB 动态分配（好的动作拿更多次数）。
//!
//! 内核不认识任何剧本：**一次 rollout 怎么跑由调用方以闭包注入**，剧本特判留在
//! 剧本侧；候选分配、CRN 种子、统计与排序全走同一份实现。这样剧本侧的特殊分发不会
//! 静默退化成通用路径（泛型 impl 内调用同名方法时，解析只会落到泛型版本）。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use log::{debug, warn};
use rand::SeedableRng;
use rand::rngs::StdRng;
use rayon::prelude::*;
use umaai_core::RolloutSeeds;
use umaai_core::diag::DiagGuard;

use super::config::SearchConfig;
use super::result::{ActionResult, OrderedRollouts, SearchOutput};
use super::searchable::FlatSearchGame;
use super::terminal::{NoTerminal, RolloutOutcome, TerminalRecord};

/// 单个候选在一次搜索中的全部累加结果
///
/// 建新类型而非元组：评分统计、终局统计、失败计数与有序缓冲一起被消费，
/// 位置参数已不可读。
struct CandidateAccum<D> {
    /// 本候选的评分分布（参与排序）
    score: ActionResult,
    /// 终局多维统计（不参与排序）
    terminal: D,
    /// rollout 失败次数
    failed: usize,
    /// 已**计划**的 rollout 次数（成功 + 失败；仅供成本探针读取）
    planned: usize,
    /// 按 rollout 序号对齐的原始分（开关关闭时为 `None`，不分配缓冲）
    ordered: Option<Vec<Option<f64>>>,
}

impl<D: Default> CandidateAccum<D> {
    /// 创建空累加器；`record_ordered` 为真时才分配有序缓冲
    fn new(record_ordered: bool) -> Self {
        Self {
            score: ActionResult::new(),
            terminal: D::default(),
            failed: 0,
            planned: 0,
            ordered: if record_ordered { Some(Vec::new()) } else { None },
        }
    }
}

impl<D> CandidateAccum<D> {
    /// 确保有序槽位覆盖到 `idx`（失败序号保持 `None`）
    fn ensure_ordered_slot(&mut self, idx: usize) {
        if let Some(ordered) = self.ordered.as_mut()
            && ordered.len() <= idx
        {
            ordered.resize(idx + 1, None);
        }
    }

    /// 并入一次成功 rollout 的结果
    ///
    /// 评分与终局统计在同一处推进，避免出现「评分记了、终局漏了」的样本集合错位。
    /// 失败项不走本方法，由调用方 [`Self::ensure_ordered_slot`] 留空。
    fn push<T>(&mut self, idx: usize, outcome: &RolloutOutcome<T>)
    where
        T: TerminalRecord<Stats = D>,
    {
        self.score.add(outcome.score);
        outcome.terminal.accumulate_into(&mut self.terminal);
        if let Some(ordered) = self.ordered.as_mut() {
            if ordered.len() <= idx {
                ordered.resize(idx + 1, None);
            }
            ordered[idx] = Some(outcome.score);
        }
    }
}

/// 探针的三份候选计数（`planned` / `succeeded` / `failed` 同序同长）
struct ProbeCounts {
    /// 每候选**计划**的 rollout 次数
    planned: Vec<usize>,
    /// 每候选**成功**的 rollout 次数
    succeeded: Vec<usize>,
    /// 每候选**失败**的 rollout 次数
    failed: Vec<usize>,
}

/// 一次搜索内核调用的成本记录（仅测量用）
///
/// 内核不暴露候选累加器（`CandidateAccum`），「每候选计划 / 成功 / 失败次数」正是预算核实要看的
/// 量。本结构把这些观测量原样带出，**不参与**任何判据。
#[derive(Debug, Clone)]
pub struct SearchProbe {
    /// 根局面回合
    pub turn: usize,
    /// 剧本阶段键
    pub stage_key: u64,
    /// 进入搜索的候选数
    pub candidates: usize,
    /// 每候选**计划**的 rollout 次数（成功 + 失败）
    pub planned: Vec<usize>,
    /// 每候选**成功**的 rollout 次数
    pub succeeded: Vec<usize>,
    /// 每候选**失败**的 rollout 次数
    pub failed: Vec<usize>,
    /// 本次搜索的激进度因子
    pub radical_factor: f64,
    /// 加权平均分口径下的中选下标（观测量，不保证等于调用方实际执行的动作）
    pub best_action_idx: usize,
    /// 内核开始时刻（与外层链路对齐用）
    pub started: Instant,
    /// 内核墙钟耗时
    pub elapsed: Duration,
}

impl SearchProbe {
    /// 计划 rollout 总数
    pub fn total_planned(&self) -> usize {
        self.planned.iter().sum()
    }

    /// 成功 rollout 总数
    pub fn total_succeeded(&self) -> usize {
        self.succeeded.iter().sum()
    }

    /// 失败 rollout 总数
    pub fn total_failed(&self) -> usize {
        self.failed.iter().sum()
    }
}

/// 扁平蒙特卡洛搜索
pub struct FlatSearch<G: FlatSearchGame>
where
    G::Action: Send + Sync + Clone,
{
    /// rollout 决策器（由剧本指定）
    rollout_trainer: G::RolloutTrainer,
    /// 搜索配置
    config: SearchConfig,
    /// rollout 失败是否让整次搜索失败（默认 `false`，只计数并告警）
    ///
    /// ❗**换过 rollout 基策的实验必须打开**：失败若与状态相关，剩下的样本就是
    /// 「以成功为条件」的分布，各候选条件还互不相同，排序会被系统性污染而分数上看不出来。
    strict_rollout: bool,
    /// 是否并行执行 rollout；关闭后串行执行，结果逐位一致（对照与调试用）
    parallel: bool,
    /// 成本探针出口（仅测量用，默认 `None`）
    probe: Option<Arc<Mutex<Vec<SearchProbe>>>>,
}

impl<G: FlatSearchGame> FlatSearch<G>
where
    G::Action: Send + Sync + Clone,
{
    /// 创建搜索器
    pub fn new(config: SearchConfig) -> Self {
        Self {
            rollout_trainer: G::default_rollout_trainer(),
            config,
            strict_rollout: false,
            parallel: true,
            probe: None,
        }
    }

    /// 用默认配置创建搜索器
    pub fn default_search() -> Self {
        Self::new(SearchConfig::default())
    }

    /// 替换 rollout 基策
    ///
    /// 搜索出的分数是**对 rollout 基策的**动作价值，换掉它就是换掉整个教师；
    /// 换过基策的结果与旧基线不可比。
    pub fn with_rollout_trainer(mut self, trainer: G::RolloutTrainer) -> Self {
        self.rollout_trainer = trainer;
        self
    }

    /// 就地改写当前 rollout 基策（不整体替换，调用顺序无关）
    pub fn map_rollout_trainer(mut self, f: impl FnOnce(G::RolloutTrainer) -> G::RolloutTrainer) -> Self {
        let old = std::mem::replace(&mut self.rollout_trainer, G::default_rollout_trainer());
        self.rollout_trainer = f(old);
        self
    }

    /// 设置 rollout 失败时是否让整次搜索失败
    pub fn with_strict_rollout(mut self, strict: bool) -> Self {
        self.strict_rollout = strict;
        self
    }

    /// 设置是否并行执行 rollout（关闭后串行，结果与并行逐位一致）
    pub fn with_parallel(mut self, parallel: bool) -> Self {
        self.parallel = parallel;
        self
    }

    /// 挂上成本探针出口（仅测量用；不改决策语义）
    pub fn with_probe(mut self, sink: Arc<Mutex<Vec<SearchProbe>>>) -> Self {
        self.probe = Some(sink);
        self
    }

    /// 搜索配置
    pub fn config(&self) -> &SearchConfig {
        &self.config
    }

    /// rollout 决策器
    pub fn rollout_trainer(&self) -> &G::RolloutTrainer {
        &self.rollout_trainer
    }

    /// 无终局观测的搜索入口（rollout 由闭包注入）
    pub fn search_with<F>(
        &self,
        game: &G,
        actions: &[G::Action],
        rng: &mut StdRng,
        rollout: F,
    ) -> Result<SearchOutput<G::Action>>
    where
        F: Fn(&G, &G::Action, u64) -> Result<f64> + Sync,
    {
        self.search_with_terminal(game, actions, rng, |g, a, seed| {
            Ok(RolloutOutcome {
                score: rollout(g, a, seed)?,
                terminal: NoTerminal,
            })
        })
    }

    /// 通用搜索入口：rollout 走通用路径，终局就地提取观测记录
    ///
    /// 剧本若有专属 rollout 路径（特殊动作分发、阶段重播种），改用
    /// [`Self::search_with_terminal`] 自行注入闭包。
    pub fn search<T, E>(
        &self,
        game: &G,
        actions: &[G::Action],
        rng: &mut StdRng,
        extract: E,
    ) -> Result<SearchOutput<G::Action, T::Stats>>
    where
        T: TerminalRecord,
        E: Fn(&G) -> T + Sync,
    {
        self.search_with_terminal(game, actions, rng, |g, a, seed| {
            self.simulate_common_extract(g, a, seed, seed, &extract)
        })
    }

    /// 带终局多维记录的搜索内核
    ///
    /// 与 [`Self::search_with`] 共用同一份实现（只差闭包多返回一个终局记录）：
    /// 候选分配、CRN 种子、失败处理与排序口径不会分叉。终局记录不参与中选判定。
    pub fn search_with_terminal<F, T>(
        &self,
        game: &G,
        actions: &[G::Action],
        rng: &mut StdRng,
        rollout: F,
    ) -> Result<SearchOutput<G::Action, T::Stats>>
    where
        T: TerminalRecord,
        F: Fn(&G, &G::Action, u64) -> Result<RolloutOutcome<T>> + Sync,
    {
        if actions.is_empty() {
            bail!("没有可用动作");
        }
        ensure!(
            G::SUPPORTS_TRUNCATED_LEAF || self.config.max_depth == 0,
            "本剧本没有 leaf 估值器，max_depth 必须为 0（当前 {}）",
            self.config.max_depth
        );

        let radical_factor = self.compute_radical_factor(game);
        // 本次搜索的 rollout 种子表：所有候选共享，由传入 rng 派生（可复现性入口）
        let seeds = RolloutSeeds::from_rng(rng);
        // 阶段键只求值一次：探针与重播种共用同一值，避免「挂探针会改变行为」
        let stage_key = game.crn_stage_key();
        debug!(
            "[回合 {}] 开始搜索: {} 个动作, search_n={}, max_depth={}, radical_factor={:.1}, ucb={}, 根种子={:#018x}",
            game.turn(),
            actions.len(),
            self.config.search_n,
            self.config.max_depth,
            radical_factor,
            self.config.use_ucb,
            seeds.root()
        );

        // rollout 期间静默诊断输出（RAII，Err 提前返回同样恢复）
        let _diag_guard = DiagGuard::suppress();
        // 探针关闭时不取时钟，避免为不用的数字付钟表开销
        let probe_started = self.probe.as_ref().map(|_| Instant::now());
        let collected = if self.config.use_ucb {
            self.search_ucb(game, actions, radical_factor, &seeds, &rollout)?
        } else {
            self.search_uniform(game, actions, &seeds, &rollout)?
        };

        // 某候选一次都没跑成功时统计全空，继续用下去等于拿垃圾数据排序
        for (i, acc) in collected.iter().enumerate() {
            if acc.score.count() == 0 {
                bail!("候选动作 {i} 的全部 rollout 均失败，搜索结果不可用");
            }
        }

        let record = self.config.record_ordered_rollouts;
        let probe_counts: Option<ProbeCounts> = self.probe.as_ref().map(|_| ProbeCounts {
            planned: collected.iter().map(|acc| acc.planned).collect(),
            succeeded: collected.iter().map(|acc| acc.score.count() as usize).collect(),
            failed: collected.iter().map(|acc| acc.failed).collect(),
        });
        let mut action_results = Vec::with_capacity(collected.len());
        let mut terminal_results = Vec::with_capacity(collected.len());
        let mut per_candidate = if record {
            Some(Vec::with_capacity(collected.len()))
        } else {
            None
        };
        for acc in collected {
            action_results.push(acc.score);
            terminal_results.push(acc.terminal);
            if let Some(ref mut rows) = per_candidate {
                rows.push(acc.ordered.unwrap_or_default());
            }
        }

        let mut out = SearchOutput::with_terminals(actions.to_vec(), action_results, terminal_results, radical_factor);
        if let Some(per_candidate) = per_candidate {
            out.ordered_rollouts = Some(OrderedRollouts {
                root_seed: seeds.root(),
                per_candidate,
            });
        }
        if let (Some(sink), Some(counts), Some(started)) = (self.probe.as_ref(), probe_counts, probe_started) {
            let probe = SearchProbe {
                turn: game.turn() as usize,
                stage_key,
                candidates: actions.len(),
                planned: counts.planned,
                succeeded: counts.succeeded,
                failed: counts.failed,
                radical_factor,
                best_action_idx: out.best_action_idx,
                started,
                elapsed: started.elapsed(),
            };
            match sink.lock() {
                Ok(mut v) => v.push(probe),
                Err(e) => warn!("[搜索][探针] 记录失败（锁中毒）: {e}"),
            }
        }
        Ok(out)
    }

    /// 单次 rollout，并在终局就地提取观测记录
    ///
    /// `extract` 在**终局局面尚存活时**调用：局面跑完即弃，多维事实无法事后补取。
    /// 决策 RNG 与规则层 `rule_master` 分开传入，生产路径两者相同。
    pub fn simulate_common_extract<T, E>(
        &self,
        game: &G,
        action: &G::Action,
        decision_seed: u64,
        rule_master: u64,
        extract: E,
    ) -> Result<RolloutOutcome<T>>
    where
        T: TerminalRecord,
        E: Fn(&G) -> T,
    {
        let rng = &mut StdRng::seed_from_u64(decision_seed);
        let mut sim_game = game.fork_for_rollout(rule_master);
        // 必须走剧本的真实对局路径，不能用通用 apply_action 顶替
        sim_game.apply_root_action(action, rng)?;
        while sim_game.next() {
            sim_game.run_stage(&self.rollout_trainer, rng)?;
        }
        sim_game.on_simulation_end(&self.rollout_trainer, rng)?;
        Ok(RolloutOutcome {
            score: sim_game.search_score(self.config.pt_favor_rate)?,
            terminal: extract(&sim_game),
        })
    }

    /// 双种子 rollout：决策 RNG 与规则层 `rule_master` 可分开
    ///
    /// 生产路径 [`Self::simulate_common`] 传入相同的两个值，保持既有 CRN。
    pub fn simulate_common_with_seeds(
        &self,
        game: &G,
        action: &G::Action,
        decision_seed: u64,
        rule_master: u64,
    ) -> Result<f64> {
        Ok(self
            .simulate_common_extract(game, action, decision_seed, rule_master, |_| NoTerminal)?
            .score)
    }

    /// 通用单次 rollout：执行动作后跑到终局
    ///
    /// 决策 RNG 与规则层 `rule_master` 使用同一 `seed`（生产 CRN：各候选第 j 次
    /// rollout 共享 `seed_at(j)`）。对照实验请走 [`Self::simulate_common_with_seeds`]。
    pub fn simulate_common(&self, game: &G, action: &G::Action, seed: u64) -> Result<f64> {
        self.simulate_common_with_seeds(game, action, seed, seed)
    }

    /// 按当前 `(回合, 阶段)` 重新播种 rollout 随机流（外挂 CRN）
    ///
    /// 只对**规则层未接管随机流**的剧本有意义：这类剧本的专属 rollout 闭包在每个
    /// 阶段边界调用本方法，使各候选在同一 `(回合, 阶段)` 抽到同一份随机性。规则层
    /// 已按 `(rollout 种子, 回合)` 自行派生的剧本不要调用它。
    pub fn reseed_for_stage(&self, rng: &mut StdRng, rollout_seed: u64, game: &G) {
        if !self.config.crn_stage_reseed {
            return;
        }
        *rng = StdRng::seed_from_u64(RolloutSeeds::stage_seed(
            rollout_seed,
            game.turn(),
            game.crn_stage_key(),
        ));
    }

    /// 计算激进度因子（确定式，无随机性）
    ///
    /// `(剩余回合 / 总回合)^0.5 × 上限`，总回合取 `max_turn() + 1`（78 回合下与既有
    /// 基准逐位一致；直接用 `max_turn()` 会让全表偏移并改变近似平局的决策）。
    fn compute_radical_factor(&self, game: &G) -> f64 {
        let total = game.max_turn() as usize + 1;
        let remain = total.saturating_sub(game.turn() as usize) as f64;
        (remain / total as f64).powf(0.5) * self.config.radical_factor_max
    }

    /// 均匀分配搜索：每个候选都从 rollout 0 开始，天然完全配对
    fn search_uniform<F, T>(
        &self,
        game: &G,
        actions: &[G::Action],
        seeds: &RolloutSeeds,
        rollout: &F,
    ) -> Result<Vec<CandidateAccum<T::Stats>>>
    where
        T: TerminalRecord,
        F: Fn(&G, &G::Action, u64) -> Result<RolloutOutcome<T>> + Sync,
    {
        let n = self.config.search_n;
        let record = self.config.record_ordered_rollouts;
        let run = |action: &G::Action| -> Result<CandidateAccum<T::Stats>> {
            let mut acc = CandidateAccum::<T::Stats>::new(record);
            self.simulate_many(game, action, n, seeds, 0, &mut acc, rollout, "")?;
            Ok(acc)
        };

        let collected: Vec<CandidateAccum<T::Stats>> = if self.parallel {
            actions.par_iter().map(run).collect::<Result<Vec<_>>>()?
        } else {
            actions.iter().map(run).collect::<Result<Vec<_>>>()?
        };

        Self::warn_failures(&collected, "均匀分配");
        Ok(collected)
    }

    /// UCB 动态分配搜索
    ///
    /// `search_value = value + cpuct × expected_stdev × sqrt(total_n) / n`。
    /// 分配决策是串行的（保确定性），每组模拟内部可并行。
    fn search_ucb<F, T>(
        &self,
        game: &G,
        actions: &[G::Action],
        radical_factor: f64,
        seeds: &RolloutSeeds,
        rollout: &F,
    ) -> Result<Vec<CandidateAccum<T::Stats>>>
    where
        T: TerminalRecord,
        F: Fn(&G, &G::Action, u64) -> Result<RolloutOutcome<T>> + Sync,
    {
        let num_actions = actions.len();
        let record = self.config.record_ordered_rollouts;
        ensure!(
            self.config.search_group_size > 0,
            "search_group_size 不能为 0（UCB 分配会死循环）"
        );
        // 首组不得越过 search_n：只收紧本函数的局部步长，不改配置对象
        let group_size = self.config.search_group_size.min(self.config.search_n).max(1);

        // 种子偏移用**计划**次数而非成功次数：后者会因失败而少计，导致同一 rollout
        // 序号在不同候选上错位，破坏配对
        let mut planned = vec![group_size; num_actions];

        let run_initial = |action: &G::Action| -> Result<CandidateAccum<T::Stats>> {
            let mut acc = CandidateAccum::<T::Stats>::new(record);
            self.simulate_many(game, action, group_size, seeds, 0, &mut acc, rollout, "")?;
            Ok(acc)
        };
        let mut collected: Vec<CandidateAccum<T::Stats>> = if self.parallel {
            actions.par_iter().map(run_initial).collect::<Result<Vec<_>>>()?
        } else {
            actions.iter().map(run_initial).collect::<Result<Vec<_>>>()?
        };

        let mut total_n = (group_size * num_actions) as f64;

        loop {
            // 终止判据用计划次数：用成功次数会在稳定失败时死循环
            let max_planned = planned.iter().copied().max().unwrap_or(0);
            if max_planned >= self.config.search_n {
                break;
            }
            let best_action_idx = self.select_ucb_action(&collected, radical_factor, total_n);
            let action = &actions[best_action_idx];
            // 该候选已计划 offset 次，本组取 seeds[offset..offset+group_size]：两个候选
            // 在 0..min(n_a, n_b) 上完全配对（不等样本数下 CRN 的标准做法）
            let offset = planned[best_action_idx];
            self.simulate_many(
                game,
                action,
                group_size,
                seeds,
                offset,
                &mut collected[best_action_idx],
                rollout,
                "[UCB]",
            )?;
            planned[best_action_idx] += group_size;
            total_n += group_size as f64;
        }

        Self::warn_failures(&collected, "UCB");
        Ok(collected)
    }

    /// 按 UCB 公式选下一个要搜索的候选（平局取最小下标，与历史行为一致）
    fn select_ucb_action<D>(&self, collected: &[CandidateAccum<D>], radical_factor: f64, total_n: f64) -> usize {
        let sqrt_total = total_n.sqrt();
        let cpuct = self.config.search_cpuct;
        let expected_stdev = self.config.expected_search_stdev;

        let mut best_idx = 0;
        let mut best_search_value = f64::NEG_INFINITY;
        for (i, acc) in collected.iter().enumerate() {
            let n = acc.score.count() as f64;
            if n == 0.0 {
                // 未搜索的动作优先级最高
                return i;
            }
            let value = acc.score.weighted_mean(radical_factor);
            let delta = cpuct * expected_stdev * sqrt_total / n;
            let search_value = value + delta;
            if search_value > best_search_value {
                best_search_value = search_value;
                best_idx = i;
            }
        }
        best_idx
    }

    /// 对同一候选执行 `n` 次 rollout，保序收集后累加进 `acc`
    ///
    /// 第 k 次取 `seeds.seed_at(offset + k)` 播种。并行收集必须保序、逐项累加：
    /// 顺序变化会改变浮点求和顺序，让所有历史基准失效。
    // 参数多但每个都是独立引用，聚成结构体只会在热路径上多一层间接；保持签名直观
    #[allow(clippy::too_many_arguments)]
    fn simulate_many<F, T>(
        &self,
        game: &G,
        action: &G::Action,
        n: usize,
        seeds: &RolloutSeeds,
        offset: usize,
        acc: &mut CandidateAccum<T::Stats>,
        rollout: &F,
        log_context: &str,
    ) -> Result<()>
    where
        T: TerminalRecord,
        F: Fn(&G, &G::Action, u64) -> Result<RolloutOutcome<T>> + Sync,
    {
        // 只在挂了探针时记账：关闭时这里是每组一次的分支，而非每条 rollout
        if self.probe.is_some() {
            acc.planned += n;
        }
        let run_one = |k: usize| rollout(game, action, seeds.seed_at(offset + k));
        let outcomes: Vec<Result<RolloutOutcome<T>>> = if self.parallel {
            (0..n).into_par_iter().map(run_one).collect()
        } else {
            (0..n).map(run_one).collect()
        };
        for (k, outcome) in outcomes.into_iter().enumerate() {
            let idx = offset + k;
            match outcome {
                Ok(v) => acc.push(idx, &v),
                Err(e) => {
                    if self.strict_rollout {
                        return Err(e).with_context(|| format!("rollout {idx} 失败（strict_rollout 已开启）"));
                    }
                    debug!("[搜索]{log_context} rollout {idx} 失败: {e}");
                    acc.failed += 1;
                    acc.ensure_ordered_slot(idx);
                }
            }
        }
        Ok(())
    }

    /// 汇总 rollout 失败次数并告警
    ///
    /// 不中断搜索（避免偶发失败拖垮实时通道层），但必须在日志里留下痕迹：
    /// 静默丢弃会把「跑失败」混同于「跑出来分低」。
    fn warn_failures<D>(collected: &[CandidateAccum<D>], stage: &str) {
        let total_failed: usize = collected.iter().map(|acc| acc.failed).sum();
        if total_failed > 0 {
            warn!("[搜索][{stage}] {total_failed} 次 rollout 失败，对应候选的样本数少于计划值");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use rand::RngCore;

    use super::*;
    use crate::game::Game;
    use crate::game::base::BaseAction;
    use crate::game::base::basic::{BasicAction, BasicGame};
    use crate::testutil;

    crate::define_terminal_record!(
        /// 测试用终局记录
        StubTerminal,
        /// 测试用终局统计
        StubTerminalStats {
            seed_low => { key: "seed_low", label: "种子低位", unit: "flag" },
        }
    );

    /// 桩基数：只由动作**身份**决定，与候选下标无关——候选顺序因此不影响统计
    fn stub_base(action: &BasicAction) -> f64 {
        match action.0 {
            BaseAction::Train(i) => 1000.0 * (i as f64 + 1.0),
            BaseAction::Race => 100.0,
            BaseAction::Sleep => 200.0,
            BaseAction::FriendOuting => 300.0,
            BaseAction::NormalOuting => 400.0,
            BaseAction::Clinic => 500.0,
        }
    }

    /// 桩分数：动作身份 + rollout 种子扰动，便于逐位核对 CRN 与统计
    fn stub_score(action: &BasicAction, seed: u64) -> f64 {
        stub_base(action) + (seed % 997) as f64
    }

    /// 用桩 rollout 跑一次搜索（串行，避免线程调度影响桩计数）
    fn run_stub(cfg: SearchConfig, seed: u64) -> Result<SearchOutput<BasicAction>> {
        let game = testutil::basic_game()?;
        let actions = game.list_actions()?;
        let mut rng = StdRng::seed_from_u64(seed);
        let fs = FlatSearch::<BasicGame>::new(cfg).with_parallel(false);
        fs.search_with(&game, &actions, &mut rng, |_g, a, s| Ok(stub_score(a, s)))
    }

    /// 层1：同种子可复现（中选下标与各候选统计逐项一致）
    #[test]
    fn test_search_reproducible() -> Result<()> {
        let cfg = SearchConfig::default().with_ucb(false).with_search_n(8);
        let a = run_stub(cfg.clone(), 7)?;
        let b = run_stub(cfg, 7)?;
        assert_eq!(a.best_action_idx, b.best_action_idx);
        assert_eq!(a.action_results.len(), b.action_results.len());
        for (x, y) in a.action_results.iter().zip(b.action_results.iter()) {
            assert_eq!(x.count(), y.count());
            assert_eq!(x.mean(), y.mean());
            assert_eq!(x.weighted_mean(a.radical_factor), y.weighted_mean(b.radical_factor));
        }
        Ok(())
    }

    /// 层1：换种子必须换到不同的随机未来（否则种子形同虚设）
    #[test]
    fn test_seed_actually_used() -> Result<()> {
        let cfg = SearchConfig::default().with_ucb(false).with_search_n(8);
        let a = run_stub(cfg.clone(), 1)?;
        let b = run_stub(cfg, 2)?;
        let changed = a
            .action_results
            .iter()
            .zip(b.action_results.iter())
            .any(|(x, y)| x.mean() != y.mean());
        assert!(changed, "桩分数依赖 rollout 种子，换入口种子后各候选均分不应完全相同");
        Ok(())
    }

    /// 层1：均匀分配下候选顺序不影响各动作统计（每候选独立取 seeds[0..n]）
    #[test]
    fn test_action_order_invariance() -> Result<()> {
        let cfg = SearchConfig::default().with_ucb(false).with_search_n(6);
        let game = testutil::basic_game()?;
        let actions = game.list_actions()?;
        let mut rev = actions.clone();
        rev.reverse();

        let run = |acts: &[BasicAction]| -> Result<SearchOutput<BasicAction>> {
            let mut rng = StdRng::seed_from_u64(11);
            let fs = FlatSearch::<BasicGame>::new(cfg.clone()).with_parallel(false);
            fs.search_with(&game, acts, &mut rng, |_g, a, s| Ok(stub_score(a, s)))
        };
        let fwd = run(&actions)?;
        let bwd = run(&rev)?;
        for (i, action) in actions.iter().enumerate() {
            let j = rev.iter().position(|x| x == action).unwrap_or(0);
            assert_eq!(
                fwd.action_results[i].mean(),
                bwd.action_results[j].mean(),
                "动作 {action:?} 的均分不应随候选顺序变化"
            );
        }
        assert_eq!(
            fwd.actions[fwd.best_action_idx], bwd.actions[bwd.best_action_idx],
            "中选动作不随候选顺序变化"
        );
        Ok(())
    }

    /// 层1：有序原始分按 rollout 序号对齐，且各候选共享同一张 CRN 种子表
    #[test]
    fn test_ordered_rollouts_align_with_crn() -> Result<()> {
        let cfg = SearchConfig::default()
            .with_ucb(false)
            .with_search_n(4)
            .with_record_ordered_rollouts(true);
        let game = testutil::basic_game()?;
        let actions = game.list_actions()?;
        let mut rng = StdRng::seed_from_u64(3);
        let fs = FlatSearch::<BasicGame>::new(cfg).with_parallel(false);
        let out = fs.search_with(&game, &actions, &mut rng, |_g, a, s| Ok(stub_score(a, s)))?;

        let ordered = out.ordered_rollouts.as_ref().expect("已开启有序原始分");
        let seeds = RolloutSeeds::from_root(ordered.root_seed);
        assert_eq!(ordered.per_candidate.len(), actions.len());
        for (i, row) in ordered.per_candidate.iter().enumerate() {
            assert_eq!(row.len(), 4);
            for (k, got) in row.iter().enumerate() {
                assert_eq!(
                    *got,
                    Some(stub_score(&actions[i], seeds.seed_at(k))),
                    "候选 {i} 第 {k} 次 rollout 的种子未与 CRN 对齐"
                );
            }
        }
        Ok(())
    }

    /// 层1：均匀分配的预算与探针计数（每候选恰好 search_n 次）
    #[test]
    fn test_uniform_budget_and_probe() -> Result<()> {
        let n = 5;
        let cfg = SearchConfig::default().with_ucb(false).with_search_n(n);
        let game = testutil::basic_game()?;
        let actions = game.list_actions()?;
        let sink = Arc::new(Mutex::new(Vec::new()));
        let mut rng = StdRng::seed_from_u64(21);
        let fs = FlatSearch::<BasicGame>::new(cfg)
            .with_parallel(false)
            .with_probe(sink.clone());
        let _out = fs.search_with(&game, &actions, &mut rng, |_g, a, s| Ok(stub_score(a, s)))?;

        let probes = sink.lock().expect("探针锁");
        assert_eq!(probes.len(), 1);
        let probe = &probes[0];
        assert!(probe.planned.iter().all(|&p| p == n));
        assert!(probe.succeeded.iter().all(|&s| s == n));
        assert!(probe.failed.iter().all(|&f| f == 0));
        assert_eq!(probe.total_planned(), n * actions.len());
        assert_eq!(probe.turn, game.turn() as usize);
        assert_eq!(probe.stage_key, game.crn_stage_key());
        Ok(())
    }

    /// 层1：UCB 按组分配（跨过 search_n 即停），且分配结果可复现
    ///
    /// 组大小 4、预算 10 → 首轮每候选 4 次，之后按 UCB 追加，直到某候选达到 12。
    #[test]
    fn test_ucb_budget_reproducible() -> Result<()> {
        let cfg = SearchConfig::default()
            .with_ucb(true)
            .with_search_n(10)
            .with_search_group_size(4);
        let game = testutil::basic_game()?;
        let actions = game.list_actions()?;

        let run = || -> Result<(SearchOutput<BasicAction>, Vec<usize>, Vec<usize>)> {
            let sink = Arc::new(Mutex::new(Vec::new()));
            let mut rng = StdRng::seed_from_u64(5);
            let fs = FlatSearch::<BasicGame>::new(cfg.clone())
                .with_parallel(false)
                .with_probe(sink.clone());
            let out = fs.search_with(&game, &actions, &mut rng, |_g, a, s| Ok(stub_score(a, s)))?;
            let probes = sink.lock().expect("探针锁");
            Ok((out, probes[0].planned.clone(), probes[0].succeeded.clone()))
        };
        let (out_a, planned_a, succeeded_a) = run()?;
        let (out_b, planned_b, _) = run()?;

        assert_eq!(planned_a, planned_b, "同种子下 UCB 分配必须可复现");
        assert!(planned_a.iter().all(|&p| p >= 4), "首轮每候选至少一组");
        assert_eq!(planned_a.iter().copied().max(), Some(12), "追加到跨过 search_n 为止");
        assert_eq!(planned_a, succeeded_a, "无失败时计划次数等于成功次数");
        assert_eq!(out_a.best_action_idx, out_b.best_action_idx);
        let total: usize = planned_a.iter().sum();
        assert!(total > 4 * actions.len(), "UCB 应至少追加过一组");
        Ok(())
    }

    /// 层1：`strict_rollout` 让失败显式报错；默认路径只计数不中断
    #[test]
    fn test_strict_rollout() -> Result<()> {
        let cfg = SearchConfig::default().with_ucb(false).with_search_n(6);
        let game = testutil::basic_game()?;
        let actions = game.list_actions()?;

        let run = |strict: bool| -> (Result<SearchOutput<BasicAction>>, usize) {
            let fails = AtomicUsize::new(0);
            let mut rng = StdRng::seed_from_u64(9);
            let fs = FlatSearch::<BasicGame>::new(cfg.clone())
                .with_parallel(false)
                .with_strict_rollout(strict);
            let out = fs.search_with(&game, &actions, &mut rng, |_g, a, s| {
                let calls = fails.fetch_add(1, Ordering::Relaxed);
                if calls.is_multiple_of(3) {
                    bail!("桩 rollout 失败");
                }
                Ok(stub_score(a, s))
            });
            (out, fails.load(Ordering::Relaxed))
        };

        let (strict_out, strict_fails) = run(true);
        assert!(strict_fails > 0, "桩必须真的失败过，否则本用例无意义");
        assert!(strict_out.is_err(), "strict_rollout 打开时失败必须冒泡");

        let (lenient_out, lenient_fails) = run(false);
        assert!(lenient_fails > 0);
        let out = lenient_out?;
        // 每候选 6 次调用中第 0、3 次失败 → 成功 4 次，样本数少于计划值但不中断
        assert!(out.action_results.iter().all(|r| r.count() == 4));
        Ok(())
    }

    /// 层1：并行与串行的结果逐位一致（保序收集 + 每条 rollout 独立种子）
    #[test]
    fn test_parallel_matches_serial() -> Result<()> {
        let cfg = SearchConfig::default().with_ucb(false).with_search_n(16);
        let game = testutil::basic_game()?;
        let actions = game.list_actions()?;

        let run = |parallel: bool| -> Result<SearchOutput<BasicAction>> {
            let mut rng = StdRng::seed_from_u64(13);
            let fs = FlatSearch::<BasicGame>::new(cfg.clone()).with_parallel(parallel);
            fs.search_with(&game, &actions, &mut rng, |_g, a, s| Ok(stub_score(a, s)))
        };
        let par = run(true)?;
        let ser = run(false)?;
        assert_eq!(par.best_action_idx, ser.best_action_idx);
        for (x, y) in par.action_results.iter().zip(ser.action_results.iter()) {
            assert_eq!(x.mean(), y.mean());
            assert_eq!(x.stdev(), y.stdev());
        }
        Ok(())
    }

    /// 层1：剧本未声明支持截断估值时，`max_depth > 0` 在入口被拒
    #[test]
    fn test_max_depth_rejected() -> Result<()> {
        let cfg = SearchConfig::default()
            .with_ucb(false)
            .with_search_n(2)
            .with_max_depth(1);
        let game = testutil::basic_game()?;
        let actions = game.list_actions()?;
        let mut rng = StdRng::seed_from_u64(1);
        let fs = FlatSearch::<BasicGame>::new(cfg).with_parallel(false);
        let out = fs.search_with(&game, &actions, &mut rng, |_g, _a, _s| Ok(0.0));
        assert!(out.is_err(), "基础局不支持截断估值，应直接报错而非静默降级");
        Ok(())
    }

    /// 层1：空候选直接报错
    #[test]
    fn test_empty_actions_rejected() -> Result<()> {
        let game = testutil::basic_game()?;
        let mut rng = StdRng::seed_from_u64(1);
        let fs = FlatSearch::<BasicGame>::new(SearchConfig::default());
        let out = fs.search_with(&game, &[], &mut rng, |_g, _a, _s| Ok(0.0));
        assert!(out.is_err());
        Ok(())
    }

    /// 层1：终局多维记录随评分一起累加（不参与排序），均值与 CRN 种子表一致
    #[test]
    fn test_terminal_stats_accumulate() -> Result<()> {
        let cfg = SearchConfig::default().with_ucb(false).with_search_n(4);
        let game = testutil::basic_game()?;
        let actions = game.list_actions()?;
        let mut rng = StdRng::seed_from_u64(17);
        let fs = FlatSearch::<BasicGame>::new(cfg).with_parallel(false);
        let out = fs.search_with_terminal(&game, &actions, &mut rng, |_g, a, s| {
            Ok(RolloutOutcome {
                score: stub_score(a, s),
                terminal: StubTerminal { seed_low: (s % 97) as f64 },
            })
        })?;

        // 根种子是入口 rng 的第一次抽样，测试里可独立复算
        let mut entry = StdRng::seed_from_u64(17);
        let seeds = RolloutSeeds::from_root(entry.next_u64());
        let expect: f64 = (0..4).map(|k| (seeds.seed_at(k) % 97) as f64).sum::<f64>() / 4.0;

        assert_eq!(out.terminal_results.len(), actions.len());
        assert_eq!(out.terminal_results[0].seed_low.count(), 4);
        assert_eq!(out.terminal_results[0].seed_low.mean(), expect);
        Ok(())
    }

    /// 层1：真实 rollout 同种子逐位可复现（基础局 + RandomTrainer）
    #[test]
    fn test_simulate_common_reproducible() -> Result<()> {
        let (game, actions) = testutil::decision_root()?;
        let fs = FlatSearch::<BasicGame>::default_search();
        let first = fs.simulate_common(&game, &actions[0], 42)?;
        let second = fs.simulate_common(&game, &actions[0], 42)?;
        assert_eq!(first, second, "同种子同动作的 rollout 必须逐位可复现");
        assert!(first.is_finite());
        Ok(())
    }
}
