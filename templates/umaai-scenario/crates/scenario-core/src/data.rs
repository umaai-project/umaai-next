//! 剧本资产（`scenario.json`）：schema、载入与校验
//!
//! 剧本数据**不进引擎仓**：根目录由宿主注入（终端 / Web / Android 各按自家布局），
//! 本模块只负责「从给定位置读 + 校验」，不碰任何写死路径。
//!
//! 模板只演示最小字段；真实剧本在这里补自己的资源表（源泉 / 事件 / 倍率等）。

use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

/// 剧本数据文件名（放在宿主注入的剧本数据根下）
pub const SCENARIO_DATA_FILE: &str = "scenario.json";

/// 剧本全部静态数据
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioData {
    /// 剧本 ID（真实剧本填自己的 ID）
    pub scenario_id: i32,
    /// 示例：剧本自持资源（模板占位，接上真实资源后改取数）
    pub sample_resource: i32,
    /// 训练基础值表：`[训练 0-4][等级] -> [速 耐 力 根 智 PT 体力消耗]`
    pub training_basic_value: Vec<Vec<[i32; 7]>>,
}

impl ScenarioData {
    /// 从宿主注入的数据根载入 `scenario.json`
    pub fn load(root: impl AsRef<Path>) -> Result<Self> {
        let path = root.as_ref().join(SCENARIO_DATA_FILE);
        let text = std::fs::read_to_string(&path).with_context(|| format!("读取剧本数据失败: {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("剧本数据来源: {}", path.display()))
    }

    /// 解析并校验剧本数据（测试 / 内嵌场景复用）
    pub fn parse(text: &str) -> Result<Self> {
        let data: Self = serde_json::from_str(text).context("解析剧本数据失败")?;
        data.validate()?;
        Ok(data)
    }

    /// 载入期校验：剧本 ID 非负、五个训练、每训练至少一档、每档七值
    fn validate(&self) -> Result<()> {
        ensure!(self.scenario_id >= 0, "scenario_id 不能为负");
        ensure!(
            self.training_basic_value.len() == 5,
            "training_basic_value 应有 5 个训练，实际 {}",
            self.training_basic_value.len()
        );
        for (i, levels) in self.training_basic_value.iter().enumerate() {
            ensure!(!levels.is_empty(), "training_basic_value[{i}] 至少应有一档");
            for (level, row) in levels.iter().enumerate() {
                ensure!(
                    row.len() == 7,
                    "training_basic_value[{i}][{level}] 应有 7 个值，实际 {}",
                    row.len()
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "scenario_id": 0,
        "sample_resource": 3,
        "training_basic_value": [
            [[1, 0, 0, 0, 0, 0, -20]],
            [[0, 1, 0, 0, 0, 0, -20]],
            [[0, 0, 1, 0, 0, 0, -20]],
            [[0, 0, 0, 1, 0, 0, -20]],
            [[0, 0, 0, 0, 1, 0, 5]]
        ]
    }"#;

    #[test]
    fn parse_sample_ok() {
        let data = ScenarioData::parse(SAMPLE).expect("样例数据应可解析");
        assert_eq!(data.scenario_id, 0);
        assert_eq!(data.sample_resource, 3);
        assert_eq!(data.training_basic_value.len(), 5);
    }

    #[test]
    fn reject_wrong_training_count() {
        let text = SAMPLE.replace(
            r#"[[0, 0, 0, 0, 1, 0, 5]]
        ]"#,
            r#"[[0, 0, 0, 0, 1, 0, 5]]
        ,[[1, 0, 0, 0, 0, 0, -20]]]"#,
        );
        assert!(ScenarioData::parse(&text).is_err(), "六个训练应报错");
    }

    #[test]
    fn reject_bad_row_width() {
        let text = SAMPLE.replace("[1, 0, 0, 0, 0, 0, -20]", "[1, 0, 0, 0, 0, 0]");
        assert!(ScenarioData::parse(&text).is_err(), "六值行应报错");
    }
}
