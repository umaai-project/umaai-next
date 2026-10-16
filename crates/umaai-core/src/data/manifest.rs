//! 数据清单（`manifest.json`）
//!
//! 清单是数据与程序一致性的唯一入口：声明数据 schema 版本与本数据根包含的文件。

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{UmaError, UmaResult};

/// 清单文件名
pub const MANIFEST_NAME: &str = "manifest.json";

/// 程序期望的数据 schema 版本；与数据不一致时启动即报错
pub const SCHEMA_VERSION: u32 = 1;

/// 程序必需的 5 张静态表（剧本数据不在此列）
pub const REQUIRED_FILES: [&str; 5] = [
    "umaDB.json",
    "cardDB.json",
    "text_data_dict.json",
    "events.json",
    "constants.json",
];

/// 数据清单
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// 数据 schema 版本
    pub schema_version: u32,
    /// 本数据根包含的文件
    pub files: Vec<String>,
}

impl Manifest {
    /// 读取清单并校验：版本一致、声明覆盖全部必需文件
    pub fn load(root: &Path) -> UmaResult<Self> {
        let path = root.join(MANIFEST_NAME);
        let text = std::fs::read_to_string(&path).map_err(|source| UmaError::Io { path: path.clone(), source })?;
        let m: Self = serde_json::from_str(&text).map_err(|source| UmaError::Parse { path: path.clone(), source })?;
        if m.schema_version != SCHEMA_VERSION {
            return Err(UmaError::Validation {
                target: format!("{MANIFEST_NAME}/schema_version"),
                detail: format!(
                    "程序期望 {SCHEMA_VERSION}，数据为 {}（需更新数据或程序）",
                    m.schema_version
                ),
            });
        }
        let missing: Vec<&str> = REQUIRED_FILES
            .iter()
            .copied()
            .filter(|f| !m.files.iter().any(|x| x == f))
            .collect();
        if !missing.is_empty() {
            return Err(UmaError::Validation {
                target: format!("{MANIFEST_NAME}/files"),
                detail: format!("缺少必需文件: {}", missing.join(", ")),
            });
        }
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：清单解析与必需文件覆盖校验
    #[test]
    fn test_manifest_parse() {
        let good = r#"{"schema_version": 1, "files": ["umaDB.json", "cardDB.json", "text_data_dict.json", "events.json", "constants.json"]}"#;
        let m: Manifest = serde_json::from_str(good).unwrap();
        assert_eq!(m.schema_version, SCHEMA_VERSION);
        assert_eq!(m.files.len(), REQUIRED_FILES.len());

        let missing: Manifest = serde_json::from_str(r#"{"schema_version": 1, "files": ["umaDB.json"]}"#).unwrap();
        let miss: Vec<&str> = REQUIRED_FILES
            .iter()
            .copied()
            .filter(|f| !missing.files.iter().any(|x| x == f))
            .collect();
        assert_eq!(miss.len(), 4, "其余 4 张表应被判定缺失");
    }
}
