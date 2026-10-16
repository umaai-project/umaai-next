//! 阶段标识：剧本自定的「阶段位置」数字
//!
//! 阶段只回答「现在走到哪一步」，**不承载中间状态**——「吃了哪碗面」「选了哪个地区」
//! 一类中间数据在剧本自己的局面字段里（旧实现同法：阶段枚举 + `pending_*` 字段）。
//! 每个剧本给自己的阶段枚举显式赋号，引擎只把号透传给前端，不解释任何语义。
//!
//! **号是稳定契约**：改号等于旧的 CRN 种子 / 基线 / 记录失效。
//!
//! 取数字用 [`StageId::get`]（或 `u64::from`）；跨类型比较与算术不提供便利实现，
//! 类型边界保留在这一层。

use std::fmt;

use serde::{Deserialize, Serialize};

/// 阶段位置标识（剧本自定的数字）
///
/// 同一进程只跑一个剧本，号段无需跨剧本统一；剧本内部仍是强类型枚举，
/// 只在对外投影时转成本类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StageId(u64);

impl StageId {
    /// 由剧本显式赋的数字构造
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// 取数字（CRN 阶段键 / 记录列 / 前端事件用）
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for StageId {
    /// 形如 `stage#3`（人读日志用；正式呈现由前端按剧本映射）
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "stage#{}", self.0)
    }
}

impl From<u64> for StageId {
    fn from(id: u64) -> Self {
        Self(id)
    }
}

impl From<StageId> for u64 {
    fn from(id: StageId) -> Self {
        id.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：数字往返与展示形态
    #[test]
    fn test_stage_id() {
        let id = StageId::new(3);
        assert_eq!(id.get(), 3);
        assert_eq!(u64::from(id), 3);
        assert_eq!(StageId::from(7_u64).get(), 7);
        assert_eq!(id.to_string(), "stage#3");
        assert_eq!(id, StageId::new(3));
        assert_ne!(id, StageId::new(4));
    }
}
