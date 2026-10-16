//! 静态数据加载与全局便捷层
//!
//! 单一数据根（由宿主注入路径）一次载入 5 张静态表，经清单与语义校验后产出值类型
//! [`Data`]。载入**不写日志**：载入统计经 [`Data::stat`] 返回，由上层打印。
//!
//! 自由比赛掩码依赖「比赛等级表」（用户可调，属配置层），故不在载入时计算，
//! 由 `umasim` 在配置就绪后补算。

pub mod manifest;
pub mod validate;

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use serde::de::DeserializeOwned;

use crate::{
    UmaError, UmaResult,
    gamedata::{EventCollection, EventData, GameConstants, SupportCardData, UmaData},
};

/// 静态数据（值类型）
#[derive(Debug)]
pub struct Data {
    /// 马娘表（键为 `gameId` 字符串）
    pub uma: BTreeMap<String, UmaData>,
    /// 支援卡表（键为 `cardId` 字符串）
    pub card: BTreeMap<String, SupportCardData>,
    /// 文本表：分组 → 条目 ID → 文本
    pub text: BTreeMap<String, BTreeMap<String, String>>,
    /// 事件表
    pub events: EventCollection,
    /// 游戏常量
    pub consts: GameConstants,
}

/// 载入统计（供上层打印）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadStat {
    /// 马娘条数
    pub uma: usize,
    /// 支援卡条数
    pub card: usize,
    /// 文本分组数
    pub text: usize,
    /// 事件总条数（五个分区之和）
    pub events: usize,
}

impl Data {
    /// 载入数据根 `root` 下的全部静态表
    ///
    /// 顺序：清单校验（版本 + 必需文件）→ 逐表解析 → 语义校验。
    pub fn load(root: impl AsRef<Path>) -> UmaResult<Self> {
        let root = root.as_ref();
        let _ = manifest::Manifest::load(root)?;
        let data = Self {
            uma: read_json(root, "umaDB.json")?,
            card: read_json(root, "cardDB.json")?,
            text: read_json(root, "text_data_dict.json")?,
            events: read_json(root, "events.json")?,
            consts: read_json(root, "constants.json")?,
        };
        validate::validate(&data)?;
        Ok(data)
    }

    /// 载入统计
    pub fn stat(&self) -> LoadStat {
        let e = &self.events;
        LoadStat {
            uma: self.uma.len(),
            card: self.card.len(),
            text: self.text.len(),
            events: e.story_events.len()
                + e.uma_events.len()
                + e.card_events.len()
                + e.friend_events.len()
                + e.system_events.len(),
        }
    }

    /// 按 ID 取马娘
    pub fn get_uma(&self, id: u32) -> UmaResult<&UmaData> {
        self.uma.get(&id.to_string()).ok_or_else(|| missing("umaDB.json", id))
    }

    /// 按 ID 取支援卡
    pub fn get_card(&self, id: u32) -> UmaResult<&SupportCardData> {
        self.card.get(&id.to_string()).ok_or_else(|| missing("cardDB.json", id))
    }

    /// 取角色名；未知返回 `"未知"`
    pub fn get_chara_name(&self, chara_id: u32) -> &str {
        self.text
            .get("6")
            .and_then(|m| m.get(&chara_id.to_string()))
            .map(String::as_str)
            .unwrap_or("未知")
    }

    /// 按键取系统事件
    pub fn system_event(&self, key: &str) -> UmaResult<&EventData> {
        self.events.system_events.get(key).ok_or_else(|| UmaError::Validation {
            target: format!("events.json/system_events/{key}"),
            detail: "未知系统事件".to_string(),
        })
    }

    /// 按键取事件概率
    pub fn event_prob(&self, key: &str) -> UmaResult<f64> {
        self.consts
            .event_probs
            .get(key)
            .copied()
            .ok_or_else(|| UmaError::Validation {
                target: format!("constants.json/event_probs/{key}"),
                detail: "未知事件概率".to_string(),
            })
    }
}

/// 全局数据实例（初始化后只读）
static GLOBAL: OnceLock<Data> = OnceLock::new();
/// 初始化时使用的数据根（用于幂等比对）
static GLOBAL_ROOT: OnceLock<PathBuf> = OnceLock::new();

impl Data {
    /// 初始化全局数据（幂等）
    ///
    /// 已初始化且数据根相同 → 返回既有实例；数据根不同 → 报错（不做热更，换数据需重启）。
    /// 并发首次初始化由 [`OnceLock`] 串行化；同进程内应只在启动时调用一次。
    pub fn init(root: impl AsRef<Path>) -> UmaResult<&'static Data> {
        let root = root.as_ref().to_path_buf();
        if let Some(existing) = GLOBAL.get() {
            return match GLOBAL_ROOT.get() {
                Some(cur) if *cur == root => Ok(existing),
                Some(cur) => Err(UmaError::InvalidState(format!(
                    "数据已用 {} 初始化，不能改为 {}（换数据需重启）",
                    cur.display(),
                    root.display()
                ))),
                None => Ok(existing),
            };
        }
        let data = Self::load(&root)?;
        let _ = GLOBAL_ROOT.set(root);
        Ok(GLOBAL.get_or_init(|| data))
    }

    /// 取全局实例；未初始化时报错
    pub fn get() -> UmaResult<&'static Data> {
        Self::try_get().ok_or_else(|| UmaError::InvalidState("数据未初始化：先调用 Data::init(root)".to_string()))
    }

    /// 取全局实例；未初始化返回 `None`
    pub fn try_get() -> Option<&'static Data> {
        GLOBAL.get()
    }
}

/// 读并解析数据文件
fn read_json<T: DeserializeOwned>(root: &Path, name: &str) -> UmaResult<T> {
    let path = root.join(name);
    let text = std::fs::read_to_string(&path).map_err(|source| UmaError::Io { path: path.clone(), source })?;
    serde_json::from_str(&text).map_err(|source| UmaError::Parse { path, source })
}

/// 构造「未找到 ID」的校验错误
fn missing(file: &str, id: u32) -> UmaError {
    UmaError::Validation {
        target: format!("{file}/{id}"),
        detail: "未找到该 ID（可能需要更新数据）".to_string(),
    }
}
