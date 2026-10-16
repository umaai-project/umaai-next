//! 继承因子（种马因子），剧本通用

use rand::Rng;
use serde::{Deserialize, Serialize};
use umaai_core::{Array5, Array6};

/// 继承信息，剧本通用
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InheritInfo {
    /// 蓝因子数量
    pub blue_count: Array5,
    /// 剧本因子属性
    pub extra_count: Array6,
}

impl InheritInfo {
    /// 获取开局属性（蓝因子 × 7）
    pub fn inherit_newgame(&self) -> Array5 {
        let mut ret = self.blue_count;
        for i in 0..5 {
            ret[i] *= 7;
        }
        ret
    }

    /// 获取局中继承属性（属性 = 蓝因子×6 + 剧本因子×scale；pt 折半起）
    pub fn inherit(&self, rng: &mut impl Rng) -> Array6 {
        let mut ret = Array6::default();
        let extra_scale: f64 = rng.random_range(0.0..2.0);
        for i in 0..5 {
            ret[i] = self.blue_count[i] * 6 + (extra_scale * self.extra_count[i] as f64).round() as i32;
        }
        // pt
        ret[5] = (self.extra_count[5] as f64 * (0.5 + 0.5 * extra_scale)).round() as i32;
        ret
    }

    /// 获取开局继承上限（3星蓝因子提供 16 上限）
    pub fn inherit_limit_newgame(&self) -> Array5 {
        let mut ret = self.blue_count;
        for i in 0..5 {
            ret[i] = (ret[i] as f32 * 5.33) as i32;
        }
        ret
    }

    /// 获取局中继承上限 0-8（与开局上限同式）
    pub fn inherit_limit(&self, _rng: &mut impl Rng) -> Array5 {
        let mut ret = self.blue_count;
        for i in 0..5 {
            ret[i] = (ret[i] as f32 * 5.33) as i32;
        }
        ret
    }
}

#[cfg(test)]
mod tests {
    use rand::{SeedableRng, rngs::StdRng};

    use super::*;

    /// 层1：继承属性与上限公式（固定种子）
    #[test]
    fn test_inherit() {
        let ii = InheritInfo {
            blue_count: [15, 3, 0, 0, 0],
            extra_count: [0, 30, 0, 0, 30, 40],
        };
        assert_eq!(ii.inherit_newgame(), [105, 21, 0, 0, 0]);
        assert_eq!(ii.inherit_limit_newgame(), [79, 15, 0, 0, 0]);

        let mut rng = StdRng::seed_from_u64(42);
        let first = ii.inherit(&mut rng);
        let second = ii.inherit(&mut rng);
        // 属性 = 蓝×6 + 剧本×scale（scale ∈ [0,2)），上限同式
        assert_eq!(first[0], 90);
        println!("first: {first:?} second: {second:?}");
        // 两次采样不同（extra_scale 随机）
        assert_ne!(first, second);
    }
}
