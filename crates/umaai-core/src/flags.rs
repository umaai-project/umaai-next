//! 局面状态标识（跨边界 DTO）
//!
//! 同时被事件数据（`add_flags` / `remove_flags`）、局面状态与协议层引用，故置核心层。
//! 本模块只放数据形状与位运算；置 / 清语义归 `umasim`。人读形态分两层：
//! 纯文本 [`fmt::Display`]（平台无关的值形态）在本层，富呈现（着色 / 表格）归 `umaai-cli`。

use std::fmt;

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{Error as DeError, Visitor},
};

/// 生病（状态异常）位集：可多种并存
///
/// 位含义见各常量：`NONE` 为无病（`Default`）；`UNKNOWN` 为「有病但种类未知」；
/// `ANY` 为通配，仅解除语境有意义。[`Self::BAD_TRAINER`] 与 [`UmaFlags::bad_trainer`]
/// 同源：由 `umasim` 保证两者同置同清。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Ill(u8);

impl Ill {
    /// 无病（空集）
    pub const NONE: Self = Self(0);
    /// 太り気味（变胖）
    pub const FAT: Self = Self(1 << 0);
    /// 肌あれ（皮肤差）
    pub const SKIN: Self = Self(1 << 1);
    /// 片頭痛（偏头痛）
    pub const HEADACHE: Self = Self(1 << 2);
    /// なまけ癖（摸鱼）
    pub const LAZY: Self = Self(1 << 3);
    /// 夜ふかし気味
    pub const NIGHT: Self = Self(1 << 4);
    /// 練習ベタ（不擅长训练）
    pub const BAD_TRAINER: Self = Self(1 << 5);
    /// 有病但种类未知（协议只给布尔时的降级）
    pub const UNKNOWN: Self = Self(1 << 6);
    /// 通配：仅解除语境有意义（事件数据的 `true` 映射至此）
    pub const ANY: Self = Self(1 << 7);
    /// 全部具名种类位（不含 [`Self::UNKNOWN`] / [`Self::ANY`]）
    pub const ALL_KINDS: Self =
        Self(Self::FAT.0 | Self::SKIN.0 | Self::HEADACHE.0 | Self::LAZY.0 | Self::NIGHT.0 | Self::BAD_TRAINER.0);

    /// 裸位集
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// 由裸位集构造
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    /// 无病（空集）
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// 是否包含 `other` 的全部位
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// 置位
    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    /// 清位
    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }
}

impl fmt::Display for Ill {
    /// 置位病名以「、」相连（`变胖、熬夜`）；空集为空串
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const KINDS: [(Ill, &str); 6] = [
            (Ill::FAT, "变胖"),
            (Ill::SKIN, "皮肤差"),
            (Ill::HEADACHE, "偏头痛"),
            (Ill::LAZY, "摸鱼"),
            (Ill::NIGHT, "熬夜"),
            (Ill::BAD_TRAINER, "不擅长训练"),
        ];
        let mut parts: Vec<&str> = vec![];
        for (bit, name) in KINDS {
            if self.contains(bit) {
                parts.push(name);
            }
        }
        if self.contains(Ill::UNKNOWN) {
            parts.push("未知病");
        }
        if self.contains(Ill::ANY) {
            parts.push("任意病");
        }
        f.write_str(&parts.join("、"))
    }
}

impl fmt::Display for UmaFlags {
    /// 已置状态词以「、」相连（计数随括号）；全默认为空串
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<String> = vec![];
        if self.qiezhe {
            parts.push("切者".into());
        }
        if self.xiaoqie {
            parts.push("小切".into());
        }
        if self.aijiao {
            parts.push("爱娇".into());
        }
        if self.good_trainer {
            parts.push("擅长训练".into());
        }
        if self.bad_trainer {
            parts.push("不擅长训练".into());
        }
        if self.positive_thinking_count > 0 {
            parts.push(format!("正向思考×{}", self.positive_thinking_count));
        }
        if self.refresh_mind > 0 {
            parts.push(format!("休息心得×{}", self.refresh_mind));
        }
        if self.lucky_count > 0 {
            parts.push(format!("幸运体质×{}", self.lucky_count));
        }
        if self.doll {
            parts.push("抓过娃娃".into());
        }
        if !self.ill.is_empty() {
            let ill = self.ill;
            parts.push(format!("生病({ill})"));
        }
        f.write_str(&parts.join("、"))
    }
}

impl Serialize for Ill {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u8(self.0)
    }
}

impl<'de> Deserialize<'de> for Ill {
    /// 兼容两种输入形态：布尔（`true` = 通配 [`Ill::ANY`]）与整数位集
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct IllVisitor;

        impl<'de> Visitor<'de> for IllVisitor {
            type Value = Ill;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("生病位集整数或布尔")
            }

            fn visit_bool<E: DeError>(self, v: bool) -> Result<Ill, E> {
                Ok(if v { Ill::ANY } else { Ill::NONE })
            }

            fn visit_u64<E: DeError>(self, v: u64) -> Result<Ill, E> {
                u8::try_from(v)
                    .map(Ill)
                    .map_err(|_| E::custom(format!("生病位集超出 u8: {v}")))
            }

            fn visit_i64<E: DeError>(self, v: i64) -> Result<Ill, E> {
                u8::try_from(v)
                    .map(Ill)
                    .map_err(|_| E::custom(format!("生病位集超出 u8: {v}")))
            }
        }

        d.deserialize_any(IllVisitor)
    }
}

/// 局面状态标识
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UmaFlags {
    /// 切者
    #[serde(default)]
    pub qiezhe: bool,
    /// 小切
    #[serde(default)]
    pub xiaoqie: bool,
    /// 爱娇
    #[serde(default)]
    pub aijiao: bool,
    /// 擅长训练
    #[serde(default)]
    pub good_trainer: bool,
    /// 不擅长训练（与 [`Ill::BAD_TRAINER`] 同源）
    #[serde(default)]
    pub bad_trainer: bool,
    /// 正向思考（心情盾）剩余次数
    #[serde(default)]
    pub positive_thinking_count: i32,
    /// 休息心得持续回合数
    #[serde(default)]
    pub refresh_mind: i32,
    /// 幸运体质次数
    #[serde(default)]
    pub lucky_count: i32,
    /// 是否抓过娃娃
    #[serde(default)]
    pub doll: bool,
    /// 生病
    #[serde(default)]
    pub ill: Ill,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：位集基本运算
    #[test]
    fn test_ill_bits() {
        let mut ill = Ill::NONE;
        assert!(ill.is_empty());
        ill.insert(Ill::FAT);
        ill.insert(Ill::SKIN);
        assert!(ill.contains(Ill::FAT) && ill.contains(Ill::SKIN));
        assert!(!ill.contains(Ill::LAZY));
        ill.remove(Ill::FAT);
        assert!(!ill.contains(Ill::FAT) && ill.contains(Ill::SKIN));
        assert_eq!(ill.bits(), Ill::SKIN.bits());
        assert_eq!(Ill::from_bits(ill.bits()), ill);
    }

    /// 层1：具名种类位互不重叠，且 `ALL_KINDS` 恰好是它们的并集
    #[test]
    fn test_ill_kinds_disjoint() {
        let kinds = [
            Ill::FAT,
            Ill::SKIN,
            Ill::HEADACHE,
            Ill::LAZY,
            Ill::NIGHT,
            Ill::BAD_TRAINER,
        ];
        let mut union = Ill::NONE;
        for k in kinds {
            assert!(!union.contains(k), "具名位重复: {k:?}");
            union.insert(k);
        }
        assert_eq!(union, Ill::ALL_KINDS);
        assert!(!Ill::ALL_KINDS.contains(Ill::UNKNOWN) && !Ill::ALL_KINDS.contains(Ill::ANY));
    }

    /// 层1：serde 双形态——布尔（事件数据）与整数（协议位集）
    #[test]
    fn test_ill_serde() {
        let t: Ill = serde_json::from_str("true").unwrap();
        assert_eq!(t, Ill::ANY);
        let f: Ill = serde_json::from_str("false").unwrap();
        assert_eq!(f, Ill::NONE);

        let n: Ill = serde_json::from_str("6").unwrap();
        let mut want = Ill::NONE;
        want.insert(Ill::SKIN);
        want.insert(Ill::HEADACHE);
        assert_eq!(n, want);
        assert_eq!(serde_json::to_string(&n).unwrap(), "6");
        assert!(serde_json::from_str::<Ill>("256").is_err(), "超出 u8 应报错");
    }

    /// 层1：事件数据里的 `ill: true` 按通配位解析
    #[test]
    fn test_event_flags_serde() {
        let flags: UmaFlags = serde_json::from_str(r#"{"ill": true, "bad_trainer": true}"#).unwrap();
        assert_eq!(flags.ill, Ill::ANY);
        assert!(flags.bad_trainer);
        assert_eq!(UmaFlags::default().ill, Ill::NONE);
    }

    /// 层1：病位集纯文本形态（具名位相连、空集为空）
    #[test]
    fn test_ill_display() {
        assert_eq!(Ill::NONE.to_string(), "");
        let mut ill = Ill::NONE;
        ill.insert(Ill::FAT);
        ill.insert(Ill::NIGHT);
        assert_eq!(ill.to_string(), "变胖、熬夜");
        let mut wild = Ill::NONE;
        wild.insert(Ill::UNKNOWN);
        wild.insert(Ill::BAD_TRAINER);
        assert_eq!(wild.to_string(), "不擅长训练、未知病");
    }

    /// 层1：状态标识纯文本形态（计数随括号、生病嵌套、全默认为空）
    #[test]
    fn test_flags_display() {
        assert_eq!(UmaFlags::default().to_string(), "");
        let mut flags = UmaFlags {
            qiezhe: true,
            aijiao: true,
            positive_thinking_count: 2,
            ..UmaFlags::default()
        };
        flags.ill.insert(Ill::FAT);
        assert_eq!(flags.to_string(), "切者、爱娇、正向思考×2、生病(变胖)");
    }
}
