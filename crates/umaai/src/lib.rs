//! # umaai（引擎层）
//!
//! 职责：`Runner` 主循环 · 协议解析（单场景直连）· `Config` · 决策编排 · luck / record
//! （无终端 / IO 重依赖）。把内部状态变化翻译成事件回调给 `Frontend`，**自己绝不打印**。
//!
//! 签名（目标形态）：`Runner::run(cfg, frontend, data) -> Result<Summary>`。
//!
//! 设计见 `docs/plan/miko_next_plan.md`。
#![warn(missing_docs)]

pub mod config;
pub mod decision;
pub mod frontend;
pub mod luck;
pub mod protocol;
pub mod record;
pub mod runner;
pub mod session;

pub use config::Config;
pub use decision::{Candidate, DecisionRecord, DecisionSource, LuckAttach};
pub use frontend::{ErrorEvent, ErrorKind, Frontend, GameStart, RunStart, StageCtx, UmaStatus};
pub use luck::{LuckScoreSnapshot, LuckScoreTracker, raw_from_display, to_display};
pub use protocol::{
    BaseParams, BasePersonStatus, BaseStatus, ChannelNote, FileChannel, FinalScoreFrame, FrameKind, FrameNote,
    FrameOutcome, RawFrame, StoryStatus, parse_base_status, plugin_dir,
};
pub use record::LuckScorer;
pub use runner::{Runner, Summary};
pub use session::{FinalFrameOutcome, FrameRoute, RunStop, Session, SessionSummary, StepOutcome};
