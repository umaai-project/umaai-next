//! 帧内事件段（`baseGame.story`）：选项与数值
//!
//! 事件帧携带的选项数值是「这一刻发生了什么」的唯一来源。本模块把线上协议翻译成核心层的
//! 事件结构：**每个选项按成功 / 失败两支建模**——成功支取 `SuccessEffectValue`、失败支取
//! `FailedEffectValue`，两支按常量表的默认成功率（`event_probs.default_success_event`）
//! 分概率；只有一支带数值时该支必定触发。
//!
//! 旧实现把数值解析出来后**丢掉了**（局部变量未挂到事件上），事件帧对局面零影响；这里按
//! 帧数据修好——真机样本中 39 个选项组有 6 组带失败值（多数两支数值相同）。

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use umaai_core::Array6;
use umaai_core::gamedata::{ActionValue, ChoiceResult, EventChoice, EventData};

/// 帧内事件
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct StoryStatus {
    /// 事件 ID
    pub id: u32,
    /// 事件名
    #[serde(default)]
    pub name: String,
    /// 触发者名（人读，不参与计算）
    #[serde(default)]
    pub trigger_name: String,
    /// 选项：外层是选项，内层是该选项的结果分支
    #[serde(default)]
    pub choices: Vec<Vec<StoryChoice>>,
}

/// 帧内事件选项
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct StoryChoice {
    /// 选项文本
    #[serde(default)]
    pub option: String,
    /// 成功 / 大成功效果描述（人读，不参与计算）
    #[serde(default)]
    pub success_effect: String,
    /// 失败 / 小成功效果描述（人读，不参与计算）
    #[serde(default)]
    pub failed_effect: String,
    /// 成功数值
    #[serde(default)]
    pub success_effect_value: Option<StoryEffectValue>,
    /// 失败数值
    #[serde(default)]
    pub failed_effect_value: Option<StoryEffectValue>,
}

/// 帧内事件数值
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct StoryEffectValue {
    /// 数值槽位：`[0..6]` 五维 + PT，`[6]` Hint 等级，`[7]` 体力，`[8]` 羁绊，`[9]` 干劲
    #[serde(default)]
    pub values: Vec<i32>,
    /// Hint 技能名（人读，不参与计算）
    #[serde(default)]
    pub skill_names: Vec<String>,
    /// 其它词条（人读，不参与计算）
    #[serde(default)]
    pub extras: Vec<String>,
    /// 状态名（人读，不参与计算）
    #[serde(default)]
    pub buff_name: Option<String>,
}

impl StoryEffectValue {
    /// 取数值槽位 `[self.0, self.0 + 6)`；越界报错（不 panic）
    fn slot6(&self, start: usize) -> Result<Array6> {
        let end = start + 6;
        if self.values.len() < end {
            bail!("事件数值槽位不足：需要 {end} 项，实际 {}", self.values.len());
        }
        let mut ret = [0i32; 6];
        ret.copy_from_slice(&self.values[start..end]);
        Ok(ret)
    }

    /// 取单个数值槽位；越界报错
    fn slot(&self, index: usize) -> Result<i32> {
        self.values
            .get(index)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("事件数值缺槽位 #{index}（实际 {} 项）", self.values.len()))
    }

    /// 五维 + PT
    pub fn status_pt(&self) -> Result<Array6> {
        self.slot6(0)
    }

    /// Hint 等级
    pub fn hint_level(&self) -> Result<i32> {
        self.slot(6)
    }

    /// 体力
    pub fn vital(&self) -> Result<i32> {
        self.slot(7)
    }

    /// 羁绊
    pub fn friendship(&self) -> Result<i32> {
        self.slot(8)
    }

    /// 干劲
    pub fn motivation(&self) -> Result<i32> {
        self.slot(9)
    }

    /// → 事件效果
    pub fn to_action_value(&self) -> Result<ActionValue> {
        Ok(ActionValue {
            status_pt: self.status_pt()?,
            hint_level: self.hint_level()?,
            vital: self.vital()?,
            friendship: self.friendship()?,
            motivation: self.motivation()?,
            max_vital: 0,
        })
    }
}

impl StoryChoice {
    /// 选项的默认成功率（常量表 `event_probs.default_success_event`）
    pub fn default_success_rate() -> Result<f64> {
        Ok(umaai_core::Data::get()?.event_prob("default_success_event")?)
    }

    /// 单个选项 → 该选项的结果分支
    ///
    /// - 成功 / 失败两支都有数值：两支按 `success_prob` 分概率（`success_prob + fail = 100`）
    /// - 只有一支有数值：该支必定触发（`prob = 100`）
    /// - 两支都缺：报错（事件帧缺数值等于无法还原局面，不能静默按 0 处理）
    fn to_branches(&self, success_prob: i32) -> Result<Vec<EventChoice>> {
        let success = self
            .success_effect_value
            .as_ref()
            .map(StoryEffectValue::to_action_value)
            .transpose()?;
        let failed = self
            .failed_effect_value
            .as_ref()
            .map(StoryEffectValue::to_action_value)
            .transpose()?;
        let branches = match (success, failed) {
            (Some(ok), Some(ng)) => vec![
                branch(&ok, ChoiceResult::Success, success_prob),
                branch(&ng, ChoiceResult::Fail, 100 - success_prob),
            ],
            (Some(ok), None) => vec![branch(&ok, ChoiceResult::Success, 100)],
            (None, Some(ng)) => vec![branch(&ng, ChoiceResult::Fail, 100)],
            (None, None) => bail!("事件选项「{}」既无成功数值也无失败数值", self.option),
        };
        Ok(branches)
    }
}

impl StoryStatus {
    /// 帧内事件 → 待处理事件
    ///
    /// 选项组多于一组时标记为玩家可选（交给训练员决策），与 `Game::run_event` 的判定一致；
    /// 人头归属（`person_index`）不在本帧内，由剧本侧按事件上下文补齐。
    pub fn to_event_data(&self) -> Result<EventData> {
        let success_prob = (StoryChoice::default_success_rate()? * 100.0).round().clamp(0.0, 100.0) as i32;
        let mut choices = Vec::with_capacity(self.choices.len());
        for (i, group) in self.choices.iter().enumerate() {
            let choice = group
                .first()
                .ok_or_else(|| anyhow::anyhow!("事件 #{} 的第 {} 个选项为空", self.id, i + 1))?;
            choices.push(choice.to_branches(success_prob)?);
        }
        Ok(EventData {
            id: self.id,
            name: self.name.clone(),
            player_select: choices.len() > 1,
            choices,
            ..Default::default()
        })
    }
}

/// 造一个带概率的结果分支
fn branch(value: &ActionValue, result: ChoiceResult, prob: i32) -> EventChoice {
    let mut entry = EventChoice::from_action_value(value, result);
    entry.prob = prob;
    entry
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 十槽位数值（五维 + pt / hint / vital / 羁绊 / 干劲）
    fn value() -> StoryEffectValue {
        StoryEffectValue {
            values: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            ..Default::default()
        }
    }

    /// 层1：槽位映射
    #[test]
    fn test_slots() -> Result<()> {
        let v = value();
        assert_eq!(v.status_pt()?, [1, 2, 3, 4, 5, 6]);
        assert_eq!(v.hint_level()?, 7);
        assert_eq!(v.vital()?, 8);
        assert_eq!(v.friendship()?, 9);
        assert_eq!(v.motivation()?, 10);
        let av = v.to_action_value()?;
        assert_eq!(av.status_pt, [1, 2, 3, 4, 5, 6]);
        assert_eq!(av.vital, 8);
        Ok(())
    }

    /// 层1：槽位不足报错（不 panic）
    #[test]
    fn test_short_values() {
        let v = StoryEffectValue {
            values: vec![1, 2, 3],
            ..Default::default()
        };
        assert!(v.status_pt().is_err());
        assert!(v.vital().is_err());
    }

    /// 层1：协议原文解析（PascalCase，缺字段容忍）
    #[test]
    fn test_parse() -> Result<()> {
        let raw = r#"{"Id":9001,"Name":"测试事件","TriggerName":"某人",
            "Choices":[[{"Option":"A","SuccessEffect":"+10","SuccessEffectValue":{"Values":[10,0,0,0,0,0,0,0,0,0],"SkillNames":[],"Extras":[]}}]]}"#;
        let story: StoryStatus = serde_json::from_str(raw)?;
        assert_eq!(story.id, 9001);
        assert_eq!(story.choices.len(), 1);
        let choice = &story.choices[0][0];
        assert_eq!(choice.option, "A");
        assert_eq!(
            choice
                .success_effect_value
                .as_ref()
                .map(|v| v.status_pt())
                .transpose()?,
            Some([10, 0, 0, 0, 0, 0])
        );
        assert!(choice.failed_effect_value.is_none());
        Ok(())
    }

    /// 造带成功 / 失败数值的选项
    fn choice(option: &str, success: Option<Vec<i32>>, failed: Option<Vec<i32>>) -> StoryChoice {
        let value = |values: Option<Vec<i32>>| values.map(|values| StoryEffectValue { values, ..Default::default() });
        StoryChoice {
            option: option.to_string(),
            success_effect_value: value(success),
            failed_effect_value: value(failed),
            ..Default::default()
        }
    }

    /// 十槽位数值（首槽为速）
    fn slots(first: i32) -> Vec<i32> {
        let mut values = vec![0; 10];
        values[0] = first;
        values
    }

    /// 层1：成功 / 失败两支按默认成功率分概率
    #[test]
    fn test_two_branches() -> Result<()> {
        umaai_core::Data::init(umaai_core::testing::data_root()?)?;
        let rate = StoryChoice::default_success_rate()?;
        let story = StoryStatus {
            id: 1,
            name: "两支".to_string(),
            choices: vec![vec![choice("A", Some(slots(10)), Some(slots(20)))]],
            ..Default::default()
        };
        let event = story.to_event_data()?;
        assert_eq!(event.choices.len(), 1);
        let group = &event.choices[0];
        assert_eq!(group.len(), 2, "成功 / 失败两支");
        assert_eq!(group[0].result, ChoiceResult::Success as i32);
        assert_eq!(group[1].result, ChoiceResult::Fail as i32);
        assert_eq!(group[0].prob + group[1].prob, 100, "两支概率互补");
        assert_eq!(group[0].prob, (rate * 100.0).round() as i32);
        assert_eq!(group[0].value.status_pt[0], 10);
        assert_eq!(group[1].value.status_pt[0], 20);
        assert!(!event.player_select, "单选项不交给训练员");
        Ok(())
    }

    /// 层1：只有一支有数值时必定触发
    #[test]
    fn test_single_branch() -> Result<()> {
        umaai_core::Data::init(umaai_core::testing::data_root()?)?;
        let only_ok = StoryStatus {
            id: 2,
            name: "只成功".to_string(),
            choices: vec![
                vec![choice("A", Some(slots(10)), None)],
                vec![choice("B", Some(slots(30)), None)],
            ],
            ..Default::default()
        };
        let event = only_ok.to_event_data()?;
        assert_eq!(event.choices.len(), 2);
        assert_eq!(event.choices[0].len(), 1, "无失败数值 → 单支");
        assert_eq!(event.choices[0][0].prob, 100);
        assert_eq!(event.choices[0][0].result, ChoiceResult::Success as i32);
        assert!(event.player_select, "多选项交给训练员决策");

        let only_ng = StoryStatus {
            id: 3,
            name: "只失败".to_string(),
            choices: vec![vec![choice("A", None, Some(slots(20)))]],
            ..Default::default()
        };
        let event = only_ng.to_event_data()?;
        assert_eq!(event.choices[0][0].result, ChoiceResult::Fail as i32);
        Ok(())
    }

    /// 层1：两支都没数值必须报错（不静默按 0）
    #[test]
    fn test_no_value_errors() -> Result<()> {
        umaai_core::Data::init(umaai_core::testing::data_root()?)?;
        let story = StoryStatus {
            id: 4,
            name: "无数值".to_string(),
            choices: vec![vec![choice("A", None, None)]],
            ..Default::default()
        };
        assert!(story.to_event_data().is_err());
        Ok(())
    }
}
