//! 终局帧（`finalScore.json`）：育成结束、点技能前的真机终局数据
//!
//! 终局帧与回合帧走**同一目录**，靠**文件名**区分；payload 是**扁平**结构（没有
//! `baseGame` 外壳），字段名沿用回合帧命名。它不进决策链路，只用于终局核对与归档。
//!
//! 三道闸（旧实现在线记录链路逐条踩过，勿简化）：
//!
//! 1. **契约必填**：五维 / 五维上限 / 剩余技能点缺一即解析失败——插件字段漂移要炸出声，
//!    不能静默按 0 打分；缺失可容忍的字段一律带 serde 默认值。
//! 2. **数值可用**：[`FinalScoreFrame::is_usable`] 拒绝「字段在场但全 0」的坏帧
//!    （放行会让五维分整体清零，却仍被当成真机终局）。
//! 3. **局号匹配**：文件名固定，文件会残留上一局；由调用方用
//!    [`FinalScoreFrame::epoch_key`] 与当前局号比对，不匹配即忽略。
//!
//! 剧本过滤不在本层：帧内 `scenario_id` 原样保留，是否接受由宿主注入的剧本档案判定。

use anyhow::Result;
use serde::{Deserialize, Serialize};
use umaai_core::{Array5, Data};
use umasim::game::Uma;

/// 终局帧（扁平结构）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinalScoreFrame {
    /// 剧本 ID（引擎不解释）
    pub scenario_id: u32,
    /// 育成局号（单调递增；缺字段时调用方应忽略该帧）
    #[serde(default, rename = "single_mode_chara_id")]
    pub single_mode_chara_id: Option<u64>,
    /// 马娘 ID
    #[serde(default)]
    pub uma_id: u32,
    /// 马娘星数
    #[serde(default)]
    pub uma_star: u32,
    /// 回合（0 基，终局帧为末回合）
    #[serde(default)]
    pub turn: i32,
    /// 客户端育成状态（2 / 3 = 结束态）
    #[serde(default)]
    pub state: i32,
    /// 客户端回合状态
    #[serde(default, rename = "playing_state")]
    pub playing_state: i32,
    /// 终局五维（契约必填）
    pub five_status: Array5,
    /// 五维上限（契约必填）
    pub five_status_limit: Array5,
    /// 点技能前的剩余技能点（契约必填）
    pub skill_pt: i32,
    /// 总 Hint 等级
    #[serde(default)]
    pub total_hints: i32,
    /// 本局累计花掉的技能点
    #[serde(default)]
    pub skill_pt_spent: i32,
    /// 继承属性增量（仅复盘分析用）
    #[serde(default, rename = "inheritGains")]
    pub inherit_gains: Vec<i32>,
}

impl FinalScoreFrame {
    /// 解析帧原文
    pub fn parse(frame: &str) -> Result<Self> {
        Ok(serde_json::from_str(frame)?)
    }

    /// 局号（`None` = 帧内缺字段，调用方应忽略该帧）
    pub fn epoch_key(&self) -> Option<u64> {
        self.single_mode_chara_id
    }

    /// 局号是否属于当前这一局（防上一局残留帧）
    pub fn matches_epoch(&self, current: u64) -> bool {
        self.epoch_key() == Some(current)
    }

    /// 帧内容是否可用：五维与五维上限不得全零
    pub fn is_usable(&self) -> bool {
        self.five_status.iter().any(|v| *v > 0) && self.five_status_limit.iter().any(|v| *v > 0)
    }

    /// 终局评分（与模拟同源：五维查表 + PT 折算）
    ///
    /// 本通道不带已学技能分，切者 / 小切的 PT 折算标志也不在帧内，故是「点技能前」
    /// 的口径，略低于游戏最终分。
    pub fn calc_score(&self) -> Result<i32> {
        Uma {
            five_status: self.five_status,
            five_status_limit: self.five_status_limit,
            skill_pt: self.skill_pt,
            skill_score: 0,
            total_hints: self.total_hints,
            ..Default::default()
        }
        .calc_score()
    }

    /// 终局评分对应的档次名
    pub fn rank_name(&self) -> Result<String> {
        Ok(Data::get()?.consts.get_rank_name(self.calc_score()?).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(five: Array5, limit: Array5) -> FinalScoreFrame {
        FinalScoreFrame {
            scenario_id: 1,
            single_mode_chara_id: Some(7),
            uma_id: 101901,
            uma_star: 4,
            turn: 77,
            state: 3,
            playing_state: 5,
            five_status: five,
            five_status_limit: limit,
            skill_pt: 100,
            total_hints: 10,
            skill_pt_spent: 0,
            inherit_gains: vec![],
        }
    }

    /// 层1：契约必填字段缺失 → 解析失败
    #[test]
    fn test_required_fields() {
        let full = r#"{"scenarioId":1,"umaId":1,"turn":77,"fiveStatus":[100,0,0,0,0],
            "fiveStatusLimit":[200,0,0,0,0],"skillPt":100}"#;
        assert!(FinalScoreFrame::parse(full).is_ok(), "契约字段齐全应可解析");

        let no_five = r#"{"scenarioId":1,"umaId":1,"turn":77,"fiveStatusLimit":[200,0,0,0,0],"skillPt":100}"#;
        let no_limit = r#"{"scenarioId":1,"umaId":1,"turn":77,"fiveStatus":[100,0,0,0,0],"skillPt":100}"#;
        let no_pt =
            r#"{"scenarioId":1,"umaId":1,"turn":77,"fiveStatus":[100,0,0,0,0],"fiveStatusLimit":[200,0,0,0,0]}"#;
        for (name, raw) in [
            ("fiveStatus", no_five),
            ("fiveStatusLimit", no_limit),
            ("skillPt", no_pt),
        ] {
            assert!(FinalScoreFrame::parse(raw).is_err(), "缺 {name} 应报错");
        }
    }

    /// 层1：全零坏帧判不可用
    #[test]
    fn test_usable() {
        assert!(!frame([0; 5], [0; 5]).is_usable(), "五维与上限全零 → 不可用");
        assert!(!frame([100, 0, 0, 0, 0], [0; 5]).is_usable(), "上限全零 → 不可用");
        assert!(frame([100, 0, 0, 0, 0], [200, 0, 0, 0, 0]).is_usable());
    }

    /// 层1：局号匹配
    #[test]
    fn test_epoch() {
        let mut f = frame([100, 0, 0, 0, 0], [200, 0, 0, 0, 0]);
        assert!(f.matches_epoch(7));
        assert!(!f.matches_epoch(8), "上一局残留帧不得匹配");
        f.single_mode_chara_id = None;
        assert_eq!(f.epoch_key(), None);
        assert!(!f.matches_epoch(7));
    }
}
