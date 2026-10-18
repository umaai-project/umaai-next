//! 输出原语：机读单行 + 人读面板与表格
//!
//! 机读行（`TAG key=value ...`）供脚本跨 commit 对比；人读块复用 `umaai-cli` 的呈现件，
//! 宽度、折行与 CJK 计宽只在那边做一次（工具件不重写一套）。
//!
//! 值里**不得出现空白**（机读行按空白切分）：调用方在填值前做替换。

use umaai_cli::render::{panel, table, term};

/// 机读单行：`TAG key=value key=value ...`
pub fn machine_line(tag: &str, fields: &[(&str, String)]) -> String {
    let body: Vec<String> = fields.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{tag} {}", body.join(" "))
}

/// 把任意文本压成机读行可用的值（空白折成 `_`）
pub fn token(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join("_")
}

/// 键值块（左列按最长键对齐）
pub fn key_values(rows: &[(String, String)]) -> String {
    panel::key_values(rows)
}

/// 分节标题（按显示宽度补横线）
pub fn title(text: &str, width: usize) -> String {
    panel::title(text, width)
}

/// 表格块；宽度由调用方给出，重定向与测试的输出才可复现
pub fn table_block(header: &[&str], rows: &[Vec<String>], width: usize) -> String {
    let mut grid = table::Grid::new(header.iter().copied());
    for row in rows {
        grid.push(row.iter().cloned());
    }
    grid.render(width)
}

/// 呈现宽度：显式值优先（低于下限收敛），其次探测终端，最后回退值
pub fn resolve_width(explicit: Option<usize>) -> usize {
    term::resolve_width(explicit)
}

/// 呈现可用的最小列数
pub const MIN_WIDTH: usize = term::MIN_WIDTH;

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：机读行按 `key=value` 拼接，字段顺序原样保留
    #[test]
    fn test_machine_line() {
        let line = machine_line("BENCH", &[("runs", "8".into()), ("score_mean", "67500.0".into())]);
        assert_eq!(line, "BENCH runs=8 score_mean=67500.0");
    }

    /// 层1：空白折成下划线，值不破坏机读行的切分
    #[test]
    fn test_token_folds_whitespace() {
        assert_eq!(token("训练 速度"), "训练_速度");
        assert_eq!(token("  a\tb\nc "), "a_b_c");
    }

    /// 层1：机读行的字段值不含空白（切分后每段都是 `key=value`）
    #[test]
    fn test_machine_line_is_splittable() {
        let line = machine_line("PROBE", &[("best_action", token("训练 速度"))]);
        for field in line.split_whitespace().skip(1) {
            assert!(field.contains('='), "字段应形如 key=value: {field}");
        }
    }

    /// 层1：表格宽度由调用方给出，窄宽度下不产生超宽行
    #[test]
    fn test_table_block_respects_width() {
        let rows = vec![vec!["一段很长的单元格文本".to_string()]];
        let out = table_block(&["列"], &rows, 16);
        let widest = out.lines().map(term::text_width).max().expect("应渲染出行");
        assert!(widest <= 16, "不得超过指定宽度: {widest}");
    }
}
