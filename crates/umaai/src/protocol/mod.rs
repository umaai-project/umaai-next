//! 协议层：小黑板帧的读取与**通用段**解析
//!
//! 帧是单个 JSON 对象：`baseGame` 段对任何剧本都一样（本模块负责），剧本专属段
//! （如拉面的 `ramen`）由剧本仓自行解析，**不进本仓**。
//!
//! 纪律：
//!
//! - 读取**容忍未知键**（插件新增字段不得让整帧失败），但必填 / 可选的分界照抄既有
//!   serde 契约——字段漂移要在解析处炸出声，不能静默按 0 参与决策。
//! - 本层不含任何剧本常量：剧本 ID、友人卡、友人首次事件都由宿主经
//!   [`BaseParams`] 注入或被调用方判定。
//! - 帧 → 局面的转换只覆盖通用字段；人头构造与回合阶段归剧本侧。

pub mod base;
pub mod channel;
pub mod final_score;
pub mod story;

pub use base::{
    BaseParams, BasePersonStatus, BaseStatus, EventOwner, EventRef, FrameNote, FrameOutcome, decode_event_id,
    parse_base_status,
};
pub use channel::{ChannelNote, FileChannel, FrameKind, RawFrame, plugin_dir};
pub use final_score::FinalScoreFrame;
pub use story::{StoryChoice, StoryEffectValue, StoryStatus};
