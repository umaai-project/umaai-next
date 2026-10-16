//! gamedata 静态数据的纯 schema/DTO
//!
//! 只含数据形状与零依赖纯函数：不带全局读取、不带呈现。载入与校验归 `data` 模块，
//! 依赖全局数据的语义构造归 `umasim`。

pub mod card;
pub mod constants;
pub mod event;
pub mod uma;

pub use card::{CardValue, SupportCardData};
pub use constants::GameConstants;
pub use event::{ActionValue, ChoiceResult, EventChoice, EventCollection, EventData, TriggerType};
pub use uma::{FreeRaceData, UmaData};
