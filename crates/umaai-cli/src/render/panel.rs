//! 面板：无边框的分节文本块
//!
//! 与表格的分工：面板放少量键值摘要，[`crate::render::table`] 放多行多列明细。
//! 面板不做折行（长文本由调用方自己截断），宽度只用于标题横线。

use crate::render::term::text_width;

/// 分节标题：`── 标题 ──────────`
///
/// 按**显示宽度**补横线（CJK 计两列），宽度不足时退化为只留前缀，不给出超宽行。
#[must_use]
pub fn title(text: &str, width: usize) -> String {
    let head = format!("── {text} ");
    let used = text_width(&head);
    if used >= width {
        return format!("── {text}");
    }
    format!("{head}{}", "─".repeat(width - used))
}

/// 键值块：左列按最长键对齐，每行一个条目
#[must_use]
pub fn key_values(rows: &[(String, String)]) -> String {
    let key_w = rows.iter().map(|(k, _)| text_width(k)).max().unwrap_or(0);
    rows.iter()
        .map(|(k, v)| {
            let pad = key_w.saturating_sub(text_width(k));
            format!("{k}{} {v}", " ".repeat(pad))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：标题补齐到指定显示宽度（CJK 计两列）
    #[test]
    fn test_title_pads_to_width() {
        let line = title("终局", 20);
        assert_eq!(text_width(&line), 20, "标题行应正好铺满: {line:?}");
        assert!(line.starts_with("── 终局 "));
    }

    /// 层1：宽度不足时退化，不产生超宽行
    #[test]
    fn test_title_narrow_width() {
        let line = title("终局", 3);
        assert_eq!(line, "── 终局");
        assert!(text_width(&line) > 3, "已无处可退，允许超宽");
    }

    /// 层1：值列按最长键的显示宽度对齐（CJK 计两列）
    #[test]
    fn test_key_values_align() {
        let rows = vec![
            ("评分".to_string(), "100".to_string()),
            ("PT".to_string(), "5".to_string()),
        ];
        // 「评分」占 4 列 → 其后 1 空格；「PT」占 2 列 → 其后 3 空格，值都从第 5 列起
        assert_eq!(key_values(&rows), "评分 100\nPT   5");
    }

    /// 层1：空输入给空串，不 panic
    #[test]
    fn test_key_values_empty() {
        assert_eq!(key_values(&[]), "");
    }
}
