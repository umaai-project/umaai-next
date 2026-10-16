//! 无剧本基础局的回合阶段机（`BasicGame` 私有）
//!
//! 只服务无剧本基础局：回合内按固定顺序 `Begin → Distribute → Train → AfterTrain` 推进，
//! 阶段机（`next()` 与 `run_stage` 的 match）都在 `BasicGame`。
//! **真实剧本有各自的阶段机**（在剧本仓），对引擎只投影 [`StageId`]。

use enum_iterator::Sequence;
use serde::{Deserialize, Serialize};

use crate::game::StageId;

/// 基础局的回合阶段
///
/// 命名以 `Basic` 开头是为了不再被误当作「通用阶段」——剧本阶段一律自持，
/// 本枚举只为基础局的状态机服务。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Sequence)]
pub enum BasicStage {
    /// 1. 回合开始
    #[default]
    Begin,
    /// 2. 分配人头前，随机事件
    Distribute,
    // --- 可操作部分
    /// 3. 选择剧本 buff 前（基础局不使用，保留号位）
    StoryShop,
    /// 4. 选择训练或比赛
    Train,
    /// 5. 回合后事件
    AfterTrain,
    // ---
    /// 6. 结束，剧本结算
    End,
}

impl From<BasicStage> for StageId {
    /// 显式赋号：与历史 CRN 阶段键一致（`Begin..End` = 0..5），**改号等于旧种子与基线失效**
    fn from(stage: BasicStage) -> Self {
        StageId::new(match stage {
            BasicStage::Begin => 0,
            BasicStage::Distribute => 1,
            BasicStage::StoryShop => 2,
            BasicStage::Train => 3,
            BasicStage::AfterTrain => 4,
            BasicStage::End => 5,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：推进顺序与终点
    #[test]
    fn test_stage_sequence() {
        let mut stage = BasicStage::Begin;
        let mut seen = vec![stage];
        while let Some(next) = stage.next() {
            stage = next;
            seen.push(stage);
        }
        assert_eq!(seen.len(), 6, "六个阶段逐个推进到 End");
        assert_eq!(*seen.last().expect("非空"), BasicStage::End);
    }

    /// 层1：阶段号与历史 CRN 阶段键一致
    #[test]
    fn test_stage_ids() {
        assert_eq!(StageId::from(BasicStage::Begin).get(), 0);
        assert_eq!(StageId::from(BasicStage::Distribute).get(), 1);
        assert_eq!(StageId::from(BasicStage::StoryShop).get(), 2);
        assert_eq!(StageId::from(BasicStage::Train).get(), 3);
        assert_eq!(StageId::from(BasicStage::AfterTrain).get(), 4);
        assert_eq!(StageId::from(BasicStage::End).get(), 5);
    }
}
