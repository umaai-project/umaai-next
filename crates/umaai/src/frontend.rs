//! 前端回调：引擎把内部状态变化翻译成结构化事件，**自己绝不打印**
//!
//! 所有方法都有空默认实现，前端只覆写关心的事件（测试用的记录器、CLI 的渲染器、
//! JSON 事件流各取所需）。事件只带数据与机器可读键，人读文案由前端决定。
//!
//! `on_stage_begin` / `on_stage_end` 成对：引擎经单一出口下发（见 [`crate::Runner`]），
//! 阶段内提前返回也会闭合，不靠各分支手写。

use serde::{Deserialize, Serialize};
use umasim::game::TurnStage;

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
    /// 回合内阶段
    pub stage: TurnStage,
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
    /// 一次决策
    fn on_decision(&mut self, _rec: &DecisionRecord) {}
    /// 错误
    fn on_error(&mut self, _err: &ErrorEvent) {}
    /// 运行结束（终局汇总）
    fn on_run_end(&mut self, _summary: &Summary) {}
}
