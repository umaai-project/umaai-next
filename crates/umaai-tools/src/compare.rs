//! 分支推进对照（复盘用）
//!
//! 一份**对照计划** JSON 自持探测回合坐标、探测动作与探测回合数；工具按配置装配局面、
//! 注入规则主种子，用确定性首候选推进到该回合的训练决策点，再克隆两支：**默认支**继续
//! 按首候选推进，**探测支**在探测回合改取探测动作。规则随机按 `(master, 回合)` 派生，
//! 故两支同回合的随机输入一致——差异只来自这一个动作。
//!
//! 只**观测与对照**：两支都在工具内推进，不写回局面、不落盘；计划自持（不读配置层），
//! 只有局面装配与搜索口径沿用配置链。

use std::path::Path;

use anyhow::{Context, Result, ensure};
use rand::SeedableRng;
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};
use umaai::Config;
use umaai_cli::cli::build_game;
use umaai_core::gamedata::EventChoice;
use umasim::game::Game;
use umasim::game::Trainer;
use umasim::game::base::basic::{BasicAction, BasicGame};
use umasim::game::base::stage::BasicStage;

use crate::setup::decision_root_at;

/// 对照计划：一份 JSON 自持探测坐标、动作与回合数
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    /// 规则主种子（缺省取配置层 `sim.seed`）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// 探测回合坐标（0 基；必须落在该回合的训练决策点）
    pub turn: i32,
    /// 探测动作：该回合候选表（`list_actions`）的下标
    pub action: usize,
    /// 探测回合数：从探测回合起推进并观测的回合数（≥ 1）
    pub turns: u32,
    /// 备注（一行；随计划自持，不参与判据）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Plan {
    /// 计划自检：坐标与回合数
    fn validate(&self) -> Result<()> {
        ensure!(self.turn >= 0, "探测回合坐标不得为负: {}", self.turn);
        ensure!(self.turns >= 1, "探测回合数必须 ≥ 1（收到 {}）", self.turns);
        Ok(())
    }
}

/// 单回合观测事实
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnFacts {
    /// 回合（0 基）
    pub turn: i32,
    /// 该回合训练决策点选中的动作展示文本（非训练阶段为空串）
    pub action: String,
    /// 评分
    pub score: i32,
    /// PT
    pub pt: i32,
    /// 五维
    pub five: Vec<i32>,
}

/// 逐回合对照行（同一回合的两支观测）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// 回合（0 基）
    pub turn: i32,
    /// 默认支观测
    pub default: TurnFacts,
    /// 探测支观测
    pub probe: TurnFacts,
}

impl Row {
    /// 评分差（探测 − 默认）
    #[must_use]
    pub fn score_delta(&self) -> i32 {
        self.probe.score - self.default.score
    }

    /// PT 差（探测 − 默认）
    #[must_use]
    pub fn pt_delta(&self) -> i32 {
        self.probe.pt - self.default.pt
    }

    /// 五维和差（探测 − 默认）
    #[must_use]
    pub fn five_delta(&self) -> i32 {
        self.probe.five.iter().sum::<i32>() - self.default.five.iter().sum::<i32>()
    }
}

/// 对照结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// 实际使用的规则主种子
    pub seed: u64,
    /// 探测回合坐标
    pub probe_turn: i32,
    /// 计划的探测回合数
    pub planned_turns: u32,
    /// 探测回合的候选动作（下标与计划里的 `action` 对齐）
    pub candidates: Vec<String>,
    /// 默认一选的展示文本
    pub default_action: String,
    /// 探测动作的展示文本
    pub probe_action: String,
    /// 默认支实际观测到的回合数
    pub default_turns: usize,
    /// 探测支实际观测到的回合数
    pub probe_turns: usize,
    /// 逐回合对照行（两支都观测到的回合；提前结束即少于计划回合数）
    pub rows: Vec<Row>,
}

impl Report {
    /// 最后一行的对照（没有观测行时为 `None`）
    #[must_use]
    pub fn last(&self) -> Option<&Row> {
        self.rows.last()
    }
}

/// 读取并校验一份对照计划
pub fn load_plan(path: &Path) -> Result<Plan> {
    let text = std::fs::read_to_string(path).with_context(|| format!("读取对照计划失败: {}", path.display()))?;
    let plan: Plan = serde_json::from_str(&text).with_context(|| format!("解析对照计划失败: {}", path.display()))?;
    plan.validate()?;
    Ok(plan)
}

/// 跑一份对照计划，返回逐回合两支观测
pub fn run(cfg: &Config, plan: &Plan) -> Result<Report> {
    plan.validate()?;
    let seed = plan.seed.unwrap_or(cfg.sim.seed);
    let mut game = build_game(cfg).context("对照局面装配失败")?;
    game.set_rule_master(seed);
    let (root, actions) = decision_root_at(game, seed, plan.turn)?;
    let candidates: Vec<String> = actions.iter().map(ToString::to_string).collect();
    let default_action = candidates.first().cloned().unwrap_or_default();
    let probe_action = candidates.get(plan.action).cloned().ok_or_else(|| {
        anyhow::anyhow!(
            "探测动作下标 {} 越界（回合 {} 候选 {} 个：{}）",
            plan.action,
            plan.turn,
            candidates.len(),
            candidates.join(" / ")
        )
    })?;

    // 两支同根：默认支全按首候选；探测支只在探测回合改取探测动作
    let default_side = run_branch(&root, seed, plan.turns, &|_| 0)?;
    let probe_side = run_branch(&root, seed, plan.turns, &|turn| {
        if turn == plan.turn { plan.action } else { 0 }
    })?;

    let paired = default_side.len().min(probe_side.len());
    let mut rows = Vec::with_capacity(paired);
    for i in 0..paired {
        ensure!(
            default_side[i].turn == probe_side[i].turn,
            "两支回合不同步: 默认 {} / 探测 {}",
            default_side[i].turn,
            probe_side[i].turn
        );
        rows.push(Row {
            turn: default_side[i].turn,
            default: default_side[i].clone(),
            probe: probe_side[i].clone(),
        });
    }
    Ok(Report {
        seed,
        probe_turn: plan.turn,
        planned_turns: plan.turns,
        candidates,
        default_action,
        probe_action,
        default_turns: default_side.len(),
        probe_turns: probe_side.len(),
        rows,
    })
}

/// 分支推进：`pick` 给出每个回合在训练决策点的候选下标
///
/// 两支都用**同一把推进钥匙**（首候选 / 探测动作），决策流各自从 `seed` 起；规则随机
/// 由局面自持、按 `(master, 回合)` 派生，故两支同回合的规则随机输入一致。
fn run_branch(root: &BasicGame, seed: u64, turns: u32, pick: &dyn Fn(i32) -> usize) -> Result<Vec<TurnFacts>> {
    let trainer = PickTrainer { pick };
    let mut game = root.clone();
    let mut rng = StdRng::seed_from_u64(seed);
    let start = game.turn();
    let last = start + i32::try_from(turns).context("探测回合数超出范围")? - 1;
    let mut facts = Vec::new();
    loop {
        let cur = game.turn();
        let action = if game.stage == BasicStage::Train {
            let actions = game.list_actions()?;
            let idx = pick(cur);
            ensure!(
                idx < actions.len(),
                "候选下标 {idx} 越界（回合 {cur} 候选 {} 个）",
                actions.len()
            );
            actions[idx].to_string()
        } else {
            String::new()
        };
        game.run_stage(&trainer, &mut rng)?;
        let more = game.next();
        // 回合走完（或对局在此结束）才记一笔：记的是该回合的收束状态
        if !more || game.turn() > cur {
            facts.push(snapshot(&game, cur, action)?);
        }
        if !more || game.turn() > last {
            break;
        }
    }
    Ok(facts)
}

/// 按固定候选下标选动作的训练员（事件选项一律取第 0 个）
struct PickTrainer<'a> {
    /// 回合 → 候选下标
    pick: &'a dyn Fn(i32) -> usize,
}

impl Trainer<BasicGame> for PickTrainer<'_> {
    fn select_action(&self, game: &BasicGame, _actions: &[BasicAction], _rng: &mut StdRng) -> Result<usize> {
        Ok((self.pick)(game.turn()))
    }

    fn select_choice(&self, _game: &BasicGame, _choices: &[Vec<EventChoice>], _rng: &mut StdRng) -> Result<usize> {
        Ok(0)
    }
}

/// 取一回合的观测事实
fn snapshot(game: &BasicGame, turn: i32, action: String) -> Result<TurnFacts> {
    let uma = game.uma();
    Ok(TurnFacts {
        turn,
        action,
        score: uma.calc_score()?,
        pt: uma.total_pt()?,
        five: uma.five_status.to_vec(),
    })
}

/// 五维逗号连接（机读行里不得出现空白）
#[must_use]
pub fn join_five(five: &[i32]) -> String {
    five.iter().map(i32::to_string).collect::<Vec<_>>().join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：计划自检拦住负坐标与零回合
    #[test]
    fn test_plan_validate() {
        let ok = Plan {
            seed: Some(42),
            turn: 12,
            action: 1,
            turns: 5,
            note: None,
        };
        assert!(ok.validate().is_ok());

        let mut bad = ok.clone();
        bad.turn = -1;
        assert!(bad.validate().is_err(), "负回合坐标应报错");

        let mut bad = ok;
        bad.turns = 0;
        assert!(bad.validate().is_err(), "零回合应报错");
    }

    /// 层1：计划 JSON 严格解析（拼错键 / 缺字段都报错），可选字段有缺省
    #[test]
    fn test_plan_serde_strict() {
        let plan: Plan = serde_json::from_str(r#"{"turn": 12, "action": 1, "turns": 5}"#).expect("最小计划");
        assert_eq!(plan.seed, None);
        assert_eq!(plan.note, None);

        assert!(
            serde_json::from_str::<Plan>(r#"{"turn": 12, "action": 1, "turns": 5, "tunrs": 1}"#).is_err(),
            "拼错的键必须被看见"
        );
        assert!(
            serde_json::from_str::<Plan>(r#"{"turn": 12, "turns": 5}"#).is_err(),
            "缺 action 应报错"
        );
    }

    /// 层1：对照行差值按「探测 − 默认」计算
    #[test]
    fn test_row_deltas() {
        let facts = |score, pt, five: [i32; 5]| TurnFacts {
            turn: 3,
            action: "训练".to_string(),
            score,
            pt,
            five: five.to_vec(),
        };
        let row = Row {
            turn: 3,
            default: facts(1000, 10, [1, 0, 0, 0, 0]),
            probe: facts(1100, 12, [0, 2, 0, 0, 0]),
        };
        assert_eq!(row.score_delta(), 100);
        assert_eq!(row.pt_delta(), 2);
        assert_eq!(row.five_delta(), 1);
        assert_eq!(join_five(&row.probe.five), "0,2,0,0,0");
    }
}
