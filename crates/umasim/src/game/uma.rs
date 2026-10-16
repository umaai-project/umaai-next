//! 训练中的马娘（领域状态），剧本通用（固定为 5 星）

use anyhow::Result;
use serde::{Deserialize, Serialize};
use umaai_core::gamedata::{ActionValue, EventChoice, FreeRaceData, UmaData};
use umaai_core::{Array5, Data, Ill, RaceMask, UmaFlags};

/// [`UmaFlags`] 的置 / 清语义（core 只放形状，行为归本层）
///
/// 生病位集的通配规则：事件数据的 `ill: true` 反序列化为 [`Ill::ANY`]，
/// **累加**语境翻译为 [`Ill::UNKNOWN`]（有病但种类未知）；**解除**语境清空
/// 全部病位。`bad_trainer` 与 [`Ill::BAD_TRAINER`] 同源，任一路径触发时同置同清。
pub trait UmaFlagsExt {
    /// 局内获得【切者】：与【小切】互斥——获得切者时小切失效
    ///
    /// 小切为局外（育成开始前）获得，局内不会再次获得，故不需要反向处理
    /// （小切不会把切者清掉）。
    fn gain_qiezhe(&mut self) -> &mut Self;

    /// PT 折算系数（`pt_score_rate`）的额外倍数：切者 ×1.1、小切 ×1.04
    ///
    /// 两者互斥（见 [`Self::gain_qiezhe`]），判断顺序无关。只乘 PT 项，
    /// 不改变 [`Uma::total_pt`] 的 PT 数量口径。
    fn pt_score_rate_factor(&self) -> f32;

    /// 累加状态（事件 `add_flags` 路径）
    fn add(&mut self, rhs: &UmaFlags) -> &mut Self;

    /// 减少状态（事件 `remove_flags` 路径）；次数类下限 0
    fn remove(&mut self, rhs: &UmaFlags) -> &mut Self;
}

impl UmaFlagsExt for UmaFlags {
    fn gain_qiezhe(&mut self) -> &mut Self {
        self.qiezhe = true;
        self.xiaoqie = false;
        self
    }

    fn pt_score_rate_factor(&self) -> f32 {
        if self.qiezhe {
            1.1
        } else if self.xiaoqie {
            1.04
        } else {
            1.0
        }
    }

    fn add(&mut self, rhs: &UmaFlags) -> &mut Self {
        if rhs.qiezhe {
            // 互斥：获得切者会清掉小切
            self.gain_qiezhe();
        } else if rhs.xiaoqie {
            self.xiaoqie = true;
        }
        self.aijiao |= rhs.aijiao;
        self.good_trainer |= rhs.good_trainer;
        self.bad_trainer |= rhs.bad_trainer;
        self.positive_thinking_count += rhs.positive_thinking_count;
        self.refresh_mind += rhs.refresh_mind;
        self.lucky_count += rhs.lucky_count;
        self.doll |= rhs.doll;
        // 累加语境：通配位翻译为「未知」位，具名位照搬（不把 ANY 位带进状态）
        let mut ill = rhs.ill;
        if ill.contains(Ill::ANY) {
            ill.remove(Ill::ANY);
            ill.insert(Ill::UNKNOWN);
        }
        self.ill.insert(ill);
        // 同置：bad_trainer 与 BAD_TRAINER 位任一来源触发即两边都置
        if self.bad_trainer || self.ill.contains(Ill::BAD_TRAINER) {
            self.bad_trainer = true;
            self.ill.insert(Ill::BAD_TRAINER);
        }
        self
    }

    fn remove(&mut self, rhs: &UmaFlags) -> &mut Self {
        self.qiezhe &= !rhs.qiezhe;
        self.xiaoqie &= !rhs.xiaoqie;
        self.aijiao &= !rhs.aijiao;
        self.good_trainer &= !rhs.good_trainer;
        self.bad_trainer &= !rhs.bad_trainer;
        // 次数类状态按下限 0 扣减（心情盾被消耗、幸运体质失效）
        self.positive_thinking_count = (self.positive_thinking_count - rhs.positive_thinking_count).max(0);
        self.refresh_mind -= rhs.refresh_mind;
        self.lucky_count = (self.lucky_count - rhs.lucky_count).max(0);
        self.doll &= !rhs.doll;
        // 解除语境：通配位清空全部病位，否则按位解除
        if rhs.ill.contains(Ill::ANY) {
            self.ill = Ill::NONE;
        } else {
            self.ill.remove(rhs.ill);
        }
        // 同清：bad_trainer 与 BAD_TRAINER 位任一被解除即两边都清
        if rhs.bad_trainer || rhs.ill.contains(Ill::BAD_TRAINER) {
            self.bad_trainer = false;
            self.ill.remove(Ill::BAD_TRAINER);
        }
        self
    }
}

/// 训练中的马娘信息，剧本通用（固定为5星）
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Uma {
    /// 马娘编号
    pub uma_id: u32,
    /// 体力
    pub vital: i32,
    /// 最大体力
    pub max_vital: i32,
    /// 干劲 [1, 5]
    pub motivation: i32,
    /// 当前属性。1200以上不减半
    pub five_status: Array5,
    /// 属性加成
    pub five_status_bonus: Array5,
    /// 属性上限
    pub five_status_limit: Array5,
    /// 剩余技能点
    pub skill_pt: i32,
    /// 已学技能评分
    pub skill_score: i32,
    /// 总共打折级数
    pub total_hints: i32,
    /// 比赛加成
    pub race_bonus: i32,
    /// Buff状态
    pub flags: UmaFlags,
    /// 生涯比赛位段（bit i ↔ 回合 11+i）
    pub career_races: RaceMask,
    /// 比赛场次位段（bit i ↔ 回合 11+i）
    pub win_races: RaceMask,
    /// 自选比赛区间（掩码按比赛等级表在构造时算好，派生态不入序列化）
    #[serde(skip)]
    pub free_races: Vec<FreeRaceData>,
}

/// `calc_score()` 的可归因分量分解
///
/// 七个分量之和逐位等于 [`Uma::calc_score`]，用于搜索层的终局归因统计。
/// PT 项不可再拆成 skill_pt 与 hint 的独立贡献：`total_pt()` 内有一次 `floor()`、
/// 外面又有一次 `as i32`，两层截断使其数学上不可分。五维记的是查表后的分数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScoreParts {
    /// 技能分（`skill_score` 原值）
    pub skill: i32,
    /// PT 折算分：`(total_pt() as f32 * pt_score_rate) as i32`
    pub pt: i32,
    /// 五维各自的查表得分（速耐力根智），已按 limit 截断
    pub five_status: [i32; 5],
}

impl ScoreParts {
    /// 七个分量之和，逐位等于 [`Uma::calc_score`]
    pub fn total(&self) -> i32 {
        self.skill + self.pt + self.five_status.iter().copied().sum::<i32>()
    }
}

impl Uma {
    /// 取静态数据里的马娘条目
    pub fn get_data(&self) -> Result<&'static UmaData> {
        Ok(Data::get()?.get_uma(self.uma_id)?)
    }

    /// 角色ID（高4位）
    pub fn chara_id(&self) -> u32 {
        self.uma_id / 100
    }

    /// 建立马娘对象
    ///
    /// `limit_base` 是**所在剧本**的五维上限基值（不含继承），由调用方取；
    /// `race_grades` 是「回合 → 比赛等级」表（归配置层），用于计算自选比赛
    /// 区间掩码——基值与等级表都在构造时一次写对，之后只做加法。
    pub fn new(id: u32, limit_base: Array5, race_grades: &[i32]) -> Result<Self> {
        let data = Data::get()?.get_uma(id)?;
        let mut free_races = data.free_races.clone();
        for f in &mut free_races {
            f.update_turn_mask(race_grades)?;
        }
        Ok(Self {
            uma_id: id,
            vital: 100,
            max_vital: 100,
            motivation: 3,
            five_status: data.five_status_initial,
            five_status_bonus: data.five_status_bonus,
            five_status_limit: limit_base,
            skill_score: 510, // 固有按5星计算
            total_hints: 21,  // 按全部初始技能3级打折计算
            career_races: data.zip_races()?,
            free_races,
            ..Default::default()
        })
    }

    /// 该回合是否为生涯比赛回合（URA 决赛 73/75/77 恒真）
    pub fn is_race_turn(&self, turn: i32) -> bool {
        if turn == 73 || turn == 75 || turn == 77 {
            true
        } else {
            self.career_races.contains_turn(turn)
        }
    }

    /// 设置第x回合为已比赛状态，用于统计自选比赛；越窗回合忽略
    pub fn set_race(&mut self, turn: i32) {
        if (RaceMask::MIN_TURN..=72).contains(&turn) {
            let _ = self.win_races.insert_turn(turn);
        }
    }

    /// 计算技能点和总Hint等级换算得到的总pt数，不包括已学习的技能
    pub fn total_pt(&self) -> Result<i32> {
        Ok((self.skill_pt as f32 + self.total_hints as f32 * Data::get()?.consts.hint_pt_rate).floor() as i32)
    }

    /// 把 [`Self::calc_score`] 分解成可归因分量（七个分量之和逐位等于总分）
    ///
    /// PT 项额外乘切者 / 小切折算系数（见 [`UmaFlagsExt::pt_score_rate_factor`]）：
    /// 这是**终局评分**口径，不改变 [`Self::total_pt`] 的 PT 数量。
    pub fn score_parts(&self) -> Result<ScoreParts> {
        let cons = &Data::get()?.consts;
        let mut five_status = [0i32; 5];
        for i in 0..5 {
            let status = self.five_status[i].min(self.five_status_limit[i]);
            five_status[i] = cons.status_final_score(status);
        }
        Ok(ScoreParts {
            skill: self.skill_score,
            pt: (self.total_pt()? as f32 * cons.pt_score_rate * self.flags.pt_score_rate_factor()) as i32,
            five_status,
        })
    }

    /// 正常计算评分，等于 [`Self::score_parts`] 七个分量之和
    pub fn calc_score(&self) -> Result<i32> {
        Ok(self.score_parts()?.total())
    }

    /// 增减干劲：**掉心情时优先消耗【心情盾】**（`positive_thinking_count`），盾为 0 才真掉
    ///
    /// 心情盾按「防一次掉心情」计：一次扣减（无论扣几级，如大失败的 -3）消耗一层盾，
    /// 心情不变；盾为 0 时按原规则 `max(1).min(5)` 夹取。心情上涨不消耗盾。
    pub fn add_motivation(&mut self, delta: i32) -> &mut Self {
        if delta < 0 && self.flags.positive_thinking_count > 0 {
            self.flags.positive_thinking_count -= 1;
            return self;
        }
        self.motivation = (self.motivation + delta).max(1).min(5);
        self
    }

    /// 应用一次训练 / 事件效果（五维截上限、PT 累加、体力夹取）
    pub fn add_value(&mut self, action: &ActionValue) -> &mut Self {
        for i in 0..5 {
            self.five_status[i] = (self.five_status[i] + action.status_pt[i]).min(self.five_status_limit[i]);
        }
        self.skill_pt += action.status_pt[5];
        self.add_motivation(action.motivation);
        self.max_vital += action.max_vital;
        self.vital = (self.vital + action.vital).min(self.max_vital).max(0);
        self.total_hints += action.hint_level;
        self
    }

    /// 根据事件选项更新 Flag 状态
    pub fn update_flags(&mut self, choice: &EventChoice) -> &mut Self {
        if let Some(flags) = &choice.add_flags {
            self.flags.add(flags);
        }
        if let Some(flags) = &choice.remove_flags {
            self.flags.remove(flags);
        }
        self
    }

    /// 返回自选比赛场数（区间掩码与已赛位段的交集）
    pub fn count_free_race(&self, free: &FreeRaceData) -> u32 {
        (self.win_races.bits() & free.mask.bits()).count_ones()
    }

    /// 自选比赛是否全部达标（供任意时点比对，无区间要求的马娘恒为 `true`）
    pub fn all_free_races_done(&self) -> bool {
        self.free_races.iter().all(|f| self.count_free_race(f) >= f.count)
    }

    /// 返回当前所处的自选比赛区间
    pub fn find_free_race(&self, turn: i32) -> Option<&FreeRaceData> {
        self.free_races
            .iter()
            .find(|f| f.start_turn <= turn as u32 && f.end_turn >= turn as u32)
    }

    /// 从位段转出已赛回合列表（升序）
    pub fn list_races(&self) -> Vec<i32> {
        (RaceMask::MIN_TURN..=RaceMask::MAX_TURN)
            .filter(|&t| self.win_races.contains_turn(t))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use rand::{SeedableRng, rngs::StdRng};

    use super::*;
    use crate::game::FriendState;
    use umaai_core::testing::data_root;

    /// 比赛等级表夹具（与旧仓默认 72 项一致，仅测试用）
    const RACE_GRADES: [i32; 72] = [
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 3, 4, 3, 3, 4, 3, 3, 2, 3, 1, 1, 3, 4, 3, 4, 2, 2, 1, 2, 1, 1, 1, 1, 1,
        3, 3, 3, 3, 1, 1, 1, 1, 1, 2, 1, 2, 3, 1, 1, 2, 1, 2, 1, 1, 2, 1, 1, 3, 3, 3, 3, 3, 1, 2, 1, 1, 1, 2, 1,
    ];

    fn init() -> Result<()> {
        Data::init(data_root()?)?;
        Ok(())
    }

    /// 次数类状态：累加、扣减下限 0；切者互斥；ill 位集同置同清
    #[test]
    fn test_flag_counts_add_remove() {
        let mut flags = UmaFlags::default();
        flags.add(&UmaFlags {
            xiaoqie: true,
            positive_thinking_count: 3,
            lucky_count: 2,
            ..Default::default()
        });
        assert_eq!(flags.positive_thinking_count, 3, "心情盾次数应累加");
        assert_eq!(flags.lucky_count, 2, "幸运体质次数应累加");
        assert!(flags.xiaoqie, "小切应被置位");

        // 消耗一次心情盾 + 幸运体质失效
        flags.remove(&UmaFlags {
            positive_thinking_count: 1,
            lucky_count: 2,
            ..Default::default()
        });
        assert_eq!(flags.positive_thinking_count, 2, "消耗一次心情盾应只扣 1");
        assert_eq!(flags.lucky_count, 0, "幸运体质应扣到 0");

        // 超额扣减不得出现负数
        flags.remove(&UmaFlags {
            positive_thinking_count: 5,
            ..Default::default()
        });
        assert_eq!(flags.positive_thinking_count, 0, "次数不应扣成负数");

        flags.remove(&UmaFlags {
            xiaoqie: true,
            ..Default::default()
        });
        assert!(!flags.xiaoqie, "小切应可移除");

        // bad_trainer 与 BAD_TRAINER 位同置同清
        let mut flags = UmaFlags::default();
        flags.add(&UmaFlags {
            bad_trainer: true,
            ..Default::default()
        });
        assert!(flags.bad_trainer && flags.ill.contains(Ill::BAD_TRAINER), "同置");
        flags.remove(&UmaFlags {
            ill: Ill::BAD_TRAINER,
            ..Default::default()
        });
        assert!(!flags.bad_trainer && !flags.ill.contains(Ill::BAD_TRAINER), "同清");

        // ill 通配位：累加翻译为 UNKNOWN、解除清空全部病位
        let mut flags = UmaFlags::default();
        flags.add(&UmaFlags {
            ill: Ill::ANY,
            ..Default::default()
        });
        assert!(flags.ill.contains(Ill::UNKNOWN) && !flags.ill.contains(Ill::ANY));
        flags.add(&UmaFlags {
            ill: Ill::FAT,
            ..Default::default()
        });
        assert!(flags.ill.contains(Ill::FAT));
        flags.remove(&UmaFlags {
            ill: Ill::ANY,
            ..Default::default()
        });
        assert!(flags.ill.is_empty(), "通配解除应清空全部病位");
    }

    /// 心情盾：掉心情优先消耗 `positive_thinking_count`，盾为 0 才真掉心情
    #[test]
    fn test_motivation_shield() {
        // 一层盾挡下一次掉心情
        let mut uma = Uma::default();
        uma.motivation = 4;
        uma.flags.positive_thinking_count = 1;
        uma.add_motivation(-1);
        assert_eq!(
            (uma.motivation, uma.flags.positive_thinking_count),
            (4, 0),
            "一层盾应挡下 -1 且心情不变"
        );

        // 大失败 -3 也只消耗一层盾
        let mut uma = Uma::default();
        uma.motivation = 4;
        uma.flags.positive_thinking_count = 2;
        uma.add_motivation(-3);
        assert_eq!(
            (uma.motivation, uma.flags.positive_thinking_count),
            (4, 1),
            "一次扣减只消耗一层盾"
        );

        // 盾为 0：真掉心情，下限 1
        let mut uma = Uma::default();
        uma.motivation = 4;
        uma.add_motivation(-1);
        assert_eq!(uma.motivation, 3, "无盾时应真掉心情");
        uma.motivation = 1;
        uma.add_motivation(-2);
        assert_eq!(uma.motivation, 1, "心情下限仍为 1");

        // 涨心情不消耗盾，上限 5
        let mut uma = Uma::default();
        uma.motivation = 3;
        uma.flags.positive_thinking_count = 2;
        uma.add_motivation(2);
        assert_eq!(
            (uma.motivation, uma.flags.positive_thinking_count),
            (5, 2),
            "上涨不消耗盾且上限为 5"
        );
    }

    /// 切者 / 小切的互斥与 PT 折算系数
    #[test]
    fn test_qiezhe_xiaoqie_exclusive_and_factor() {
        let mut flags = UmaFlags {
            xiaoqie: true,
            ..Default::default()
        };
        flags.gain_qiezhe();
        assert!(flags.qiezhe && !flags.xiaoqie, "获得切者应清掉小切");

        let mut flags = UmaFlags {
            xiaoqie: true,
            ..Default::default()
        };
        flags.add(&UmaFlags {
            qiezhe: true,
            ..Default::default()
        });
        assert!(flags.qiezhe && !flags.xiaoqie, "事件 add 路径应清掉小切");

        let factor = |qiezhe: bool, xiaoqie: bool| {
            UmaFlags {
                qiezhe,
                xiaoqie,
                ..Default::default()
            }
            .pt_score_rate_factor()
        };
        assert_eq!(factor(false, false), 1.0, "无状态不加成");
        assert_eq!(factor(true, false), 1.1, "切者 ×1.1");
        assert_eq!(factor(false, true), 1.04, "小切 ×1.04");
        assert_eq!(factor(true, true), 1.1, "两者同时存在时以切者为准");
    }

    /// 基本构造：初始五维 / 上限 / 生涯位段 / 自选区间掩码
    #[test]
    fn test_uma() -> Result<()> {
        init()?;
        let limit = Data::get()?.consts.five_status_limit_base;
        let uma = Uma::new(101901, limit, &RACE_GRADES)?;
        assert_eq!(uma.five_status, uma.get_data()?.five_status_initial);
        assert_eq!(uma.five_status_limit, limit);
        assert_eq!((uma.vital, uma.max_vital, uma.motivation), (100, 100, 3));
        // 生涯位段与数据条目一致
        let races = &uma.get_data()?.races;
        assert_eq!(uma.career_races.count(), races.len() as u32);
        for &t in races {
            assert!(uma.is_race_turn(t), "回合 {t} 应为生涯比赛");
        }
        assert!(uma.is_race_turn(73) && uma.is_race_turn(75) && uma.is_race_turn(77));
        assert!(!uma.is_race_turn(0) && !uma.is_race_turn(10));
        println!("uma: {uma:?}");
        Ok(())
    }

    /// 已赛位段 → 回合列表；set_race 越窗忽略
    #[test]
    fn test_win_races() -> Result<()> {
        init()?;
        let limit = Data::get()?.consts.five_status_limit_base;
        let mut uma = Uma::new(101901, limit, &RACE_GRADES)?;
        for t in [11, 12, 72] {
            uma.set_race(t);
        }
        uma.set_race(80); // 越窗忽略
        assert_eq!(uma.list_races(), vec![11, 12, 72]);
        // find_free_race / count_free_race 走构造时算好的掩码
        if let Some(free) = uma.find_free_race(30) {
            println!("区间: {free:?} 已赛 {}", uma.count_free_race(free));
        }
        Ok(())
    }

    /// 按文档公式独立重算七个分量，对照 `score_parts()`
    fn expected_score_parts(uma: &Uma) -> ScoreParts {
        let cons = &Data::get().expect("数据未初始化").consts;
        let mut five_status = [0i32; 5];
        for i in 0..5 {
            // 刻意不调 `cons.status_final_score()`：这份是用来对照 `score_parts()` 的
            // 独立实现，两边共用同一个查表函数就不再是 oracle 了。
            let table = &cons.five_status_final_score;
            let status = uma.five_status[i].min(uma.five_status_limit[i]).max(0) as usize;
            five_status[i] = table[status.min(table.len() - 1)];
        }
        // 切者/小切的 PT 折算加成：独立写死系数，保证能抓住实现里的系数写错
        let pt_factor = if uma.flags.qiezhe {
            1.1
        } else if uma.flags.xiaoqie {
            1.04
        } else {
            1.0
        };
        ScoreParts {
            skill: uma.skill_score,
            pt: (uma.total_pt().expect("total_pt") as f32 * cons.pt_score_rate * pt_factor) as i32,
            five_status,
        }
    }

    /// 打印并断言七个分量与公式逐位相等
    fn check_score_parts_case(label: &str, uma: &Uma) {
        let parts = uma.score_parts().expect("score_parts");
        let expected = expected_score_parts(uma);
        let total = parts.total();
        let calc = uma.calc_score().expect("calc_score");
        println!(
            "{label}: skill={} pt={} five={:?} total={} calc_score={}",
            parts.skill, parts.pt, parts.five_status, total, calc
        );
        assert_eq!(parts, expected, "{label}: 七个分量必须与公式逐位相等");
        assert_eq!(total, calc, "{label}: total 必须等于 calc_score()");
    }

    /// `score_parts` 求和逐位等于 `calc_score`（多组边界）
    #[test]
    fn test_score_parts_matches_calc_score() -> Result<()> {
        init()?;

        // 1. 全零
        check_score_parts_case("全零", &Uma::default());

        // 2. 五维触顶被 limit 截断
        let mut uma = Uma::default();
        uma.five_status = [5000, 4000, 3000, 2000, 1000];
        uma.five_status_limit = [100, 80, 60, 40, 20];
        uma.skill_score = 510;
        check_score_parts_case("五维触顶", &uma);

        // 3. 五维为负（按 0 查表）
        let mut uma = Uma::default();
        uma.five_status = [-10, -1, 0, 50, 100];
        uma.five_status_limit = [1200, 1200, 1200, 1200, 1200];
        check_score_parts_case("五维为负", &uma);

        // 4. skill_pt 与 total_hints 非零（total_pt 发生 floor）
        let mut uma = Uma::default();
        uma.skill_pt = 1;
        uma.total_hints = 1;
        uma.skill_score = 100;
        uma.five_status = [200, 180, 160, 140, 120];
        uma.five_status_limit = [1200, 1200, 1200, 1200, 1200];
        println!("floor 边界: total_pt()={}", uma.total_pt().expect("total_pt"));
        check_score_parts_case("floor 边界", &uma);

        // 5. 切者 / 小切：PT 项按 1.1 / 1.04 放大（只乘 PT 项）
        for (label, qiezhe, factor) in [("切者", true, 1.1_f32), ("小切", false, 1.04_f32)] {
            let mut uma = Uma::default();
            uma.skill_pt = 1000;
            uma.five_status = [200, 180, 160, 140, 120];
            uma.five_status_limit = [1200, 1200, 1200, 1200, 1200];
            let base_pt = uma.score_parts().expect("score_parts").pt;
            uma.flags.qiezhe = qiezhe;
            uma.flags.xiaoqie = !qiezhe;
            let boosted = uma.score_parts().expect("score_parts").pt;
            check_score_parts_case(label, &uma);
            assert_eq!(
                boosted,
                (base_pt as f32 * factor) as i32,
                "{label}: 终局评分的 PT 项应按折算系数放大"
            );
        }

        // 随机数仅用于本测试的 seed 演示位（保持文件自包含）
        let _ = StdRng::seed_from_u64(42);
        let _ = FriendState::new(None, 0)?; // 顺带冒烟：无友人卡路径
        Ok(())
    }
}
