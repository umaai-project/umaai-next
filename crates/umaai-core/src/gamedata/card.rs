//! 支援卡数据（cardDB.json）的纯 schema/DTO

use serde::{Deserialize, Serialize};

use crate::Array6;

/// 支援卡单档数值
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardValue {
    /// 友情加成
    #[serde(default, rename = "youQing")]
    pub youqing: f32,
    /// 干劲加成
    #[serde(default, rename = "ganJing")]
    pub ganjing: i32,
    /// 训练加成
    #[serde(default, rename = "xunLian")]
    pub xunlian: i32,
    /// 赛后加成
    #[serde(default, rename = "saiHou")]
    pub saihou: i32,
    /// 得意率
    #[serde(default, rename = "deYiLv")]
    pub deyilv: f32,
    /// 初始羁绊
    #[serde(default, rename = "initialJiBan")]
    pub initial_jiban: i32,
    /// 启发等级
    #[serde(default)]
    pub hint_level: i32,
    /// 启发概率提升
    #[serde(default)]
    pub hint_prob_increase: i32,
    /// 智训练体力恢复
    #[serde(default)]
    pub wiz_vital_bonus: i32,
    /// 失败率下降
    #[serde(default)]
    pub fail_rate_drop: f32,
    /// 体力消耗降低
    #[serde(default)]
    pub vital_cost_drop: f32,
    /// 事件效果提高
    #[serde(default)]
    pub event_effect_up: i32,
    /// 事件回复量提高
    #[serde(default)]
    pub event_recovery_amount_up: i32,
    /// 副属性（速 / 耐 / 力 / 根 / 智 / pt）
    pub bonus: Array6,
    /// 初始属性
    pub initial_bonus: Array6,
    /// 启发收益
    pub hint_bonus: Array6,
}

/// 支援卡数据（cardDB.json 单条）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportCardData {
    /// 支援卡 ID
    pub card_id: u32,
    /// 角色 ID
    pub chara_id: u32,
    /// 卡名
    pub card_name: String,
    /// 全名
    pub full_name: String,
    /// 稀有度（1/2/3）
    pub rarity: u32,
    /// 卡类型：0 速 / 1 耐 / 2 力 / 3 根 / 4 智 / 5 友人 / 6 团队
    pub card_type: i32,
    /// 各档数值
    pub card_value: Vec<CardValue>,
    /// 固有类型
    #[serde(default)]
    pub unique_effect_type: u32,
    /// 固有描述
    pub unique_effect_summary: Option<String>,
    /// 固有数值
    #[serde(default)]
    pub unique_effect_param: Vec<i32>,
}

impl SupportCardData {
    /// 短名：`]` 前保留原样，`]` 后只取前两个字符
    pub fn short_name(&self) -> String {
        let mut parts = self.card_name.split(']');
        let left = parts.next().unwrap_or_default();
        let right_short: String = parts.next().unwrap_or_default().chars().take(2).collect();
        format!("{left}]{right_short}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：短名截断规则
    #[test]
    fn test_short_name() {
        let card = SupportCardData {
            card_id: 1,
            chara_id: 1,
            card_name: "[特雷森学园]特别周".to_string(),
            full_name: "x".to_string(),
            rarity: 3,
            card_type: 4,
            card_value: vec![],
            unique_effect_type: 0,
            unique_effect_summary: None,
            unique_effect_param: vec![],
        };
        assert_eq!(card.short_name(), "[特雷森学园]特别");
    }

    /// 层1：cardDB 的拼音键名（youQing / ganJing / …）按显式 rename 解析
    #[test]
    fn test_pinyin_renames() {
        let v: CardValue = serde_json::from_str(
            r#"{"youQing": 10, "ganJing": 25, "initialJiBan": 15, "deYiLv": 20, "bonus": [0,0,0,0,0,0],
                "initialBonus": [0,0,0,0,0,0], "hintBonus": [0,0,0,0,0,0]}"#,
        )
        .unwrap();
        assert_eq!(v.youqing, 10.0);
        assert_eq!(v.ganjing, 25);
        assert_eq!(v.initial_jiban, 15);
        assert_eq!(v.deyilv, 20.0);
    }
}
