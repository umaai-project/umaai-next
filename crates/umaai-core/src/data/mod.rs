//! 静态数据加载与全局便捷层
//!
//! 数据来源由宿主注入（见 [`DataSource`]）：目录、内嵌资产、内存 JSON 三种形态都支持，
//! 一次载入 5 张静态表，经清单与语义校验后产出值类型 [`Data`]。载入**不写日志**：载入
//! 统计经 [`Data::stat`] 返回，由上层打印。
//!
//! 自由比赛掩码依赖「比赛等级表」（用户可调，属配置层），故不在载入时计算，
//! 由 `umasim` 在配置就绪后补算。

pub mod manifest;
pub mod source;
pub mod validate;

use std::{collections::BTreeMap, path::Path, sync::OnceLock};

use crate::{
    UmaError, UmaResult,
    gamedata::{EventCollection, EventData, GameConstants, SupportCardData, UmaData},
};

pub use source::{DataFile, DataSource, DirSource, MemorySource};

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
    /// 从任意数据来源载入全部静态表
    ///
    /// 顺序：清单校验（版本 + 必需文件）→ 逐表解析 → 语义校验。
    pub fn from_source<S: DataSource>(source: &S) -> UmaResult<Self> {
        let manifest: manifest::Manifest = source.table(DataFile::Manifest)?;
        manifest.check()?;
        let data = Self {
            uma: source.table(DataFile::Uma)?,
            card: source.table(DataFile::Card)?,
            text: source.table(DataFile::Text)?,
            events: source.table(DataFile::Events)?,
            consts: source.table(DataFile::Constants)?,
        };
        validate::validate(&data)?;
        Ok(data)
    }

    /// 载入数据根 `root` 下的全部静态表（目录来源的便捷入口）
    pub fn load(root: impl AsRef<Path>) -> UmaResult<Self> {
        Self::from_source(&DirSource::new(root))
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
        self.uma.get(&id.to_string()).ok_or_else(|| missing(DataFile::Uma, id))
    }

    /// 按 ID 取支援卡
    pub fn get_card(&self, id: u32) -> UmaResult<&SupportCardData> {
        self.card
            .get(&id.to_string())
            .ok_or_else(|| missing(DataFile::Card, id))
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

/// 全局数据实例与来源标识（初始化后只读；放在同一锁里，杜绝「实例与标识不同步」）
static GLOBAL: OnceLock<(String, Data)> = OnceLock::new();

impl Data {
    /// 初始化全局数据（幂等），数据根路径形态
    ///
    /// 已初始化且数据根相同 → 返回既有实例；数据根不同 → 报错（不做热更，换数据需重启）。
    /// 同进程内应只在启动时调用一次：并发首启由 [`OnceLock`] 裁决谁装上，**不按调用先后**，
    /// 落败的一方按标识不匹配报错（不静默改用别人的数据）。
    pub fn init(root: impl AsRef<Path>) -> UmaResult<&'static Data> {
        Self::init_from(&DirSource::new(root))
    }

    /// 初始化全局数据（幂等），任意数据来源
    ///
    /// 幂等比对用[来源标识](DataSource::key)：标识相同 → 返回既有实例；不同 → 报错。
    pub fn init_from<S: DataSource>(source: &S) -> UmaResult<&'static Data> {
        let key = source.key();
        if let Some((cur, data)) = GLOBAL.get() {
            return Self::same_source(cur, &key, data);
        }
        let data = Self::from_source(source)?;
        let (cur, stored) = GLOBAL.get_or_init(|| (key.clone(), data));
        Self::same_source(cur, &key, stored)
    }

    /// 校验来源标识一致（不一致即报错，不回退到已有实例）
    fn same_source(cur: &str, key: &str, data: &'static Data) -> UmaResult<&'static Data> {
        if cur == key {
            return Ok(data);
        }
        Err(UmaError::InvalidState(format!(
            "数据已用 {cur} 初始化，不能改为 {key}（换数据需重启）"
        )))
    }

    /// 取全局实例；未初始化时报错
    pub fn get() -> UmaResult<&'static Data> {
        Self::try_get().ok_or_else(|| UmaError::InvalidState("数据未初始化：先调用 Data::init(root)".to_string()))
    }

    /// 取全局实例；未初始化返回 `None`
    pub fn try_get() -> Option<&'static Data> {
        GLOBAL.get().map(|(_, data)| data)
    }
}

/// 构造「未找到 ID」的校验错误
fn missing(file: DataFile, id: u32) -> UmaError {
    UmaError::Validation {
        target: format!("{}/{}", file.name(), id),
        detail: "未找到该 ID（可能需要更新数据）".to_string(),
    }
}
