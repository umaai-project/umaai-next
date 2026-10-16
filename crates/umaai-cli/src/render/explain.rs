//! 领域词元：把领域数字翻成人读文本（纯函数，无 IO、无状态）
//!
//! 轴名与干劲名取自静态数据（`constants.json` 的 `train_names` / `motivation_names`），
//! 不在代码里写第二份。训练等级、事件文案一类的**剧本相关**件归剧本仓
//! （旧仓把这些和领域计算混在 `explain*` 里，见 `docs/issues.md`）。

use umaai_core::gamedata::GameConstants;

/// 干劲名（越界给占位符，不 panic）
#[must_use]
pub fn motivation(consts: &GameConstants, motivation: i32) -> &str {
    let idx = motivation - 1;
    if idx < 0 {
        return "干劲?";
    }
    consts
        .motivation_names
        .get(idx as usize)
        .map_or("干劲?", String::as_str)
}

/// 五维一行：`速 120 耐 90 力 80 根 70 智 60`（轴名缺失时退化为下标）
#[must_use]
pub fn five_status(consts: &GameConstants, five: &[i32; 5]) -> String {
    let mut out = String::new();
    for (i, value) in five.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        match consts.train_names.get(i) {
            Some(name) => out.push_str(&format!("{name} {value}")),
            None => out.push_str(&format!("#{i} {value}")),
        }
    }
    out
}

/// 数值带符号：正数显式带 `+`，便于与增减量对齐阅读
#[must_use]
pub fn signed(value: i32) -> String {
    if value > 0 {
        format!("+{value}")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata;

    /// 层1：干劲名按 1 基取表，越界不 panic
    #[test]
    fn test_motivation() {
        let c = testdata::consts();
        assert_eq!(motivation(c, 1), "绝不调");
        assert_eq!(motivation(c, 5), "绝好调");
        assert_eq!(motivation(c, 0), "干劲?");
        assert_eq!(motivation(c, 9), "干劲?", "越界取不到名字应给占位符");
    }

    /// 层1：五维按轴名拼接、顺序固定
    #[test]
    fn test_five_status() {
        assert_eq!(
            five_status(testdata::consts(), &[120, 90, 80, 70, 60]),
            "速 120 耐 90 力 80 根 70 智 60"
        );
    }

    /// 层1：轴名缺失时退化为下标而非 panic
    #[test]
    fn test_five_status_missing_names() {
        assert_eq!(
            five_status(testdata::empty(), &[1, 2, 3, 4, 5]),
            "#0 1 #1 2 #2 3 #3 4 #4 5"
        );
    }

    /// 层1：符号格式化
    #[test]
    fn test_signed() {
        assert_eq!(signed(3), "+3");
        assert_eq!(signed(0), "0");
        assert_eq!(signed(-3), "-3");
    }
}
