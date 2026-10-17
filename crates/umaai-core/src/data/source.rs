//! 静态数据来源（目录 / 内存）
//!
//! 只出「取一项数据」与「来源标识」两件事，**不假设数据在磁盘上**：目录、内嵌资产、内存
//! JSON 都可实现 [`DataSource`]，宿主也可以在自有实现里组合多个底层来源。
//!
//! 数据形态收两种：已解析的 JSON 值（前端宿主直接给对象）与 JSON 文本（资产 / 网络响应 /
//! `include_str!`）。两者都**直解到目标类型**，不经中间 `Value` 树——`text_data_dict.json`
//! 在本仓有 5.5 MB，多一次全量解析就多一份临时内存。

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::de::DeserializeOwned;

use crate::{UmaError, UmaResult};

/// 数据根内的固定项（5 张静态表 + 清单）
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum DataFile {
    /// `manifest.json`：schema 版本与文件清单（数据与程序一致性的唯一入口）
    Manifest,
    /// `umaDB.json`：马娘表
    Uma,
    /// `cardDB.json`：支援卡表
    Card,
    /// `text_data_dict.json`：文本表
    Text,
    /// `events.json`：事件表
    Events,
    /// `constants.json`：游戏常量
    Constants,
}

impl DataFile {
    /// 对应的文件名（目录来源取文件、错误定位用）
    pub const fn name(self) -> &'static str {
        match self {
            Self::Manifest => super::manifest::MANIFEST_NAME,
            Self::Uma => "umaDB.json",
            Self::Card => "cardDB.json",
            Self::Text => "text_data_dict.json",
            Self::Events => "events.json",
            Self::Constants => "constants.json",
        }
    }
}

/// 静态数据来源
///
/// 表数据**不做叠加**：`manifest.json` 声明的是「一份完整数据集 + schema 版本」，叠一层
/// 就等于绕过版本门控；需要「基线 + 补丁」的宿主在自己的实现里先合并，再交出成品。
pub trait DataSource {
    /// 取一项数据并反序列化到目标类型
    ///
    /// 实现方决定底层形态：目录源读文件文本，内存源可给 [`serde_json::Value`] 或 JSON
    /// 文本。**不要先建中间 `Value` 树再转**：整表直解省一次全量解析与一份临时内存。
    fn table<T: DeserializeOwned>(&self, file: DataFile) -> UmaResult<T>;

    /// 来源标识：仅用于全局初始化的幂等比对（换数据需重启），内容不变时须保持稳定
    fn key(&self) -> String;
}

/// 目录来源：数据根目录下的常规文件
#[derive(Debug, Clone)]
pub struct DirSource {
    /// 数据根目录
    root: PathBuf,
}

impl DirSource {
    /// 以数据根目录创建
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
        }
    }

    /// 数据根目录
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl DataSource for DirSource {
    fn table<T: DeserializeOwned>(&self, file: DataFile) -> UmaResult<T> {
        let path = self.root.join(file.name());
        let text = std::fs::read_to_string(&path).map_err(|source| UmaError::Io { path: path.clone(), source })?;
        serde_json::from_str(&text).map_err(|source| UmaError::Parse { path, source })
    }

    fn key(&self) -> String {
        self.root.display().to_string()
    }
}

/// 内存来源的一项数据形态
#[derive(Debug, Clone)]
enum Payload {
    /// 已解析的 JSON 值（借用反序列化，不复制）
    Value(serde_json::Value),
    /// 原始 JSON 文本
    Text(String),
}

/// 内存来源：宿主自备数据，不经文件系统
///
/// 典型用法：Web / Android 宿主把前端对象（[`Self::with_json`]）或资产 / 网络响应文本
/// （[`Self::with_text`]）交进来；桌面宿主用 `include_str!` 内嵌数据时同样走文本一路。
/// 六项数据缺一不可，缺失在取用时报错并指出缺哪一项。
#[derive(Debug, Clone)]
pub struct MemorySource {
    /// 来源标识（幂等比对用）
    key: String,
    /// 已放入的数据项
    tables: BTreeMap<DataFile, Payload>,
}

impl MemorySource {
    /// 以来源标识创建（标识只用于全局初始化幂等比对，内容变了就该换标识）
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            tables: BTreeMap::new(),
        }
    }

    /// 放入一项**已解析**的 JSON 值
    pub fn with_json(mut self, file: DataFile, value: serde_json::Value) -> Self {
        self.tables.insert(file, Payload::Value(value));
        self
    }

    /// 放入一项 **JSON 文本**
    pub fn with_text(mut self, file: DataFile, text: impl Into<String>) -> Self {
        self.tables.insert(file, Payload::Text(text.into()));
        self
    }
}

impl DataSource for MemorySource {
    fn table<T: DeserializeOwned>(&self, file: DataFile) -> UmaResult<T> {
        let origin = || PathBuf::from(format!("{}/{}", self.key, file.name()));
        match self.tables.get(&file) {
            Some(Payload::Value(value)) => {
                T::deserialize(value).map_err(|source| UmaError::Parse { path: origin(), source })
            }
            Some(Payload::Text(text)) => {
                serde_json::from_str(text).map_err(|source| UmaError::Parse { path: origin(), source })
            }
            None => Err(UmaError::Validation {
                target: origin().display().to_string(),
                detail: "数据来源未提供该项（六项缺一不可）".to_string(),
            }),
        }
    }

    fn key(&self) -> String {
        self.key.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：文件名映射与目录来源的根目录
    #[test]
    fn test_file_names() {
        assert_eq!(DataFile::Manifest.name(), "manifest.json");
        assert_eq!(DataFile::Uma.name(), "umaDB.json");
        assert_eq!(DataFile::Text.name(), "text_data_dict.json");

        let src = DirSource::new("gamedata");
        assert_eq!(src.root(), Path::new("gamedata"));
        assert_eq!(src.key(), "gamedata");
    }

    /// 层1：内存来源两条形态取到同一结果，缺项与坏文本报错并指出来源
    #[test]
    fn test_memory_source() -> UmaResult<()> {
        let text = r#"{"a": 1, "b": [2, 3]}"#;
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
        let src = MemorySource::new("mem")
            .with_text(DataFile::Constants, text)
            .with_json(DataFile::Uma, value);

        let from_text: BTreeMap<String, serde_json::Value> = src.table(DataFile::Constants)?;
        let from_json: BTreeMap<String, serde_json::Value> = src.table(DataFile::Uma)?;
        assert_eq!(from_text["a"], serde_json::json!(1));
        assert_eq!(from_json["b"], serde_json::json!([2, 3]));
        assert_eq!(src.key(), "mem");

        let missing = src.table::<serde_json::Value>(DataFile::Card).unwrap_err();
        assert!(missing.to_string().contains("mem"), "缺项错误应指出来源: {missing}");
        assert!(
            missing.to_string().contains("cardDB.json"),
            "缺项错误应指出文件: {missing}"
        );

        let broken = MemorySource::new("mem").with_text(DataFile::Uma, "{ not json }");
        let err = broken.table::<serde_json::Value>(DataFile::Uma).unwrap_err();
        assert!(err.to_string().contains("umaDB.json"), "解析错误应指出文件: {err}");
        Ok(())
    }
}
