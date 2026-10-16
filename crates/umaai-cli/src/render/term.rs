//! 终端探测与呈现门禁
//!
//! 宽度取自真实终端；非 tty（重定向、CI）与探测失败退化为 [`FALLBACK_WIDTH`]。
//! 着色受三重门禁：编译期 feature `color`、运行时 `--no-color` 与 `NO_COLOR` 环境变量、
//! 以及 stdout 是否为 tty。门禁集中在本模块，其余模块只拿判定结果，不自己读环境。
//!
//! 文本显示宽度与折行也走本模块（CJK 按两列、ANSI 转义不计宽），避免各处各算一套。

use std::io::IsTerminal;

/// 探测不到终端宽度时的回退列数
pub const FALLBACK_WIDTH: usize = 100;

/// 呈现可用的最小列数
///
/// 低于该列数终端上报不采信（多为探测噪声），显式宽度也按下限收敛：再窄下去
/// 表格会把 CJK 切成替换字符，入口层另按本值严格报错。
pub const MIN_WIDTH: usize = 40;

/// 探测终端列数（非 tty / 探测失败 / 数值过小一律返回 `None`）
#[must_use]
pub fn width() -> Option<usize> {
    console::Term::stdout()
        .size_checked()
        .map(|(_, cols)| usize::from(cols))
        .filter(|cols| *cols >= MIN_WIDTH)
}

/// 呈现宽度：显式值优先（低于 [`MIN_WIDTH`] 收敛到下限），其次探测终端，最后回退值
#[must_use]
pub fn resolve_width(explicit: Option<usize>) -> usize {
    explicit
        .map(|cols| cols.max(MIN_WIDTH))
        .or_else(width)
        .unwrap_or(FALLBACK_WIDTH)
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

/// 按显示宽度折行（CJK 按两列、ANSI 转义不计宽，内嵌换行按硬断处理）
///
/// 优先在空格处断行，单个词超宽才硬切；转义序列不跨行拆开。
/// 每段显示宽度不超过 `width`（单字符本身超宽时除外），折行只丢断点空格，其余内容与原文一致。
#[must_use]
pub fn wrap_display(text: &str, width: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0usize;
    // 最近一个空格：`(空格的字节下标, 空格之前的行宽)`
    let mut space_at: Option<(usize, usize)> = None;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\n' {
            out.push(std::mem::take(&mut cur));
            cur_w = 0;
            space_at = None;
            continue;
        }
        if c == '\u{1b}' {
            // ANSI 转义序列整段带走，不计宽也不拆行
            cur.push(c);
            if chars.peek() == Some(&'[') {
                for n in chars.by_ref() {
                    cur.push(n);
                    if ('\u{40}'..='\u{7e}').contains(&n) {
                        break;
                    }
                }
            } else if let Some(n) = chars.next() {
                cur.push(n);
            }
            continue;
        }
        let cw = text_width(&c.to_string());
        if cur_w > 0 && cur_w + cw > width {
            if c == ' ' {
                // 分隔空格放不下：断在当前词边界（空格丢掉，不当新行开头）
                out.push(std::mem::take(&mut cur));
                cur_w = 0;
                space_at = None;
                continue;
            }
            match space_at.take() {
                // 在最近空格处断：新行接空格之后的内容
                Some((at, before)) if at > 0 => {
                    let rest = cur[at + 1..].to_string();
                    cur.truncate(at);
                    cur_w -= before + 1;
                    out.push(std::mem::take(&mut cur));
                    cur = rest;
                }
                // 没有可断的空格（或空格在行首）：按宽度硬切
                _ => {
                    out.push(std::mem::take(&mut cur));
                    cur_w = 0;
                }
            }
        }
        if c == ' ' {
            space_at = Some((cur.len(), cur_w));
        }
        cur.push(c);
        cur_w += cw;
    }
    if !cur.is_empty() || out.is_empty() {
        out.push(cur);
    }
    out
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

    /// 层1：显式宽度低于下限时收敛到 `MIN_WIDTH`
    #[test]
    fn test_resolve_width_floors_explicit() {
        assert_eq!(resolve_width(Some(20)), MIN_WIDTH);
        assert_eq!(resolve_width(Some(MIN_WIDTH)), MIN_WIDTH);
    }

    /// 层1：CJK 按两列、ANSI 不计宽
    #[test]
    fn test_text_width() {
        assert_eq!(text_width("abc"), 3);
        assert_eq!(text_width("速"), 2, "汉字占两列");
        assert_eq!(text_width("\u{1b}[31mab\u{1b}[0m"), 2, "转义序列不计宽");
    }

    /// 层1：折行优先在空格处断，超宽词硬切，转义序列不计宽也不拆行
    #[test]
    fn test_wrap_display() {
        assert_eq!(wrap_display("aaa bbb ccc", 7), vec!["aaa bbb", "ccc"], "空格处断行");
        assert_eq!(wrap_display("abcdefghij", 4), vec!["abcd", "efgh", "ij"], "无空格硬切");
        assert_eq!(wrap_display("a\nb", 10), vec!["a", "b"], "内嵌换行按硬断");
        let colored = wrap_display("\u{1b}[31mab\u{1b}[0m cd", 3);
        assert!(
            colored.iter().all(|line| text_width(line) <= 3),
            "折行后每段不得超宽: {colored:?}"
        );
        assert_eq!(
            colored.concat(),
            "\u{1b}[31mab\u{1b}[0mcd",
            "折行只丢断点空格，转义序列与内容保持"
        );
    }
}
