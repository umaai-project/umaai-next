//! # umaai-tests（集成 / 端到端测试，`publish = false`）
//!
//! 测试支撑与集成测试的家：
//!
//! - [`Checker`]：打印 + 聚合，失败返回 `Err`（**不用 `assert`**，保持 `cargo test` 有失败信号）。
//! - 共享夹具：替代旧库 `#[path = "..."]` 跨目录引入的写法。
//! - baseline 加载 / 比对：`tests/baseline.rs` + 基线数据文件（自动判定）。
//!
//! 测试分层与约定见 `docs/plan/miko_next_plan.md` 的「测试策略」节。
#![warn(missing_docs)]

use anyhow::{Result, bail};

/// 集成测试检查器：逐条打印结果并聚合失败项
///
/// 用法：`let mut c = Checker::new("标题"); c.check(cond, "说明"); c.finish()?;`
/// 失败不打断流程，`finish()` 汇总为 `Err`，保证 `cargo test` 有失败信号。
pub struct Checker {
    title: String,
    failures: Vec<String>,
}

impl Checker {
    /// 建立检查器
    pub fn new(title: &str) -> Self {
        println!("== {title} ==");
        Self {
            title: title.to_string(),
            failures: vec![],
        }
    }

    /// 记录一条检查：通过打 `ok`，失败打 `FAIL` 并记录说明
    pub fn check(&mut self, ok: bool, label: &str) -> &mut Self {
        if ok {
            println!("ok   {label}");
        } else {
            println!("FAIL {label}");
            self.failures.push(label.to_string());
        }
        self
    }

    /// 结束：全部通过返回 `Ok`，否则聚合失败项返回 `Err`
    pub fn finish(self) -> Result<()> {
        if self.failures.is_empty() {
            println!("== {} 全部通过 ==", self.title);
            Ok(())
        } else {
            bail!(
                "{} 失败 {} 项: {}",
                self.title,
                self.failures.len(),
                self.failures.join("; ")
            )
        }
    }
}

/// 跨 crate 集成测试共用的夹具（不承诺稳定 API）
///
/// 集中一处是为了让「同一局参数」在多条端到端路径上可对照：直接 `run_full_game`、
/// 经引擎 `Runner`、以及配置默认层，都取这里的马娘 / 卡组 / 继承 / 等级表。
#[doc(hidden)]
pub mod testing {
    use anyhow::Result;
    use rand::rngs::StdRng;
    use umaai_core::gamedata::EventChoice;
    use umasim::game::InheritInfo;
    use umasim::game::Trainer;
    use umasim::game::base::BaseAction;
    use umasim::game::base::basic::{BasicAction, BasicGame};

    /// 比赛等级表（与 `gamedata/default_config.toml` 的 `standard` 预设同值）
    pub const RACE_GRADES: [i32; 72] = [
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 3, 4, 3, 3, 4, 3, 3, 2, 3, 1, 1, 3, 4, 3, 4, 2, 2, 1, 2, 1, 1, 1, 1, 1,
        3, 3, 3, 3, 1, 1, 1, 1, 1, 2, 1, 2, 3, 1, 1, 2, 1, 2, 1, 1, 2, 1, 1, 3, 3, 3, 3, 3, 1, 2, 1, 1, 1, 2, 1,
    ];

    /// 冒烟用马娘 ID（与配置默认层同值）
    pub const UMA_ID: u32 = 101901;

    /// 冒烟用卡组（与配置默认层同值）
    pub const DECK: [u32; 6] = [302424, 302464, 302484, 302564, 302574, 302644];

    /// 冒烟用继承因子（与配置默认层同值）
    pub fn inherit() -> InheritInfo {
        InheritInfo {
            blue_count: [15, 3, 0, 0, 0],
            extra_count: [0, 30, 0, 0, 30, 30],
        }
    }

    /// 冒烟用训练员：Race > Clinic > Sleep > 其他（保证跑满 78 回合），选项恒选第 0 组
    pub struct RaceFirstTrainer;

    impl Trainer<BasicGame> for RaceFirstTrainer {
        fn select_action(&self, _game: &BasicGame, actions: &[BasicAction], _rng: &mut StdRng) -> Result<usize> {
            let pri = |a: &BasicAction| match a.0 {
                BaseAction::Race => 0,
                BaseAction::Clinic => 1,
                BaseAction::Sleep => 2,
                _ => 3,
            };
            Ok(actions
                .iter()
                .enumerate()
                .min_by_key(|(_, a)| pri(a))
                .map(|(i, _)| i)
                .unwrap_or(0))
        }

        fn select_choice(&self, _game: &BasicGame, _choices: &[Vec<EventChoice>], _rng: &mut StdRng) -> Result<usize> {
            Ok(0)
        }
    }

    /// 建一局冒烟基础局（与配置默认层同参数）
    pub fn basic_newgame() -> Result<BasicGame> {
        BasicGame::newgame(UMA_ID, &DECK, inherit(), &RACE_GRADES)
    }
}
