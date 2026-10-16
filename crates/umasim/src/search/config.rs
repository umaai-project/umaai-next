//! 搜索配置
//!
//! 扁平蒙特卡洛搜索的参数。默认值沿用既有调参基线，入口层（`umaai`）按配置覆盖；
//! 搜索内核只读取本结构，不读任何全局状态。

/// 搜索配置
///
/// 字段分三类：预算（`search_n` / `max_depth` / `group_size`）、评分口径
/// （`radical_factor_max` / `pt_favor_rate`）、机制开关（UCB / 外挂 CRN / 有序原始分）。
#[derive(Debug, Clone)]
pub struct SearchConfig {
    /// 每个动作的搜索次数（搜索越多越准，越慢）
    pub search_n: usize,

    /// 最大搜索深度（回合数）
    ///
    /// `0` = 搜到终局（推荐）。`> 0` 需要剧本提供 leaf 估值器，内核在入口显式拒绝
    /// 未声明支持的剧本，不静默降级为终局搜索。
    pub max_depth: usize,

    /// 激进度因子上限
    ///
    /// 每回合按剩余回合**确定性衰减**：`(剩余回合 / 总回合)^0.5 × 本值`。
    /// 越大越倾向高分高风险动作（`weighted_mean` 的排名加权指数越大越偏高分尾部）。
    pub radical_factor_max: f64,

    /// 是否启用 UCB 动态分配（`false` = 各候选均匀分配）
    pub use_ucb: bool,

    /// UCB 每次给选中动作追加的搜索次数
    pub search_group_size: usize,

    /// UCB 探索常数（`cpuct`）
    ///
    /// `search_value = value + cpuct × expected_stdev × sqrt(total_n) / n`，越大越倾向
    /// 探索搜索次数少的动作。与 `expected_search_stdev` 以乘积出现，两者只需同定其积。
    pub search_cpuct: f64,

    /// 探索项使用的预期分数标准差（分数量纲）
    pub expected_search_stdev: f64,

    /// 是否按 `(回合, 阶段)` 重播种 rollout 随机流（外挂 CRN 开关）
    ///
    /// 只对**规则层未接管随机流**的剧本有意义，见 `FlatSearch::reseed_for_stage`。
    pub crn_stage_reseed: bool,

    /// 是否按 rollout 序号保留有序原始分（教师数据采集用）
    ///
    /// 默认关闭：生产路径不分配缓冲、不写 [`ordered_rollouts`](super::SearchOutput::ordered_rollouts)。
    /// 开启后失败序号记 `None`，不压缩槽位——否则后续元素会与其他候选的 CRN 配对错位。
    pub record_ordered_rollouts: bool,

    /// PT 偏好折算率（搜索排序口径）
    ///
    /// 搜索评分的 PT 项乘本值：`1.0` = 不偏 PT，`2.0` = 既有定档。归配置层，
    /// 剧本用自身注入的数值构造，内核不读全局配置。
    pub pt_favor_rate: f64,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            search_n: 1024,
            max_depth: 0,
            radical_factor_max: 50.0,
            use_ucb: true,
            search_group_size: 256,
            search_cpuct: 1.0,
            expected_search_stdev: 2200.0,
            crn_stage_reseed: true,
            record_ordered_rollouts: false,
            pt_favor_rate: 2.0,
        }
    }
}

impl SearchConfig {
    /// 设置每个动作的搜索次数
    pub fn with_search_n(mut self, n: usize) -> Self {
        self.search_n = n;
        self
    }

    /// 设置最大搜索深度（`0` = 搜到终局）
    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }

    /// 设置激进度因子上限
    pub fn with_radical_factor_max(mut self, max: f64) -> Self {
        self.radical_factor_max = max;
        self
    }

    /// 启用 / 禁用 UCB 动态分配
    pub fn with_ucb(mut self, enabled: bool) -> Self {
        self.use_ucb = enabled;
        self
    }

    /// 设置 UCB 每组搜索次数
    pub fn with_search_group_size(mut self, size: usize) -> Self {
        self.search_group_size = size;
        self
    }

    /// 设置 UCB 探索常数
    pub fn with_search_cpuct(mut self, cpuct: f64) -> Self {
        self.search_cpuct = cpuct;
        self
    }

    /// 设置探索项使用的预期分数标准差
    pub fn with_expected_search_stdev(mut self, stdev: f64) -> Self {
        self.expected_search_stdev = stdev;
        self
    }

    /// 启用 / 禁用按 `(回合, 阶段)` 重播种（外挂 CRN）
    pub fn with_crn_stage_reseed(mut self, enabled: bool) -> Self {
        self.crn_stage_reseed = enabled;
        self
    }

    /// 启用 / 禁用按 rollout 序号保留有序原始分
    pub fn with_record_ordered_rollouts(mut self, enabled: bool) -> Self {
        self.record_ordered_rollouts = enabled;
        self
    }

    /// 设置 PT 偏好折算率
    pub fn with_pt_favor_rate(mut self, rate: f64) -> Self {
        self.pt_favor_rate = rate;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：默认值锁定既有基线（改动即视为口径变更）
    #[test]
    fn test_defaults() {
        let c = SearchConfig::default();
        assert_eq!(c.search_n, 1024);
        assert_eq!(c.max_depth, 0);
        assert!(c.use_ucb);
        assert_eq!(c.search_group_size, 256);
        assert_eq!(c.expected_search_stdev, 2200.0);
        assert_eq!(c.pt_favor_rate, 2.0);
        assert!(!c.record_ordered_rollouts);
    }

    /// 层1：builder 逐字段生效
    #[test]
    fn test_builders() {
        let c = SearchConfig::default()
            .with_search_n(8)
            .with_max_depth(0)
            .with_ucb(false)
            .with_search_group_size(2)
            .with_radical_factor_max(10.0)
            .with_pt_favor_rate(1.0)
            .with_crn_stage_reseed(false)
            .with_record_ordered_rollouts(true);
        assert_eq!(c.search_n, 8);
        assert!(!c.use_ucb);
        assert_eq!(c.search_group_size, 2);
        assert_eq!(c.radical_factor_max, 10.0);
        assert_eq!(c.pt_favor_rate, 1.0);
        assert!(!c.crn_stage_reseed);
        assert!(c.record_ordered_rollouts);
    }
}
