//! 比赛回合位段
//!
//! 位置位段：bit i ↔ 回合 `MIN_TURN + i`（窗口 `11..=74`，恰好 64 位）。
//! 生涯比赛表、自由比赛区间与局面比赛记录共用同一约定，避免各处手写移位与魔法值。

use serde::{Deserialize, Serialize};

use crate::{UmaError, UmaResult};

/// 比赛回合位段
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RaceMask(u64);

impl RaceMask {
    /// 窗口下界（含）
    pub const MIN_TURN: i32 = 11;
    /// 窗口上界（含）
    pub const MAX_TURN: i32 = 74;
    /// 空位段
    pub const EMPTY: Self = Self(0);

    /// 由回合列表构造；回合超出窗口时报错
    pub fn from_turns(turns: &[i32]) -> UmaResult<Self> {
        let mut mask = Self::EMPTY;
        for &turn in turns {
            mask.insert_turn(turn)?;
        }
        Ok(mask)
    }

    /// 置位一个回合；回合超出窗口时报错
    pub fn insert_turn(&mut self, turn: i32) -> UmaResult<()> {
        self.0 |= 1u64 << Self::bit_of(turn)?;
        Ok(())
    }

    /// 是否包含某回合；回合超出窗口时返回 `false`
    pub fn contains_turn(self, turn: i32) -> bool {
        Self::bit_of(turn).is_ok_and(|bit| self.0 & (1u64 << bit) != 0)
    }

    /// 已置位的回合数
    pub fn count(self) -> u32 {
        self.0.count_ones()
    }

    /// 是否为空位段
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// 裸位段（与协议 / JSON 互转、区间裁剪用）
    pub fn bits(self) -> u64 {
        self.0
    }

    /// 由裸位段构造
    pub fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    /// 回合 → 位序号（0 基）；回合超出窗口报错
    fn bit_of(turn: i32) -> UmaResult<u32> {
        if (Self::MIN_TURN..=Self::MAX_TURN).contains(&turn) {
            Ok((turn - Self::MIN_TURN) as u32)
        } else {
            Err(UmaError::Validation {
                target: format!("raceMask/turn{turn}"),
                detail: format!("回合超出 {}..={}", Self::MIN_TURN, Self::MAX_TURN),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：置位 / 查询 / 计数 / 裸位段往返
    #[test]
    fn test_basic() {
        let mask = RaceMask::from_turns(&[11, 26, 71]).unwrap();
        assert_eq!(mask.count(), 3);
        assert!(mask.contains_turn(11) && mask.contains_turn(26) && mask.contains_turn(71));
        assert!(!mask.contains_turn(12));
        assert_eq!(RaceMask::from_bits(mask.bits()), mask);
        assert!(!mask.is_empty());
        assert!(RaceMask::EMPTY.is_empty());
        assert_eq!(mask.bits(), (1u64 << 0) | (1u64 << 15) | (1u64 << 60));
    }

    /// 层1：重复回合只置一位
    #[test]
    fn test_dedup() {
        assert_eq!(RaceMask::from_turns(&[20, 20, 20]).unwrap().count(), 1);
    }

    /// 层1：窗口越界报错，查询越界返回 false
    #[test]
    fn test_window() {
        assert!(RaceMask::from_turns(&[RaceMask::MIN_TURN]).is_ok());
        assert!(RaceMask::from_turns(&[RaceMask::MAX_TURN]).is_ok());
        assert!(RaceMask::from_turns(&[RaceMask::MIN_TURN - 1]).is_err());
        assert!(RaceMask::from_turns(&[RaceMask::MAX_TURN + 1]).is_err());
        let mut m = RaceMask::EMPTY;
        assert!(m.insert_turn(0).is_err());
        assert!(!m.contains_turn(9999), "查询越界应为 false");
    }

    /// 层1：serde 透明（与协议 / JSON 的裸位段一致）
    #[test]
    fn test_serde_transparent() {
        let mask = RaceMask::from_turns(&[11, 12]).unwrap();
        let text = serde_json::to_string(&mask).unwrap();
        assert_eq!(text, "3");
        assert_eq!(serde_json::from_str::<RaceMask>("3").unwrap(), mask);
        assert_eq!(serde_json::from_str::<RaceMask>("0").unwrap(), RaceMask::EMPTY);
    }
}
