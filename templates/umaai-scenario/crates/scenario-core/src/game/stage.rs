//! 剧本阶段机（内部强类型；对外只投影 [`StageId`]）
//!
//! 纪律：**无数据、无引用、可 `Copy`**；中间状态（吃了哪碗面 / 选了哪个地区）放局面字段。

use umasim::game::StageId;

/// 剧本阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stage {
    /// 回合开始（事件链）
    #[default]
    Begin,
    /// 训练 / 比赛决策点
    Train,
    /// 回合结算
    End,
}

impl Stage {
    /// 下一个阶段（`None` = 本回合走完）
    pub(crate) fn next(self) -> Option<Self> {
        match self {
            Self::Begin => Some(Self::Train),
            Self::Train => Some(Self::End),
            Self::End => None,
        }
    }
}

impl From<Stage> for StageId {
    /// 显式赋号：**号是稳定契约**，改号等于旧种子与基线失效
    fn from(stage: Stage) -> Self {
        Self::new(match stage {
            Stage::Begin => 0,
            Stage::Train => 1,
            Stage::End => 2,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_chain() {
        assert_eq!(Stage::Begin.next(), Some(Stage::Train));
        assert_eq!(Stage::Train.next(), Some(Stage::End));
        assert_eq!(Stage::End.next(), None);
    }

    #[test]
    fn stage_ids_are_stable() {
        assert_eq!(StageId::from(Stage::Begin), StageId::new(0));
        assert_eq!(StageId::from(Stage::Train), StageId::new(1));
        assert_eq!(StageId::from(Stage::End), StageId::new(2));
    }
}
