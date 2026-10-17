//! 层2 集成测试：真实 `gamedata/` 的载入、访问与校验
//!
//! 按项目约定：集成测试逐条 `print` 观测，末尾经 `Checker` 汇总，有失败才返回 `Err`。

use umaai_core::{Data, DataFile, DirSource, MemorySource, UmaError, UmaResult, data::validate::validate, testing};

/// 观测收集器：逐条打印 `OK` / `NG`，末尾有 NG 则返回 `Err`
struct Checker {
    failed: Vec<String>,
}

impl Checker {
    fn new() -> Self {
        Self { failed: Vec::new() }
    }

    fn check(&mut self, ok: bool, what: &str) {
        println!("  [{}] {what}", if ok { "OK" } else { "NG" });
        if !ok {
            self.failed.push(what.to_string());
        }
    }

    fn finish(self) -> UmaResult<()> {
        if self.failed.is_empty() {
            return Ok(());
        }
        Err(UmaError::Internal(format!(
            "{} 项观测未通过: {}",
            self.failed.len(),
            self.failed.join(" / ")
        )))
    }
}

/// 载入真实数据根：统计、访问器与掩码初始状态
#[test]
fn test_load_real_data() -> UmaResult<()> {
    let root = testing::data_root()?;
    let data = Data::load(&root)?;
    let stat = data.stat();
    println!("载入统计: {stat:?}");

    let mut c = Checker::new();
    c.check(stat.uma > 100, "马娘表条数 > 100");
    c.check(stat.card > 100, "支援卡表条数 > 100");
    c.check(stat.text > 0, "文本分组非空");
    c.check(stat.events > 10, "事件总条数 > 10");

    // 访问器：取首条马娘 / 支援卡
    let (uma_id, uma) = data.uma.iter().next().expect("马娘表非空");
    println!("样本马娘 {uma_id}: {} ({})", uma.name, uma.short_name());
    c.check(data.get_uma(uma.game_id).is_ok(), "按 ID 取马娘");
    c.check(data.get_uma(u32::MAX).is_err(), "未知马娘 ID 报错");

    let (card_id, card) = data.card.iter().next().expect("支援卡表非空");
    println!("样本支援卡 {card_id}: {} ({})", card.card_name, card.short_name());
    c.check(data.get_card(card.card_id).is_ok(), "按 ID 取支援卡");
    c.check(data.get_card(u32::MAX).is_err(), "未知支援卡 ID 报错");

    // 文本与常量快捷方法
    c.check(data.get_chara_name(card.chara_id) != "未知", "角色名可查");
    c.check(data.get_chara_name(u32::MAX) == "未知", "未知角色名回退");
    c.check(data.system_event("extra_train").is_ok(), "加练系统事件可查");
    c.check(data.event_prob("card_event").is_ok(), "事件概率可查");
    c.check(data.event_prob("__missing__").is_err(), "未知事件概率报错");

    // 自由比赛掩码依赖配置层，载入时保持 json 原值（空位段）
    let mask_zero = data
        .uma
        .values()
        .all(|u| u.free_races.iter().all(|f| f.mask.is_empty()));
    c.check(mask_zero, "载入时未计算自由比赛掩码");
    c.finish()
}

/// 全局初始化幂等：同源重复调用返回同一实例，异源报错
#[test]
fn test_init_idempotent() -> UmaResult<()> {
    let root = testing::data_root()?;
    let a = Data::init(&root)?;
    let b = Data::init(&root)?;
    let mut c = Checker::new();
    c.check(std::ptr::eq(a, b), "同数据根重复初始化返回同一实例");
    c.check(Data::try_get().is_some(), "初始化后 try_get 可取值");
    c.check(
        Data::init_from(&DirSource::new(&root)).is_ok(),
        "同来源经 init_from 命中既有实例",
    );
    c.check(
        Data::init(root.join("..").join("gamedata")).is_err(),
        "异数据根初始化报错",
    );
    c.check(
        Data::init_from(&MemorySource::new("mem:other")).is_err(),
        "异来源标识初始化报错（不静默改数据）",
    );
    c.finish()
}

/// 内存来源：真实数据的 JSON 文本与 JSON 值两条形态都能载入，结果与目录来源一致
#[test]
fn test_load_from_memory_source() -> UmaResult<()> {
    let root = testing::data_root()?;
    let payloads = [
        DataFile::Manifest,
        DataFile::Uma,
        DataFile::Card,
        DataFile::Text,
        DataFile::Events,
        DataFile::Constants,
    ];
    let mut text_src = MemorySource::new("mem:text");
    let mut json_src = MemorySource::new("mem:json");
    for file in payloads {
        let path = root.join(file.name());
        let text = std::fs::read_to_string(&path).map_err(|source| UmaError::Io { path: path.clone(), source })?;
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|source| UmaError::Parse { path: path.clone(), source })?;
        text_src = text_src.with_text(file, text);
        json_src = json_src.with_json(file, value);
    }

    // 目录来源只取统计与样本 ID，随后立即释放，避免三份数据集同时驻留
    let dir = Data::load(&root)?;
    let stat_dir = dir.stat();
    let uma_id = dir.uma.values().next().expect("马娘表非空").game_id;
    let card_id = dir.card.values().next().expect("支援卡表非空").card_id;
    drop(dir);

    let from_text = Data::from_source(&text_src)?;
    let from_json = Data::from_source(&json_src)?;
    println!("目录 {stat_dir:?}");
    println!("文本 {:?}", from_text.stat());
    println!("JSON 值 {:?}", from_json.stat());

    let mut c = Checker::new();
    c.check(from_text.stat() == stat_dir, "文本来源的载入统计与目录来源一致");
    c.check(from_json.stat() == stat_dir, "JSON 值来源的载入统计与目录来源一致");
    c.check(from_text.get_uma(uma_id).is_ok(), "文本来源可按 ID 取马娘");
    c.check(from_json.get_uma(uma_id).is_ok(), "JSON 值来源可按 ID 取马娘");
    c.check(from_json.get_card(card_id).is_ok(), "JSON 值来源可按 ID 取支援卡");
    c.check(
        from_text.system_event("extra_train").is_ok(),
        "文本来源可用系统事件快捷方法",
    );

    let empty = MemorySource::new("mem:empty");
    let missing = Data::from_source(&empty);
    println!("缺项来源 → {missing:?}");
    c.check(missing.is_err(), "来源缺项必须报错，不得静默当空表");
    c.finish()
}

/// 缺数据根：清单读取失败即报错
#[test]
fn test_load_missing_root() {
    let root = testing::workspace_root()
        .expect("workspace 根")
        .join("target/__not_exist__");
    println!("载入不存在的根 {root:?}");
    assert!(Data::load(&root).is_err(), "缺清单应报错");
}

/// 语义校验：破坏关键表后应被拒绝
#[test]
fn test_validate_rejects_broken_data() -> UmaResult<()> {
    let root = testing::data_root()?;
    let mut data = Data::load(&root)?;

    data.uma.clear();
    let e = validate(&data);
    println!("清空马娘表 → {e:?}");
    let mut c = Checker::new();
    c.check(e.is_err(), "马娘表为空应被拒绝");
    c.finish()
}

/// 清单版本门控：版本不一致即报错
#[test]
fn test_manifest_version_gate() -> UmaResult<()> {
    let dir = testing::workspace_root()?.join("target/test-tmp/umaai-core-manifest");
    std::fs::create_dir_all(&dir).map_err(|e| UmaError::Internal(format!("建临时目录失败: {e}")))?;
    let files = "\"umaDB.json\", \"cardDB.json\", \"text_data_dict.json\", \"events.json\", \"constants.json\"";
    let bad = format!(r#"{{"schema_version": 999, "files": [{files}]}}"#);
    std::fs::write(dir.join("manifest.json"), bad).map_err(|e| UmaError::Internal(format!("写临时清单失败: {e}")))?;

    let e = umaai_core::data::manifest::Manifest::load(&dir);
    println!("版本 999 → {e:?}");
    let _ = std::fs::remove_dir_all(&dir);

    let mut c = Checker::new();
    c.check(e.is_err(), "清单版本不一致应被拒绝");
    c.finish()
}
