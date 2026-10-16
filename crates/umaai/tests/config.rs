//! L2 模块集成：配置层（默认层 / 覆盖层 / 预设表 / 严格报错）
//!
//! 走真实数据根文件 + 覆盖层文本，检查「单一真源、缺字段报错、拼错键报错、
//! 预设按名取、剧本段确实不在本仓配置里」。

mod common;

use anyhow::Result;
use umaai::Config;
use umaai_core::testing::data_root;

use crate::common::Checks;

/// 载入仓库默认配置
fn load_base() -> Result<Config> {
    Config::load_from_data_root(data_root()?, None)
}

/// 默认层：真实文件字段齐全、预设可解析
#[test]
fn config_load_default() -> Result<()> {
    let mut c = Checks::new("配置默认层");
    let cfg = load_base()?;
    c.check(cfg.sim.uma == 101901, "sim.uma 取自默认层");
    c.check(cfg.sim.cards.len() == 6, "sim.cards 六张");
    c.check(cfg.sim.seed == 42, "sim.seed 取自默认层");
    c.check(cfg.search.search_n == 1024, "search.search_n 取自默认层");
    c.check(cfg.search.pt_favor_rate == 2.0, "pt_favor_rate 保持既有定档");
    c.check(!cfg.record.enabled, "record 默认关闭");
    let grades = cfg.race_grades()?;
    c.check(grades.len() == 72, "比赛等级表 72 项");
    c.check(grades.iter().all(|g| (0..=4).contains(g)), "等级取值均在 0-4");
    c.check(cfg.inherit().blue_count == [15, 3, 0, 0, 0], "继承因子可构造");
    println!(
        "配置摘要: sim={:?} search_n={} preset={}",
        cfg.sim, cfg.search.search_n, cfg.sim.race_grades
    );
    c.finish()
}

/// 数据文件里不得残留剧本专属段（剧本配置归剧本仓）
#[test]
fn config_has_no_scenario_segments() -> Result<()> {
    let mut c = Checks::new("配置无剧本段");
    let text = std::fs::read_to_string(data_root()?.join("default_config.toml"))?;
    for key in ["ramen", "onsen", "friend_complete_required", "collector"] {
        c.check(!text.contains(key), &format!("数据根默认配置不含 `{key}`"));
    }
    println!("默认配置字节数: {}", text.len());
    c.finish()
}

/// 覆盖层：逐字段覆盖、未提及字段沿用、段缺失整段沿用
#[test]
fn config_overlay_merge() -> Result<()> {
    let mut c = Checks::new("配置覆盖层");
    let base = Config::load_from_data_root(data_root()?, None)?;
    let cfg = Config::parse(
        &std::fs::read_to_string(data_root()?.join("default_config.toml"))?,
        Some("[sim]\numa = 102601\nseed = 7\n\n[search]\npt_favor_rate = 1.0\n"),
    )?;
    c.check(cfg.sim.uma == 102601, "覆盖 uma 生效");
    c.check(cfg.sim.seed == 7, "覆盖 seed 生效");
    c.check(cfg.sim.cards == base.sim.cards, "未提及字段沿用默认层");
    c.check(cfg.search.pt_favor_rate == 1.0, "覆盖搜索字段生效");
    c.check(cfg.search.search_n == base.search.search_n, "未提及搜索字段沿用");
    c.check(cfg.sim.race_grades == base.sim.race_grades, "预设选择沿用");
    c.finish()
}

/// 错误路径：缺预设、无覆盖文件、拼错键都必须报错
#[test]
fn config_error_paths() -> Result<()> {
    let mut c = Checks::new("配置错误路径");
    let base_text = std::fs::read_to_string(data_root()?.join("default_config.toml"))?;

    let bad_preset = base_text.replace("race_grades = \"standard\"", "race_grades = \"missing\"");
    let err = Config::parse(&bad_preset, None);
    let msg = err.as_ref().map_or_else(|e| format!("{e:#}"), |_| String::new());
    c.check(err.is_err() && msg.contains("missing"), "预设缺名报错且指明名字");

    let bad_key = Config::parse(&base_text, Some("[sim]\nume = 1\n"));
    let msg = bad_key.as_ref().map_or_else(|e| format!("{e:#}"), |_| String::new());
    c.check(bad_key.is_err() && msg.contains("ume"), "覆盖层拼错键报错");

    let missing_file = Config::load_from_data_root(data_root()?, Some(std::path::Path::new("no_such.toml")));
    c.check(missing_file.is_err(), "覆盖层文件缺失报错");

    let short_preset = Config::parse(
        &base_text.replace("race_grades = \"standard\"", "race_grades = \"short\""),
        Some("[presets.race_grades]\nshort = [0, 0, 0]\n"),
    );
    c.check(short_preset.is_err(), "预设长度不足报错");

    println!("错误样例: {msg}");
    c.finish()
}
