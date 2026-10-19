//! 层2 集成测试：更新库端到端（假传输 + 真实文件系统，不联网）
//!
//! 按项目约定：集成测试逐条 `print` 观测，末尾经本地 `Checker` 汇总，有失败才返回 `Err`。
//!
//! 覆盖：首次全量更新、只拉变化文件、下载损坏不动目标目录、频道互不阻断、
//! schema 门控、替换失败回滚、进度回调、子目录路径。

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use umaai_update::{
    Channel, ChannelUpdateOutcome, Options, Progress, Source, TestTransport, Transport, check, manifest, update,
};

/// 忽略进度的用例用（函数项天然对任意生命周期成立，闭包绑定到变量会被要求唯一生命周期）
fn ignore_progress(_p: Progress<'_>) {}

/// 观测收集器：逐条打印 `OK` / `NG`，末尾有 NG 则返回 `Err`
struct Checker {
    failed: Vec<String>,
}

impl Checker {
    fn new(title: &str) -> Self {
        println!("== {title} ==");
        Self { failed: Vec::new() }
    }

    fn check(&mut self, ok: bool, what: &str) {
        println!("  [{}] {what}", if ok { "OK" } else { "NG" });
        if !ok {
            self.failed.push(what.to_string());
        }
    }

    fn finish(self) -> Result<(), Box<dyn Error>> {
        if self.failed.is_empty() {
            println!("== 全部通过 ==");
            return Ok(());
        }
        Err(format!("{} 项观测未通过: {}", self.failed.len(), self.failed.join(" / ")).into())
    }
}

/// 建一个唯一的临时目录（测试之间不共享）
fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("umaai-update-it-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// 写入发布目录里的一个文件
fn publish(dir: &Path, rel: &str, bytes: &[u8]) {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// 把发布目录挂成假源：清单按 `build` 生成，文件按 `Source::file_url` 挂载
fn serve(source: &Source, channel: &str, publish_dir: &Path) -> Result<TestTransport, Box<dyn Error>> {
    let built = manifest::build(publish_dir, manifest::BuildOptions::new("v1").with_schema_version(1))?;
    let mut transport = TestTransport::new().serve_str(source.manifest_url(channel), &built.to_json()?);
    for entry in &built.files {
        let bytes = fs::read(publish_dir.join(&entry.path))?;
        transport = transport.serve(source.file_url(channel, &entry.path), bytes);
    }
    Ok(transport)
}

const BASE: &str = "https://cdn.example/uma";

/// 首次更新：空目录全量安装、临时目录清理、复跑即最新
#[test]
fn test_first_full_update() -> Result<(), Box<dyn Error>> {
    let root = tmp("first-full");
    let publish_dir = root.join("publish");
    let data_dir = root.join("data");
    fs::create_dir_all(&publish_dir)?;
    fs::create_dir_all(&data_dir)?;
    publish(&publish_dir, "a.txt", b"aaa");
    publish(&publish_dir, "sub/b.txt", b"bbbb");

    let source = Source::new(BASE)?;
    let channel = Channel::data("ai_data", "AI 数据", &data_dir)?;
    let transport = serve(&source, "ai_data", &publish_dir)?;

    let mut checker = Checker::new("首次全量更新");
    let report = check(&source, std::slice::from_ref(&channel), &transport);
    let outcome = report.channels[0].result.as_ref().expect("检查应成功");
    checker.check(outcome.pending.len() == 2, "空目录应列出 2 个待更新文件");
    checker.check(report.pending() == 1, "报告应计 1 个待更新频道");

    let options = Options::new(&root);
    let report = update(
        &source,
        std::slice::from_ref(&channel),
        &transport,
        &options,
        &mut ignore_progress,
    );
    checker.check(report.all_ok(), "更新应全部成功");
    checker.check(report.updated() == 1, "应有 1 个频道发生替换");
    checker.check(
        matches!(report.channels[0].result, Ok(ChannelUpdateOutcome::Updated { ref files, .. }) if files.len() == 2),
        "结果应点名 2 个被替换文件",
    );
    checker.check(fs::read(data_dir.join("a.txt"))? == b"aaa", "根目录文件内容正确");
    checker.check(fs::read(data_dir.join("sub/b.txt"))? == b"bbbb", "子目录文件内容正确");
    checker.check(!options.temp_root().exists(), "成功后临时目录应被删除");

    let report = check(&source, std::slice::from_ref(&channel), &transport);
    checker.check(report.up_to_date(), "复跑应判定为最新");
    let _ = fs::remove_dir_all(&root);
    checker.finish()
}

/// 只拉变化文件：本地已一致的文件不再下载
#[test]
fn test_only_changed_file_is_fetched() -> Result<(), Box<dyn Error>> {
    let root = tmp("only-changed");
    let publish_dir = root.join("publish");
    let data_dir = root.join("data");
    fs::create_dir_all(&publish_dir)?;
    fs::create_dir_all(&data_dir)?;
    publish(&publish_dir, "a.txt", b"aaa");
    publish(&publish_dir, "b.txt", b"bbb");
    // 本地 a 已一致、b 被改坏
    fs::write(data_dir.join("a.txt"), b"aaa")?;
    fs::write(data_dir.join("b.txt"), b"wrong")?;

    let source = Source::new(BASE)?;
    let channel = Channel::data("ai_data", "AI 数据", &data_dir)?;
    let transport = serve(&source, "ai_data", &publish_dir)?;

    let mut checker = Checker::new("只拉变化文件");
    let report = check(&source, std::slice::from_ref(&channel), &transport);
    let outcome = report.channels[0].result.as_ref().expect("检查应成功");
    checker.check(
        outcome.pending.len() == 1 && outcome.pending[0].path == "b.txt",
        "只应列出被改坏的 b.txt",
    );
    checker.check(outcome.pending_bytes == 3, "待更新字节数应为 3");

    let options = Options::new(&root);
    let requests_before = transport.requests().len();
    let report = update(
        &source,
        std::slice::from_ref(&channel),
        &transport,
        &options,
        &mut ignore_progress,
    );
    checker.check(report.all_ok(), "更新应成功");
    let fetched = transport.requests();
    let after: Vec<&String> = fetched.iter().skip(requests_before).collect();
    checker.check(after.len() == 2, "只应再请求 2 次（清单 + 变化文件）");
    checker.check(
        after.iter().any(|u| u.ends_with("/b.txt")) && !after.iter().any(|u| u.ends_with("/a.txt")),
        "请求里应有 b.txt、没有 a.txt",
    );
    checker.check(fs::read(data_dir.join("b.txt"))? == b"bbb", "b.txt 已换成远端内容");
    let _ = fs::remove_dir_all(&root);
    checker.finish()
}

/// 下载内容损坏：不动目标目录（先全下后装），临时目录保留待排查
#[test]
fn test_corrupted_download_leaves_target_untouched() -> Result<(), Box<dyn Error>> {
    let root = tmp("corrupted");
    let publish_dir = root.join("publish");
    let data_dir = root.join("data");
    fs::create_dir_all(&publish_dir)?;
    fs::create_dir_all(&data_dir)?;
    publish(&publish_dir, "a.txt", b"aaa");
    publish(&publish_dir, "b.txt", b"bbb");
    fs::write(data_dir.join("a.txt"), b"old-a")?;

    let source = Source::new(BASE)?;
    let channel = Channel::data("ai_data", "AI 数据", &data_dir)?;
    let built = manifest::build(&publish_dir, manifest::BuildOptions::new("v1"))?;
    let mut transport = TestTransport::new().serve_str(source.manifest_url("ai_data"), &built.to_json()?);
    for entry in &built.files {
        let bytes = fs::read(publish_dir.join(&entry.path))?;
        // b.txt 的内容与清单登记的哈希不符
        let served = if entry.path == "b.txt" {
            b"tampered".to_vec()
        } else {
            bytes
        };
        transport = transport.serve(source.file_url("ai_data", &entry.path), served);
    }

    let options = Options::new(&root);
    let report = update(
        &source,
        std::slice::from_ref(&channel),
        &transport,
        &options,
        &mut ignore_progress,
    );
    let mut checker = Checker::new("下载损坏");
    checker.check(!report.all_ok(), "更新应失败");
    checker.check(
        format!("{}", report.channels[0].result.as_ref().unwrap_err()).contains("校验"),
        "错误应点名校验不一致",
    );
    checker.check(fs::read(data_dir.join("a.txt"))? == b"old-a", "目标文件不应被改动");
    checker.check(!data_dir.join("b.txt").exists(), "损坏文件不应落到目标目录");
    checker.check(options.temp_root().exists(), "失败时临时目录应保留");
    let _ = fs::remove_dir_all(&root);
    checker.finish()
}

/// 频道互不阻断：一个频道清单取不到，另一个照常更新
#[test]
fn test_channels_are_independent() -> Result<(), Box<dyn Error>> {
    let root = tmp("independent");
    let publish_dir = root.join("publish");
    let data_a = root.join("data-a");
    let data_b = root.join("data-b");
    fs::create_dir_all(&publish_dir)?;
    fs::create_dir_all(&data_a)?;
    fs::create_dir_all(&data_b)?;
    publish(&publish_dir, "a.txt", b"aaa");

    let source = Source::new(BASE)?;
    let channels = [
        Channel::data("ai_data", "AI 数据", &data_a)?,
        Channel::data("ura_data", "小黑板数据", &data_b)?,
    ];
    // 只挂 ai_data 的清单：ura_data 会报「假源没有该 URL」
    let mut transport = serve(&source, "ai_data", &publish_dir)?;
    transport = transport.fail(source.manifest_url("ura_data"), "404");

    let options = Options::new(&root);
    let report = update(&source, &channels, &transport, &options, &mut |_p| {});
    let mut checker = Checker::new("频道互不阻断");
    checker.check(report.failed() == 1, "应只有 1 个频道失败");
    checker.check(report.updated() == 1, "另一个频道应已更新");
    checker.check(data_a.join("a.txt").exists(), "成功频道已落文件");
    checker.check(!data_b.join("a.txt").exists(), "失败频道未写任何东西");
    let _ = fs::remove_dir_all(&root);
    checker.finish()
}

/// schema 门控：数据与程序版本不一致时拒绝更新（一个字节都不动）
#[test]
fn test_schema_gate_blocks_install() -> Result<(), Box<dyn Error>> {
    let root = tmp("schema-gate");
    let publish_dir = root.join("publish");
    let data_dir = root.join("data");
    fs::create_dir_all(&publish_dir)?;
    fs::create_dir_all(&data_dir)?;
    publish(&publish_dir, "a.txt", b"aaa");

    let source = Source::new(BASE)?;
    let channel = Channel::data("ai_data", "AI 数据", &data_dir)?.expect_schema(7);
    let transport = serve(&source, "ai_data", &publish_dir)?; // 清单声明 schema_version = 1

    let options = Options::new(&root);
    let report = update(
        &source,
        std::slice::from_ref(&channel),
        &transport,
        &options,
        &mut ignore_progress,
    );
    let mut checker = Checker::new("schema 门控");
    checker.check(!report.all_ok(), "期望 7 / 远端 1 应失败");
    checker.check(!data_dir.join("a.txt").exists(), "门控失败不应安装文件");
    checker.check(!options.temp_root().exists(), "门控在下载前拦截，不应留临时目录");
    let _ = fs::remove_dir_all(&root);
    checker.finish()
}

/// 替换失败回滚：第二个文件装不进去时，先前替换的文件要被还原
#[test]
fn test_install_failure_rolls_back() -> Result<(), Box<dyn Error>> {
    let root = tmp("rollback");
    let publish_dir = root.join("publish");
    let data_dir = root.join("data");
    fs::create_dir_all(&publish_dir)?;
    fs::create_dir_all(&data_dir)?;
    publish(&publish_dir, "a.txt", b"aaa");
    publish(&publish_dir, "b.txt", b"bbb");
    // 目标目录里 b.txt 是个目录：替换必然失败（清单按字典序，a 先装）
    fs::create_dir_all(data_dir.join("b.txt"))?;

    let source = Source::new(BASE)?;
    let channel = Channel::data("ai_data", "AI 数据", &data_dir)?;
    let transport = serve(&source, "ai_data", &publish_dir)?;

    let options = Options::new(&root);
    let report = update(
        &source,
        std::slice::from_ref(&channel),
        &transport,
        &options,
        &mut ignore_progress,
    );
    let mut checker = Checker::new("替换失败回滚");
    checker.check(!report.all_ok(), "替换失败应报错");
    checker.check(!data_dir.join("a.txt").exists(), "先前替换的 a.txt 应被回滚删除");
    checker.check(data_dir.join("b.txt").is_dir(), "造成失败的目录应原样保留");
    let _ = fs::remove_dir_all(&root);
    checker.finish()
}

/// 进度回调：每个文件都有 (0/total) 起始与 (total/total) 结束
#[test]
fn test_progress_events() -> Result<(), Box<dyn Error>> {
    let root = tmp("progress");
    let publish_dir = root.join("publish");
    let data_dir = root.join("data");
    fs::create_dir_all(&publish_dir)?;
    fs::create_dir_all(&data_dir)?;
    publish(&publish_dir, "a.txt", b"aaa");
    publish(&publish_dir, "b.txt", b"bbbbbb");

    let source = Source::new(BASE)?;
    let channel = Channel::data("ai_data", "AI 数据", &data_dir)?;
    let transport = serve(&source, "ai_data", &publish_dir)?;

    let mut events: Vec<(String, u64, u64)> = Vec::new();
    let report = update(
        &source,
        std::slice::from_ref(&channel),
        &transport,
        &Options::new(&root),
        &mut |p: Progress<'_>| {
            events.push((p.path.to_string(), p.done, p.total));
        },
    );

    let mut checker = Checker::new("进度回调");
    checker.check(report.all_ok(), "更新应成功");
    for path in ["a.txt", "b.txt"] {
        let of_file: Vec<&(String, u64, u64)> = events.iter().filter(|e| e.0 == path).collect();
        checker.check(
            of_file.first().is_some_and(|e| e.1 == 0),
            &format!("{path} 应有起始进度 0"),
        );
        checker.check(
            of_file.last().is_some_and(|e| e.1 == e.2 && e.2 > 0),
            &format!("{path} 应有结束进度 total"),
        );
    }
    let _ = fs::remove_dir_all(&root);
    checker.finish()
}

/// 传输抽象可直接自实现（宿主自备传输的形态）
#[test]
fn test_custom_transport() -> Result<(), Box<dyn Error>> {
    struct Oneshot {
        url: String,
        body: Vec<u8>,
    }

    impl Transport for Oneshot {
        fn download(
            &self,
            url: &str,
            _headers: &[(String, String)],
            sink: &mut dyn std::io::Write,
        ) -> umaai_update::UpdateResult<u64> {
            if url != self.url {
                return Err(umaai_update::UpdateError::Http {
                    url: url.to_string(),
                    detail: "不认识的地址".to_string(),
                });
            }
            sink.write_all(&self.body)
                .map_err(|source| umaai_update::UpdateError::Io {
                    path: PathBuf::from("<自定义传输>"),
                    source,
                })?;
            Ok(self.body.len() as u64)
        }
    }

    let source = Source::new(BASE)?;
    let channel = Channel::data("ai_data", "AI 数据", tmp("custom-transport-data"))?;
    let json = r#"{"protocol":1,"version":"v1","files":[{"path":"a.txt","size":3,"sha256":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"}]}"#;
    let transport = Oneshot {
        url: source.manifest_url("ai_data"),
        body: json.as_bytes().to_vec(),
    };
    let report = check(&source, std::slice::from_ref(&channel), &transport);
    let mut checker = Checker::new("宿主自备传输");
    checker.check(report.channels[0].result.is_ok(), "自定义传输应被接受");
    checker.check(report.pending() == 1, "应判定为待更新");
    let _ = fs::remove_dir_all(channel.dir());
    checker.finish()
}
