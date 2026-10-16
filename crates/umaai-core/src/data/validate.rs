//! 数据语义校验
//!
//! 校验载入后的数据满足程序依赖的不变量；失败时定位到「文件 / 条目 / 字段」。
//! 只做「缺了就会让下游静默算错」的检查，不追求穷尽数据本身的正确性。

use crate::{RaceMask, UmaError, UmaResult};

use super::Data;

/// 校验载入后的数据
pub fn validate(data: &Data) -> UmaResult<()> {
    if data.uma.is_empty() {
        return Err(field("umaDB.json", "马娘表为空"));
    }
    if data.card.is_empty() {
        return Err(field("cardDB.json", "支援卡表为空"));
    }
    if !data.text.contains_key("6") {
        return Err(field("text_data_dict.json/6", "缺少角色名分组（角色名查询依赖）"));
    }
    if data.events.story_events.is_empty() {
        return Err(field("events.json/story_events", "必发事件为空"));
    }
    if data.events.card_events.is_empty() {
        return Err(field("events.json/card_events", "支援卡事件为空"));
    }
    if !data.events.system_events.contains_key("extra_train") {
        return Err(field("events.json/system_events/extra_train", "缺少加练事件"));
    }
    if data.consts.train_names.is_empty() {
        return Err(field("constants.json/train_names", "训练名为空"));
    }
    if data.consts.training_vital_threshold.len() != data.consts.train_names.len() {
        return Err(field(
            "constants.json/training_vital_threshold",
            format!(
                "训练位数 {} 与训练名数 {} 不一致",
                data.consts.training_vital_threshold.len(),
                data.consts.train_names.len()
            ),
        ));
    }
    if data.consts.five_status_final_score.is_empty() {
        return Err(field("constants.json/five_status_final_score", "评分表为空"));
    }
    for key in ["card_event", "uma_event", "drop_motivation"] {
        if !data.consts.event_probs.contains_key(key) {
            return Err(field(format!("constants.json/event_probs/{key}"), "缺少随机事件分布键"));
        }
    }
    for (id, uma) in &data.uma {
        for &race in &uma.races {
            if !in_race_window(race as i64) {
                return Err(field(
                    format!("umaDB.json/{id}/races"),
                    format!("比赛回合 {race} 超出位段窗口"),
                ));
            }
        }
        for fr in &uma.free_races {
            if fr.start_turn > fr.end_turn {
                return Err(field(
                    format!("umaDB.json/{id}/freeRaces"),
                    format!("自由比赛区间倒置: {} > {}", fr.start_turn, fr.end_turn),
                ));
            }
            if !(in_race_window(fr.start_turn as i64) && in_race_window(fr.end_turn as i64)) {
                return Err(field(
                    format!("umaDB.json/{id}/freeRaces"),
                    format!("区间 {}..={} 超出位段窗口", fr.start_turn, fr.end_turn),
                ));
            }
        }
    }
    Ok(())
}

/// 回合是否落在位段窗口内
fn in_race_window(turn: i64) -> bool {
    (RaceMask::MIN_TURN as i64..=RaceMask::MAX_TURN as i64).contains(&turn)
}

/// 构造带定位串的校验错误
fn field(target: impl Into<String>, detail: impl Into<String>) -> UmaError {
    UmaError::Validation {
        target: target.into(),
        detail: detail.into(),
    }
}
