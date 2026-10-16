//! 终局多维记录
//!
//! 搜索排序只用单一评分标量；本模块负责**不参与排序**的终局观测量，把「选速度，因为
//! 均分 56712」变成「因为最终智力高 300、第三年剧本 PT 多 187」。
//!
//! 两条硬约束：
//!
//! 1. **不复用带直方图的累加分布**：那条路每实例分配十万级直方图（约 400 KB），20 余维
//!    乘候选数会打穿热路径；加权均值对「距上限还差多少」这类量纲也毫无意义。
//! 2. **非线性量必须在 rollout 内部算完**：逐维 `sum` / `sum_sq` 只能重建线性量，而
//!    「PT 是否达到 1500」「五维最差缺口」是对局面的非线性归约，事后重建会说谎。
//!
//! 类型关系：`TerminalRecord`（一次 rollout 的定长事实）→ `TerminalStats`（按候选累加）
//! → `NamedMetricRef`（`visit` 出口，名称与数值绑定）；未接观测的剧本用 `NoTerminal*`。

use std::fmt::Debug;

use umaai_core::stats::Moments;

/// 一个已与名称、标签、量纲绑定的统计维度引用
///
/// 消费方拿到的是「名字和数值已经配好对」的整体，不存在按下标去平行名表里
/// 取名字这一步——那正是维度增删后静默错位的来源。
#[derive(Debug, Clone, Copy)]
pub struct NamedMetricRef<'a> {
    /// 稳定的机器可读键（用于 CSV 表头、NN 编码器取字段）
    pub key: &'static str,
    /// 人类可读标签
    pub label: &'static str,
    /// 量纲（`score` / `status` / `pt` / `flag`）
    pub unit: &'static str,
    /// 对应统计值
    pub result: &'a Moments,
}

/// 一个剧本的终局统计集合（按候选累加的结果）
pub trait TerminalStats: Debug + Clone + Default + Send + Sync + 'static {
    /// 逐项访问「名称与数值已绑定」的统计维度
    fn visit(&self, visitor: &mut dyn FnMut(NamedMetricRef<'_>));

    /// 维度个数
    fn dim_count(&self) -> usize {
        let mut n = 0;
        self.visit(&mut |_| n += 1);
        n
    }
}

/// 一次 rollout 产生的定长终局记录
///
/// `Copy` + 固定字段：热路径上不允许每次 rollout 做堆分配。
pub trait TerminalRecord: Debug + Clone + Copy + Send + Sync + 'static {
    /// 对应的逐候选累加器
    type Stats: TerminalStats;

    /// 把本次终局事实并入统计
    fn accumulate_into(&self, stats: &mut Self::Stats);
}

/// 空终局记录（尚未接入观测的剧本）
///
/// ZST：单态化后 [`RolloutOutcome`] 与评分标量同尺寸，累加是空操作。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoTerminal;

/// 空终局统计
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoTerminalStats;

impl TerminalRecord for NoTerminal {
    type Stats = NoTerminalStats;

    fn accumulate_into(&self, _stats: &mut Self::Stats) {}
}

impl TerminalStats for NoTerminalStats {
    fn visit(&self, _visitor: &mut dyn FnMut(NamedMetricRef<'_>)) {}
}

/// 一次 rollout 的主评分与终局记录
///
/// 终局局面只存在于 rollout 内部，跑完即弃，故多维事实必须在这里随评分一起
/// 返回——事后无法补取。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RolloutOutcome<T> {
    /// 参与搜索排序的评分（口径由剧本的搜索评分钩子给定，含 PT 偏好折算）
    pub score: f64,
    /// 不参与排序的终局观测记录
    pub terminal: T,
}

/// 从单一字段清单同时生成终局记录、累加器与名称绑定
///
/// 字段名、键、标签、量纲写在同一处，因而不存在「平行名表与数值顺序错位」这类
/// 静默 bug：增删或重排维度只改这一份声明。
///
/// 注意它守不住的东西：某个键是否误取了另一维的值，属于领域映射错误，
/// 只能靠构造侧的单元测试兜底。
///
/// 用法（剧本侧）：
///
/// ```ignore
/// umasim::define_terminal_record!(
///     /// 终局记录
///     RamenTerminal,
///     /// 终局统计
///     RamenTerminalStats {
///         speed => { key: "speed", label: "速度", unit: "status" },
///         pt => { key: "pt", label: "PT", unit: "pt" },
///     }
/// );
/// ```
#[macro_export]
macro_rules! define_terminal_record {
    (
        $(#[$record_meta:meta])*
        $record:ident,
        $(#[$stats_meta:meta])*
        $stats:ident {
            $(
                $field:ident => { key: $key:literal, label: $label:literal, unit: $unit:literal }
            ),+ $(,)?
        }
    ) => {
        $(#[$record_meta])*
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct $record {
            $(
                #[doc = $label]
                pub $field: f64
            ),+
        }

        $(#[$stats_meta])*
        #[derive(Debug, Clone, Copy, Default, PartialEq)]
        pub struct $stats {
            $(
                #[doc = $label]
                pub $field: $crate::search::Moments
            ),+
        }

        impl $crate::search::TerminalRecord for $record {
            type Stats = $stats;

            fn accumulate_into(&self, stats: &mut Self::Stats) {
                $(stats.$field.add(self.$field);)+
            }
        }

        impl $crate::search::TerminalStats for $stats {
            fn visit(&self, visitor: &mut dyn FnMut($crate::search::NamedMetricRef<'_>)) {
                $(
                    visitor($crate::search::NamedMetricRef {
                        key: $key,
                        label: $label,
                        unit: $unit,
                        result: &self.$field
                    });
                )+
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::define_terminal_record!(
        /// 测试用终局记录
        SampleRecord,
        /// 测试用终局统计
        SampleStats {
            speed => { key: "speed", label: "速度", unit: "status" },
            pt => { key: "pt", label: "PT", unit: "pt" },
        }
    );

    /// 层1：记录并入统计后按维度对齐，且名称与数值同源绑定
    #[test]
    fn test_record_accumulate_and_visit() {
        let mut stats = SampleStats::default();
        for (speed, pt) in [(100.0, 10.0), (300.0, 30.0)] {
            SampleRecord { speed, pt }.accumulate_into(&mut stats);
        }
        assert_eq!(stats.speed.count(), 2);
        assert_eq!(stats.speed.mean(), 200.0);
        assert_eq!(stats.pt.mean(), 20.0);
        assert_eq!(stats.dim_count(), 2);

        let mut seen = Vec::new();
        stats.visit(&mut |m| seen.push((m.key, m.label, m.unit, m.result.mean())));
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0], ("speed", "速度", "status", 200.0));
        assert_eq!(seen[1], ("pt", "PT", "pt", 20.0));
    }

    /// 层1：空终局记录是 ZST，且不改变评分标量的尺寸
    #[test]
    fn test_no_terminal_is_zst() {
        use std::mem::size_of;

        assert_eq!(size_of::<NoTerminal>(), 0);
        assert_eq!(size_of::<NoTerminalStats>(), 0);
        assert_eq!(size_of::<RolloutOutcome<NoTerminal>>(), size_of::<f64>());

        let mut stats = NoTerminalStats;
        NoTerminal.accumulate_into(&mut stats);
        assert_eq!(stats.dim_count(), 0);
    }
}
