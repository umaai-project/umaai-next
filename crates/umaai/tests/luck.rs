//! L2 模块集成：运气分装配链（配置接线 + 多回合累计 + 跳过语义 + 序列化形状）
//!
//! 数值对照用公开的显示分换算 [`to_display`] 独立重算，另钉一个绝对数字防止公式走样；
//! 「跳过」按旧实现的语义检查：无评分决策既不挂运气分、也不推进基线。

mod common;

use anyhow::Result;
use umaai::{Candidate, Config, DecisionRecord, LuckScorer, to_display};
use umaai_core::testing::data_root;

use crate::common::Checks;

/// 带评分候选的决策记录（按搜索次数加权后基线 = 均分）
fn scored(raw: f64) -> DecisionRecord {
    DecisionRecord {
        kind: "train".to_string(),
        action_index: 0,
        action_desc: format!("训练 {raw}"),
        candidates: vec![
            Candidate {
                description: "训练 A".to_string(),
                score: Some(raw - 50.0),
                n: 1,
            },
            Candidate {
                description: "训练 B".to_string(),
                score: Some(raw + 50.0),
                n: 1,
            },
        ],
        luck: None,
        extra: None,
    }
}

/// 配置接线：运气分加成来自数据根默认配置
#[test]
fn luck_config_wired() -> Result<()> {
    let mut c = Checks::new("运气分配置接线");
    let cfg = Config::load_from_data_root(data_root()?, None)?;
    c.check(cfg.record.luck_turn_bonus == 70, "默认配置的运气分加成为 70");
    c.check(!cfg.record.enabled, "在线记录默认关闭");
    println!("record 段: {cfg:?}");
    c.finish()
}

/// 多回合：逐回合运气与累计运气与独立重算一致
#[test]
fn luck_multi_turn() -> Result<()> {
    let mut c = Checks::new("运气分多回合累计");
    let cfg = Config::load_from_data_root(data_root()?, None)?;
    let bonus = cfg.record.luck_turn_bonus;
    let max_turn = 78;
    let epoch = 42;
    let raws = [50000.0, 50150.0, 50320.0, 50280.0];
    // 钉一个绝对数字：turn 0 / raw 50000 / bonus 70 / max 78 → 50000 + 78×70
    c.check(to_display(50000.0, 0, max_turn, bonus) == 55460.0, "显示分换算绝对值");

    let mut scorer = LuckScorer::new(bonus);
    let displays: Vec<f64> = raws
        .iter()
        .enumerate()
        .map(|(i, raw)| to_display(*raw, i as i32, max_turn, bonus))
        .collect();

    for (i, raw) in raws.iter().enumerate() {
        let mut rec = scored(*raw);
        let delta = scorer.attach(&mut rec, epoch, i as i32, max_turn);
        let luck = rec.luck.clone().expect("有评分应挂载");
        let expect_delta = if i == 0 {
            None
        } else {
            Some(displays[i] - displays[i - 1])
        };
        c.check(delta == expect_delta, &format!("第 {i} 回合运气 = 显示分差"));
        c.check(
            (luck.snapshot.total_luck_score - (displays[i] - displays[0])).abs() < 1e-9,
            &format!("第 {i} 回合累计运气 = 当前 − 初始"),
        );
        c.check(luck.action_luck.len() == 2, "逐候选偏差与候选同长");
        println!(
            "  turn={i} raw={raw} delta={delta:?} total={} action_luck={:?}",
            luck.snapshot.total_luck_score, luck.action_luck
        );
    }
    c.check(scorer.epoch_key() == Some(epoch), "切局键已记录");
    c.check(
        (scorer.snapshot().total_luck_score - (displays[3] - displays[0])).abs() < 1e-9,
        "快照累计运气一致",
    );
    c.finish()
}

/// 跳过语义：无评分决策不挂载、不推进基线（下一回合仍从上一有效回合起算）
#[test]
fn luck_skip_unscored() -> Result<()> {
    let mut c = Checks::new("运气分跳过语义");
    let cfg = Config::load_from_data_root(data_root()?, None)?;
    let mut scorer = LuckScorer::new(cfg.record.luck_turn_bonus);
    let max_turn = 78;

    let mut first = scored(50000.0);
    scorer.attach(&mut first, 7, 0, max_turn);
    let before = scorer.snapshot();

    let mut skipped = DecisionRecord::action_only("event", "无评分决策");
    c.check(
        scorer.attach(&mut skipped, 7, 1, max_turn).is_none(),
        "跳过时不产生回合运气",
    );
    c.check(skipped.luck.is_none(), "跳过时不挂运气分");
    c.check(scorer.snapshot() == before, "跳过时不推进基线");

    let mut next = scored(50150.0);
    let delta = scorer.attach(&mut next, 7, 2, max_turn).expect("有评分应算出差值");
    let expect = to_display(50150.0, 2, max_turn, cfg.record.luck_turn_bonus)
        - to_display(50000.0, 0, max_turn, cfg.record.luck_turn_bonus);
    c.check((delta - expect).abs() < 1e-9, "差值直接跨过被跳过的回合");
    println!("跳过后再记: delta={delta} expect={expect}");
    c.finish()
}

/// 切局与序列化：切局重置，快照四键齐全
#[test]
fn luck_epoch_and_serde() -> Result<()> {
    let mut c = Checks::new("运气分切局与序列化");
    let cfg = Config::load_from_data_root(data_root()?, None)?;
    let mut scorer = LuckScorer::new(cfg.record.luck_turn_bonus);

    let mut a = scored(50000.0);
    scorer.attach(&mut a, 1, 0, 78);
    let mut b = scored(50150.0);
    scorer.attach(&mut b, 1, 1, 78);
    c.check(scorer.snapshot().total_luck_score > 0.0, "同局内累计运气为正");

    let mut c2 = scored(48000.0);
    c.check(scorer.attach(&mut c2, 2, 1, 78).is_none(), "切局首回合无回合运气");
    c.check(scorer.snapshot().total_luck_score == 0.0, "切局后累计运气清零");

    let json = serde_json::to_string(&c2)?;
    for key in [
        "initial_terminal_baseline",
        "current_terminal_baseline",
        "total_luck_score",
        "last_turn_delta",
        "action_luck",
    ] {
        c.check(json.contains(key), &format!("序列化含键 {key}"));
    }
    println!("样例决策 JSON: {json}");
    c.finish()
}
