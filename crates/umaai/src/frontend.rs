//! 前端回调：引擎把内部状态变化翻译成结构化事件，**自己绝不打印**
//!
//! 所有方法都有空默认实现，前端只覆写关心的事件（测试用的记录器、CLI 的渲染器、
//! JSON 事件流各取所需）。事件只带数据与机器可读键，人读文案由前端决定。
//!
//! `on_stage_begin` / `on_stage_end` 成对：引擎经单一出口下发（见 [`crate::Runner`]），
//! 阶段内提前返回也会闭合，不靠各分支手写。局面摘要 [`UmaStatus`] 在阶段闭合之后下发，
//! 因此它自己取失败时不会破坏这对事件。

use serde::{Deserialize, Serialize};
use umasim::game::StageId;

use crate::decision::DecisionRecord;
use crate::runner::Summary;

/// 运行开始时的上下文
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunStart {
    /// 剧本选择器（不透明，原样来自配置）
    pub scenario: String,
    /// 训练员选择器（不透明，原样来自配置）
    pub trainer: String,
    /// 基础随机种子
    pub seed: u64,
}

/// 一局开始时的上下文
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameStart {
    /// 切局键（在线记录按它判「新的一局」；离线单局取马娘 ID）
    pub epoch_key: u64,
    /// 马娘 ID
    pub uma_id: u32,
    /// 最大回合数
    pub max_turn: i32,
}

/// 阶段开始时的上下文
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageCtx {
    /// 当前回合（0 基）
    pub turn: i32,
    /// 最大回合数
    pub max_turn: i32,
    /// 阶段位置（剧本自定的标识；引擎不解释语义）
    pub stage: StageId,
}

/// 阶段结束时的局面摘要（只带剧本无关字段）
///
/// 供回合面板一类的呈现消费。剧本专属资源信息（地域 / 诀窍 / 泡汤等）不进本结构，
/// 由剧本侧自行呈现；剧本要补自己的字段时在剧本仓里另做事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UmaStatus {
    /// 当前回合（0 基）
    pub turn: i32,
    /// 最大回合数
    pub max_turn: i32,
    /// 阶段位置（剧本自定的标识；引擎不解释语义）
    pub stage: StageId,
    /// 体力
    pub vital: i32,
    /// 体力上限
    pub max_vital: i32,
    /// 干劲 `[1, 5]`
    pub motivation: i32,
    /// 五维 `[速, 耐, 力, 根, 智]`
    pub five_status: [i32; 5],
    /// 当前总 PT（技能点 + Hint 折算）
    pub pt: i32,
    /// 累计 Hint 等级
    pub total_hints: i32,
}

/// 错误类别（供前端分类渲染；引擎只给类别与细节，不给文案）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ErrorKind {
    /// 配置相关
    Config,
    /// 全局数据相关
    Data,
    /// 对局推进相关
    Game,
    /// 协议与帧相关
    Protocol,
    /// 引擎内部
    Internal,
}

/// 错误事件
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorEvent {
    /// 错误类别
    pub kind: ErrorKind,
    /// 错误细节（`anyhow` 因果链的完整文本）
    pub detail: String,
}

/// 前端：接收引擎事件（`&mut self`；引擎单线程驱动一局）
pub trait Frontend {
    /// 运行开始
    fn on_run_start(&mut self, _ctx: &RunStart) {}
    /// 一局开始（切局语义，见 [`GameStart::epoch_key`]）
    fn on_game_start(&mut self, _ctx: &GameStart) {}
    /// 阶段开始（与 [`Self::on_stage_end`] 成对）
    fn on_stage_begin(&mut self, _ctx: &StageCtx) {}
    /// 阶段结束
    fn on_stage_end(&mut self) {}
    /// 阶段结束时的局面摘要（阶段成功后下发；与 [`Self::on_stage_end`] 同点，在其之后）
    fn on_uma_status(&mut self, _status: &UmaStatus) {}
    /// 一次决策
    fn on_decision(&mut self, _rec: &DecisionRecord) {}
    /// 错误
    fn on_error(&mut self, _err: &ErrorEvent) {}
    /// 运行结束（终局汇总）
    fn on_run_end(&mut self, _summary: &Summary) {}
}
