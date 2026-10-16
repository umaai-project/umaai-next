//! 马娘数据（umaDB.json）的纯 schema/DTO

use serde::{Deserialize, Serialize};

use crate::{Array5, RaceMask, UmaError, UmaResult};

/// 自由比赛区间数据
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreeRaceData {
    /// 开始回合（游戏回合号）
    pub start_turn: u32,
    /// 结束回合
    pub end_turn: u32,
    /// 比赛次数
    pub count: u32,
    /// 比赛等级门槛；有值时只标记等级不低于它（数值更小）的比赛
    pub grade: Option<u32>,
    /// 可打回合的位段，json 中不存在，载入后由 [`Self::update_turn_mask`] 计算
    #[serde(default)]
    pub mask: RaceMask,
}

impl FreeRaceData {
    /// 按比赛等级表计算比赛掩码：可打的回合置位
    ///
    /// `race_grades` 为「回合 → 比赛等级」表，须覆盖 `[start_turn, end_turn]`；
    /// 表未覆盖时报错，回合越出位段窗口由 [`RaceMask`] 报错。
    pub fn update_turn_mask(&mut self, race_grades: &[i32]) -> UmaResult<()> {
        let mut mask = RaceMask::EMPTY;
        for turn in self.start_turn..=self.end_turn {
            let grade = race_grades.get(turn as usize).ok_or_else(|| UmaError::Validation {
                target: format!("freeRace/turn{turn}"),
                detail: format!("比赛等级表长度 {} 未覆盖回合 {turn}", race_grades.len()),
            })?;
            let hit = match self.grade {
                Some(g) => *grade <= g as i32,
                None => true,
            };
            if hit {
                mask.insert_turn(turn as i32)?;
            }
        }
        self.mask = mask;
        Ok(())
    }
}

/// 马娘数据（umaDB.json 单条）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UmaData {
    /// 马娘 ID
    pub game_id: u32,
    /// 星数
    pub star: u32,
    /// 名字
    pub name: String,
    /// 五维加成
    pub five_status_bonus: Array5,
    /// 初始五维
    pub five_status_initial: Array5,
    /// 生涯比赛回合
    pub races: Vec<i32>,
    /// 自由比赛区间
    pub free_races: Vec<FreeRaceData>,
    /// 特殊生涯赛程说明；无则 `None`
    #[serde(default)]
    pub race_note: Option<String>,
}

impl UmaData {
    /// 短名：取 `]` 之后的部分；无 `]` 时返回全名
    pub fn short_name(&self) -> &str {
        self.name.split(']').next_back().unwrap_or(&self.name)
    }

    /// 把生涯比赛回合压缩为位段（[`RaceMask`]）
    ///
    /// 回合超出位段窗口时报错（不 panic）。
    pub fn zip_races(&self) -> UmaResult<RaceMask> {
        RaceMask::from_turns(&self.races)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：短名取 `]` 之后的部分
    #[test]
    fn test_short_name() {
        let mut u = UmaData {
            game_id: 1,
            star: 3,
            name: "[特别梦想家]特别周".to_string(),
            five_status_bonus: [0; 5],
            five_status_initial: [0; 5],
            races: vec![],
            free_races: vec![],
            race_note: None,
        };
        assert_eq!(u.short_name(), "特别周");
        u.name = "无前缀".to_string();
        assert_eq!(u.short_name(), "无前缀");
    }

    /// 层1：生涯比赛压缩为位段，越界回合报错
    #[test]
    fn test_zip_races() {
        let races = vec![11, 26, 71];
        let u = UmaData {
            game_id: 1,
            star: 3,
            name: "x".to_string(),
            five_status_bonus: [0; 5],
            five_status_initial: [0; 5],
            races: races.clone(),
            free_races: vec![],
            race_note: None,
        };
        let mask = u.zip_races().unwrap();
        for r in races {
            assert!(mask.contains_turn(r), "回合 {r} 应置位");
        }
        assert_eq!(mask.count(), 3);

        let bad = UmaData { races: vec![5], ..u };
        assert!(bad.zip_races().is_err(), "回合 < 11 应报错");
    }

    /// 层1：自由比赛掩码只用等级不低于指定值的回合置位
    #[test]
    fn test_update_turn_mask() {
        // 回合 11..=15 的比赛等级依次 5/4/3/2/1（1 = G1 最高）
        let mut grades = vec![0i32; 75];
        for (i, g) in [5, 4, 3, 2, 1].iter().enumerate() {
            grades[11 + i] = *g;
        }
        let mut fr = FreeRaceData {
            start_turn: 11,
            end_turn: 15,
            count: 1,
            grade: Some(2),
            mask: RaceMask::EMPTY,
        };
        fr.update_turn_mask(&grades).unwrap();
        // 门槛 G2（等级值 ≤ 2）：只有回合 14(2)、15(1) 置位
        assert_eq!(fr.mask.count(), 2);
        assert!(fr.mask.contains_turn(14));
        assert!(!fr.mask.contains_turn(13), "等级 3 低于 G2，不应置位");

        // 无门槛时全部置位；等级表未覆盖则报错
        let mut fr2 = FreeRaceData { grade: None, ..fr };
        assert!(fr2.update_turn_mask(&grades).is_ok());
        assert_eq!(fr2.mask.count(), 5);
        assert!(fr2.update_turn_mask(&grades[..12]).is_err(), "表未覆盖应报错");
    }
}
