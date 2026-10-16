//! 终端探测与呈现门禁
//!
//! 宽度取自真实终端；非 tty（重定向、CI）与探测失败退化为 [`FALLBACK_WIDTH`]。
//! 着色受三重门禁：编译期 feature `color`、运行时 `--no-color` 与 `NO_COLOR` 环境变量、
//! 以及 stdout 是否为 tty。门禁集中在本模块，其余模块只拿判定结果，不自己读环境。
//!
//! 文本显示宽度也走本模块（CJK 按两列、ANSI 转义不计宽），避免各处各算一套。

use std::io::IsTerminal;

/// 探测不到终端宽度时的回退列数
pub const FALLBACK_WIDTH: usize = 100;

/// 低于该列数不采信终端上报（多为探测噪声，照用会把表格压得没法读）
const MIN_TRUSTED_WIDTH: usize = 40;

/// 探测终端列数（非 tty / 探测失败 / 数值过小一律返回 `None`）
#[must_use]
pub fn width() -> Option<usize> {
    console::Term::stdout()
        .size_checked()
        .map(|(_, cols)| usize::from(cols))
        .filter(|cols| *cols >= MIN_TRUSTED_WIDTH)
}

/// 呈现宽度：显式值优先，其次探测终端，最后回退值
#[must_use]
pub fn resolve_width(explicit: Option<usize>) -> usize {
    explicit.or_else(width).unwrap_or(FALLBACK_WIDTH)
}

/// 是否着色（编译期 feature + `--no-color` + `NO_COLOR` + stdout 是否 tty）
#[must_use]
pub fn use_color(no_color_flag: bool) -> bool {
    cfg!(feature = "color") && !no_color_flag && !no_color_env() && std::io::stdout().is_terminal()
}

/// `NO_COLOR` 是否生效（按该约定：存在且非空即生效）
fn no_color_env() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty())
}

/// 文本的显示宽度（CJK 按两列算，ANSI 转义序列不计宽）
#[must_use]
pub fn text_width(text: &str) -> usize {
    console::measure_text_width(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：显式宽度优先于探测
    #[test]
    fn test_resolve_width_explicit_wins() {
        assert_eq!(resolve_width(Some(77)), 77);
    }

    /// 层1：探测不到时退化为回退值（测试进程 stdout 非 tty）
    #[test]
    fn test_resolve_width_fallback() {
        assert_eq!(resolve_width(None), width().unwrap_or(FALLBACK_WIDTH));
    }

    /// 层1：CJK 按两列、ANSI 不计宽
    #[test]
    fn test_text_width() {
        assert_eq!(text_width("abc"), 3);
        assert_eq!(text_width("速"), 2, "汉字占两列");
        assert_eq!(text_width("\u{1b}[31mab\u{1b}[0m"), 2, "转义序列不计宽");
    }
}
