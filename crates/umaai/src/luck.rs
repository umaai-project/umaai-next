//! 运气分：把搜索期望分换算成显示分，并给出逐回合波动
//!
//! 口径（沿用旧实现的行为基准）：
//!
//! - **显示分换算**：`display = raw + (max_turn − turn) × bonus`；初始基线恒按
//!   `turn = 0` 换算（与「首次出现在哪个回合」无关），当前基线按实际回合换算。
//! - **回合运气** = 当前显示分 − 上一回合显示分；**总运气** = 当前显示分 − 初始显示分。
//! - **切局**按 `epoch_key` 判定：键变化或首次记录即整体重置（总运气归零、回合运气为空）。
//!
//! 与旧实现的两处刻意差异：
//!
//! 1. **回合一律 0 基**。旧实现在下发视图里用 1 基回合、而 `raw → display` 反推用 0 基，
//!    使反推出的原始分系统性偏一个 `bonus`；本模块只保留一种基，[`to_display`] 与
//!    [`raw_from_display`] 严格互逆。
//! 2. **`bonus` 由调用方注入**，不读全局常量——同一进程内不同配置/测试可各自取值。

use serde::{Deserialize, Serialize};

/// 一次运气分快照
///
/// 四个字段始终齐全（未初始化时给零值 / `None`），序列化后下游拿到的键不会缺项。
/// 字段名沿用旧实现的在线输出契约，便于下游沿用既有解析。
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct LuckScoreSnapshot {
    /// T(1) 的显示分：本局首次决策基线按 `turn = 0` 换算后的值
    pub initial_terminal_baseline: f64,
    /// T(n+1) 的显示分：最近一次记录基线按实际回合换算后的值
    pub current_terminal_baseline: f64,
    /// T(n+1) − T(1)（显示分口径）：本局累计运气
    pub total_luck_score: f64,
    /// T(n+1) − T(n)（显示分口径）：本回合运气；首次 / 切局后为 `None`
    pub last_turn_delta: Option<f64>,
}

/// 原始分 → 显示分（`turn` 为 0 基）
pub fn to_display(raw: f64, turn: i32, max_turn: i32, bonus: i32) -> f64 {
    raw + (max_turn - turn) as f64 * bonus as f64
}

/// 显示分 → 原始分（与 [`to_display`] 严格互逆，`turn` 为 0 基）
pub fn raw_from_display(display: f64, turn: i32, max_turn: i32, bonus: i32) -> f64 {
    display - (max_turn - turn) as f64 * bonus as f64
}

/// 运气分跟踪器（按切局键重置的状态机）
#[derive(Debug, Clone, Default)]
pub struct LuckScoreTracker {
    epoch_key: Option<u64>,
    initial_display: Option<f64>,
    last_display: Option<f64>,
    total_luck: f64,
    last_turn_delta: Option<f64>,
}

impl LuckScoreTracker {
    /// 新建空跟踪器（首回合 / 切局后首次记录返回 `None`）
    pub fn new() -> Self {
        Self::default()
    }

    /// 当前已记录的切局键（`None` 表示尚未记录过任何一局）
    pub fn epoch_key(&self) -> Option<u64> {
        self.epoch_key
    }

    /// 记录一个新回合的基线（原始分口径），返回本回合运气；首次 / 切局后为 `None`
    ///
    /// 切局判定：`epoch_key` 变化或尚未记录初始基线。切局时初始基线按 `turn = 0` 换算，
    /// 当前基线按实际回合换算，总运气清零。
    pub fn on_new_turn(
        &mut self,
        epoch_key: u64,
        t_n_baseline: f64,
        turn: i32,
        max_turn: i32,
        bonus: i32,
    ) -> Option<f64> {
        let current = to_display(t_n_baseline, turn, max_turn, bonus);
        if self.epoch_key != Some(epoch_key) || self.initial_display.is_none() {
            self.epoch_key = Some(epoch_key);
            self.initial_display = Some(to_display(t_n_baseline, 0, max_turn, bonus));
            self.last_display = Some(current);
            self.total_luck = 0.0;
            self.last_turn_delta = None;
            return None;
        }
        let delta = self.last_display.map(|prev| current - prev)?;
        self.total_luck = current - self.initial_display.unwrap_or(current);
        self.last_turn_delta = Some(delta);
        self.last_display = Some(current);
        Some(delta)
    }

    /// 拍快照（字段齐全；未初始化时为零值 / `None`）
    pub fn snapshot(&self) -> LuckScoreSnapshot {
        LuckScoreSnapshot {
            initial_terminal_baseline: self.initial_display.unwrap_or(0.0),
            current_terminal_baseline: self.last_display.unwrap_or(0.0),
            total_luck_score: self.total_luck,
            last_turn_delta: self.last_turn_delta,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：首次记录返回 `None`，初始基线恒按回合 0 换算
    #[test]
    fn test_first_turn() {
        let mut t = LuckScoreTracker::new();
        // bonus=1, max_turn=78, turn=5：current = 50000+73 = 50073；initial = 50000+78 = 50078
        assert_eq!(t.on_new_turn(42, 50000.0, 5, 78, 1), None);
        let snap = t.snapshot();
        assert_eq!(snap.initial_terminal_baseline, 50078.0);
        assert_eq!(snap.current_terminal_baseline, 50073.0);
        assert_eq!(snap.total_luck_score, 0.0);
        assert_eq!(snap.last_turn_delta, None);
        assert_eq!(t.epoch_key(), Some(42));
    }

    /// 层1：两回合累加（显示分口径）
    #[test]
    fn test_two_turns() {
        let mut t = LuckScoreTracker::new();
        t.on_new_turn(42, 50000.0, 0, 78, 1);
        // turn=1：current = 50150+77 = 50227；delta = 50227−50078 = 149
        assert_eq!(t.on_new_turn(42, 50150.0, 1, 78, 1), Some(149.0));
        let snap = t.snapshot();
        assert_eq!(snap.total_luck_score, 149.0);
        assert_eq!(snap.last_turn_delta, Some(149.0));
    }

    /// 层1：多回合累加，总运气 = 末次显示分 − 初始显示分
    #[test]
    fn test_multi_turns() {
        let mut t = LuckScoreTracker::new();
        let raws = [50000.0, 50150.0, 50320.0, 50400.0, 50280.0];
        for (i, raw) in raws.iter().enumerate() {
            let turn = i as i32;
            let delta = t.on_new_turn(42, *raw, turn, 78, 1);
            assert_eq!(delta.is_none(), i == 0, "仅首回合为空");
        }
        let snap = t.snapshot();
        assert_eq!(snap.initial_terminal_baseline, 50078.0);
        assert_eq!(snap.current_terminal_baseline, 50354.0);
        assert_eq!(snap.total_luck_score, 276.0);
        assert_eq!(snap.last_turn_delta, Some(-121.0));
    }

    /// 层1：切局重置（总运气清零、回合运气清空）
    #[test]
    fn test_epoch_change_resets() {
        let mut t = LuckScoreTracker::new();
        t.on_new_turn(42, 50000.0, 0, 78, 1);
        t.on_new_turn(42, 50150.0, 1, 78, 1);
        assert_eq!(t.snapshot().total_luck_score, 149.0);

        assert_eq!(t.on_new_turn(99, 48000.0, 1, 78, 1), None, "切局后首回合为空");
        let snap = t.snapshot();
        assert_eq!(t.epoch_key(), Some(99));
        assert_eq!(snap.initial_terminal_baseline, 48078.0);
        assert_eq!(snap.current_terminal_baseline, 48077.0);
        assert_eq!(snap.total_luck_score, 0.0);
        assert_eq!(snap.last_turn_delta, None);
    }

    /// 层1：`bonus = 0` 退化为原始分
    #[test]
    fn test_zero_bonus() {
        let mut t = LuckScoreTracker::new();
        t.on_new_turn(42, 50000.0, 3, 78, 0);
        let snap = t.snapshot();
        assert_eq!(snap.initial_terminal_baseline, 50000.0);
        assert_eq!(snap.current_terminal_baseline, 50000.0);
    }

    /// 层1：显示分换算严格互逆（同一回合基）
    #[test]
    fn test_display_round_trip() {
        for (turn, max_turn, bonus) in [(0, 78, 70), (5, 78, 1), (77, 78, 70), (3, 78, 0)] {
            let raw = 51_234.5;
            let display = to_display(raw, turn, max_turn, bonus);
            assert_eq!(raw_from_display(display, turn, max_turn, bonus), raw);
        }
    }

    /// 层1：快照序列化后四个键齐全
    #[test]
    fn test_snapshot_keys() {
        let snap = LuckScoreTracker::new().snapshot();
        let json = serde_json::to_string(&snap).expect("serialize");
        for key in [
            "initial_terminal_baseline",
            "current_terminal_baseline",
            "total_luck_score",
            "last_turn_delta",
        ] {
            assert!(json.contains(key), "缺键 {key}: {json}");
        }
    }
}
