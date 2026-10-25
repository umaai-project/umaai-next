//! 剧本配置段（`scenario_config.toml`）：schema、载入与覆盖层合并
//!
//! 复用引擎「默认层 + 覆盖层」的泛型解析（`OverlayConfig` / `parse_with_overlay`）：
//! 默认层字段齐全、未知键严格报错；覆盖层同构、字段全 `Option`，段缺失整段沿用。
//! 根目录由宿主注入，本模块不写死路径。

use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use umaai::config::{OverlayConfig, parse_with_overlay};

/// 剧本配置文件名（放在宿主注入的剧本配置根下）
pub const SCENARIO_CONFIG_FILE: &str = "scenario_config.toml";

/// 训练下标上限（0-4：速 / 耐 / 力 / 根 / 智）
const TRAIN_INDEX_MAX: usize = 4;

/// 剧本专属配置段
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioConfig {
    /// 训练优先顺序（样例训练员按回合轮转取用）
    pub sample_order: Vec<usize>,
}

/// 覆盖层：与默认层同构、字段全 `Option`（`None` = 沿用默认层）
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioConfigOverlay {
    sample_order: Option<Vec<usize>>,
}

impl OverlayConfig for ScenarioConfig {
    type Overlay = ScenarioConfigOverlay;

    fn apply_overlay(&mut self, overlay: Self::Overlay) {
        if let Some(order) = overlay.sample_order {
            self.sample_order = order;
        }
    }

    fn validate(&self) -> Result<()> {
        ScenarioConfig::validate(self)
    }
}

impl ScenarioConfig {
    /// 解析 TOML 文本（默认层 + 可选覆盖层；测试 / 内嵌场景复用）
    pub fn parse(base_toml: &str, overlay_toml: Option<&str>) -> Result<Self> {
        parse_with_overlay(base_toml, overlay_toml)
    }

    /// 载入：默认层文件 + 显式指定的覆盖层文件（给了就必须存在）
    pub fn load(base: impl AsRef<Path>, overlay: Option<&Path>) -> Result<Self> {
        let base = base.as_ref();
        let base_text =
            std::fs::read_to_string(base).with_context(|| format!("读取剧本配置失败: {}", base.display()))?;
        let overlay_text = match overlay {
            Some(path) => Some(
                std::fs::read_to_string(path).with_context(|| format!("读取剧本覆盖配置失败: {}", path.display()))?,
            ),
            None => None,
        };
        Self::parse(&base_text, overlay_text.as_deref()).with_context(|| format!("剧本配置来源: {}", base.display()))
    }

    /// 载入：默认层 + **约定覆盖层**（`<dir>/scenario_config.toml`，存在才合并）
    ///
    /// 与引擎 `Config::load_with_overlay_file` 同语义：覆盖层缺席不报错。
    pub fn load_with_overlay_file(base_dir: impl AsRef<Path>, dir: impl AsRef<Path>) -> Result<Self> {
        let overlay = dir.as_ref().join(SCENARIO_CONFIG_FILE);
        let overlay = overlay.exists().then_some(overlay);
        Self::load(base_dir.as_ref().join(SCENARIO_CONFIG_FILE), overlay.as_deref())
    }

    /// 载入期校验：顺序表非空、训练下标在 0-4
    fn validate(&self) -> Result<()> {
        ensure!(!self.sample_order.is_empty(), "sample_order 不能为空");
        for &index in &self.sample_order {
            ensure!(
                index <= TRAIN_INDEX_MAX,
                "sample_order 训练下标越界: {index}（应为 0-{TRAIN_INDEX_MAX}）"
            );
        }
        Ok(())
    }
}

impl Default for ScenarioConfig {
    /// 与仓内默认层 `data/scenario_config.toml` 同值（未注入配置的宿主 / 测试兜底）
    fn default() -> Self {
        Self {
            sample_order: vec![0, 1, 2, 3, 4],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_base_ok() {
        let cfg = ScenarioConfig::parse("sample_order = [0, 1, 2, 3, 4]", None).expect("默认层应可解析");
        assert_eq!(cfg.sample_order, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn overlay_replaces_list() {
        let cfg = ScenarioConfig::parse("sample_order = [0, 1, 2, 3, 4]", Some("sample_order = [4, 3]"))
            .expect("覆盖层应可合并");
        assert_eq!(cfg.sample_order, vec![4, 3]);
    }

    #[test]
    fn unknown_key_rejected() {
        let err = ScenarioConfig::parse("sample_orders = [0]", None).unwrap_err();
        assert!(err.to_string().contains("解析默认配置失败"));
    }

    #[test]
    fn out_of_range_index_rejected() {
        assert!(ScenarioConfig::parse("sample_order = [5]", None).is_err());
    }
}
