//! 语义着色：调用方给语义，不给颜色
//!
//! 关闭着色时所有方法退化为原样返回，调用方不需要为「有没有颜色」分支。
//! 编译期 feature `color` 关闭时，本模块不含任何颜色代码。

use crate::render::term;

/// 语义色调
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// 常规文本
    Normal,
    /// 正向（变好、增益）
    Good,
    /// 提示（偏低但未告警）
    Warn,
    /// 告警（危险、减益）
    Bad,
    /// 强调（标题、标识）
    Accent,
    /// 弱化（次要信息）
    Muted,
}

/// 上色器：持一个颜色开关，剧本侧可按自己的配置构造
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Styler {
    color: bool,
}

impl Styler {
    /// 按开关构造
    #[must_use]
    pub fn new(color: bool) -> Self {
        Self { color }
    }

    /// 按终端三重门禁构造（见 [`crate::render::term::use_color`]）
    #[must_use]
    pub fn from_term(no_color_flag: bool) -> Self {
        Self::new(term::use_color(no_color_flag))
    }

    /// 是否着色
    #[must_use]
    pub fn color(self) -> bool {
        self.color
    }

    /// 按色调上色（关闭时原样返回）
    #[must_use]
    pub fn paint(self, tone: Tone, text: &str) -> String {
        if !self.color {
            return text.to_string();
        }
        paint_color(tone, text)
    }
}

/// 体力色调：按上限归一后 `<35%` 告警、`<50%` 提示（旧实现口径，上限 100 时逐位一致）
#[must_use]
pub fn vital_tone(vital: i32, max_vital: i32) -> Tone {
    let pct = if max_vital > 0 { vital * 100 / max_vital } else { vital };
    if pct <= 34 {
        Tone::Bad
    } else if pct <= 49 {
        Tone::Warn
    } else {
        Tone::Good
    }
}

/// 干劲色调（`[1, 5]`，越高越好）
#[must_use]
pub fn motivation_tone(motivation: i32) -> Tone {
    match motivation {
        5 | 4 => Tone::Good,
        3 => Tone::Normal,
        2 => Tone::Warn,
        _ => Tone::Bad,
    }
}

/// 增减量色调：正为增益、负为减益
#[must_use]
pub fn delta_tone(delta: i32) -> Tone {
    match delta {
        d if d > 0 => Tone::Good,
        d if d < 0 => Tone::Bad,
        _ => Tone::Normal,
    }
}

/// 实际着色（编译期 feature 关闭时退化为纯文本）
#[cfg(feature = "color")]
fn paint_color(tone: Tone, text: &str) -> String {
    use colored::Colorize;

    match tone {
        Tone::Normal => text.to_string(),
        Tone::Good => text.green().to_string(),
        Tone::Warn => text.yellow().to_string(),
        Tone::Bad => text.red().to_string(),
        Tone::Accent => text.bright_cyan().to_string(),
        Tone::Muted => text.bright_black().to_string(),
    }
}

/// 实际着色（feature 关闭：即便上色器开关为真也无色可上）
#[cfg(not(feature = "color"))]
fn paint_color(_tone: Tone, text: &str) -> String {
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：关闭着色后逐字等于原文
    #[test]
    fn test_styler_off_is_plain() {
        let s = Styler::new(false);
        assert_eq!(s.paint(Tone::Bad, "危险"), "危险");
        assert!(!s.color());
    }

    /// 层1：体力三档（按上限归一的百分比）
    #[test]
    fn test_vital_tone() {
        assert_eq!(vital_tone(34, 100), Tone::Bad);
        assert_eq!(vital_tone(35, 100), Tone::Warn);
        assert_eq!(vital_tone(49, 100), Tone::Warn);
        assert_eq!(vital_tone(50, 100), Tone::Good);
        assert_eq!(vital_tone(30, 60), Tone::Good, "按上限归一而非绝对值");
        assert_eq!(vital_tone(10, 0), Tone::Bad, "上限缺失时按绝对值兜底");
    }

    /// 层1：增减量色调
    #[test]
    fn test_delta_tone() {
        assert_eq!(delta_tone(1), Tone::Good);
        assert_eq!(delta_tone(-1), Tone::Bad);
        assert_eq!(delta_tone(0), Tone::Normal);
    }

    /// 层1：着色开关打开时着色结果仍含原文
    #[test]
    fn test_paint_keeps_text() {
        let painted = Styler::new(true).paint(Tone::Good, "abc");
        assert!(painted.contains("abc"), "着色不得吞掉文本: {painted:?}");
    }
}
