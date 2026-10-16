//! 搜索框架（计算层）
//!
//! 搜索排序只用**单轴**评分（口径见 [`FlatSearchGame::search_score`]）；不参与排序的
//! 终局观测量走 [`terminal`]。内核 [`FlatSearch`] 不认识任何剧本，一次 rollout 怎么跑
//! 由调用方以闭包注入。
//!
//! 纪律：剧本经 [`FlatSearchGame`] 接入，接入新剧本不应改动本模块任何一行。

pub mod config;
pub mod flat_search;
pub mod result;
pub mod searchable;
pub mod terminal;

pub use config::SearchConfig;
pub use flat_search::{FlatSearch, SearchProbe};
pub use result::{ActionResult, OrderedRollouts, ScoreEntry, SearchOutput};
pub use searchable::FlatSearchGame;
pub use terminal::{NamedMetricRef, NoTerminal, NoTerminalStats, RolloutOutcome, TerminalRecord, TerminalStats};
pub use umaai_core::stats::Moments;
