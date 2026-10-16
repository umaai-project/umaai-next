//! 表格渲染：本仓**唯一**接触表格库的模块
//!
//! 实现取 `tabled`（选型理由见 `docs/issues.md`）：行 / 列合并齐全、按 ANSI 计宽
//! （`ansi` feature，单元格内多色不破坏对齐）、有显式宽度策略。换库只改本文件。
//!
//! 宽度**必须由调用方给出**（见 [`crate::render::term::resolve_width`]）：表格库自己不探测
//! 终端；显式传宽也让重定向与测试的输出可复现。

use tabled::builder::Builder;
use tabled::settings::style::Style;
use tabled::settings::width::Width;

/// 一张待渲染的表：表头 + 数据行，单元格是**已上色**的字符串
///
/// 单元格里可以放带 ANSI 颜色的文本，宽度计算不计转义序列（依赖 `tabled` 的 `ansi`
/// feature），因此「同一格内多色」不会把列冲歪。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grid {
    header: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Grid {
    /// 建表（表头可为空）
    #[must_use]
    pub fn new(header: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            header: header.into_iter().map(Into::into).collect(),
            rows: Vec::new(),
        }
    }

    /// 只有数据行、没有表头
    #[must_use]
    pub fn bare() -> Self {
        Self::default()
    }

    /// 追加一行（列数不足的行由表格库补空）
    pub fn push(&mut self, row: impl IntoIterator<Item = impl Into<String>>) {
        self.rows.push(row.into_iter().map(Into::into).collect());
    }

    /// 是否没有任何数据行
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 渲染为文本块（超出 `width` 列时按词折行）
    #[must_use]
    pub fn render(&self, width: usize) -> String {
        let mut builder = Builder::default();
        if !self.header.is_empty() {
            builder.push_record(self.header.iter().cloned());
        }
        for row in &self.rows {
            builder.push_record(row.iter().cloned());
        }
        let mut table = builder.build();
        table.with(Style::modern());
        table.with(Width::wrap(width).keep_words(true));
        table.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：表头 + 数据行按 UTF-8 边框渲染，行数正确
    #[test]
    fn test_render_shape() {
        let mut g = Grid::new(["速", "耐"]);
        g.push(["120", "90"]);
        let out = g.render(80);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 5, "顶边 + 表头 + 分隔 + 数据 + 底边: {out}");
        assert!(out.contains("速") && out.contains("120"));
    }

    /// 层1：空表只出表头，`is_empty` 只看数据行
    #[test]
    fn test_empty_grid() {
        let g = Grid::new(["a", "b"]);
        assert!(g.is_empty());
        assert!(!g.render(80).is_empty(), "有表头仍应渲染出边框");
        assert!(Grid::bare().render(80).is_empty(), "无表头无数据应为空串");
    }

    /// 层1：超过宽度时折行，不产生超宽行
    #[test]
    fn test_wrap_respects_width() {
        let mut g = Grid::new(["列"]);
        g.push(["一段需要折行的很长文本"]);
        let out = g.render(12);
        let widest = out
            .lines()
            .map(crate::render::term::text_width)
            .max()
            .expect("应渲染出行");
        assert!(widest <= 12, "不得超过指定宽度: {widest} in {out:?}");
    }
}
