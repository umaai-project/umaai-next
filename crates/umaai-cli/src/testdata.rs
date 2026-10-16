//! 单元测试共用的静态数据夹具
//!
//! 只填本 crate 用到的两张名字表（轴名 / 干劲名），其余字段取结构合法的空值——本 crate
//! 不碰它们。刻意手写全字段：`umaai-core` 改结构时这里会编译失败，而不是静默漏掉新字段。

use std::collections::BTreeMap;
use std::sync::OnceLock;

use umaai_core::gamedata::GameConstants;

/// 名字表为空的夹具（测退化分支）
fn bare() -> GameConstants {
    GameConstants {
        five_status_limit_base: [0; 5],
        train_names: Vec::new(),
        motivation_names: Vec::new(),
        training_vital_threshold: Vec::new(),
        group_buff_end_prob: Vec::new(),
        pt_score_rate: 0.0,
        hint_pt_rate: 0.0,
        five_status_final_score: Vec::new(),
        rank_scores: Vec::new(),
        rank_names: Vec::new(),
        event_probs: BTreeMap::new(),
        no_event_turns: Vec::new(),
        base_hint_rate: 0.0,
        rest_probs: Vec::new(),
        hint_event_value: Vec::new(),
        max_hint_per_card: 0,
        five_status_favor_rate: Vec::new(),
    }
}

/// 名字表为空的夹具
pub fn empty() -> &'static GameConstants {
    static FIXTURE: OnceLock<GameConstants> = OnceLock::new();
    FIXTURE.get_or_init(bare)
}

/// 带轴名与干劲名的夹具
pub fn consts() -> &'static GameConstants {
    static FIXTURE: OnceLock<GameConstants> = OnceLock::new();
    FIXTURE.get_or_init(|| GameConstants {
        train_names: vec!["速".into(), "耐".into(), "力".into(), "根".into(), "智".into()],
        motivation_names: vec![
            "绝不调".into(),
            "不调".into(),
            "普通".into(),
            "好调".into(),
            "绝好调".into(),
        ],
        ..bare()
    })
}
