//! L2 模块集成：帧通道（路径常量 / 配置错误出口 / 启动兜底 / 真 watcher）
//!
//! 真 watcher 的写入链路依赖文件系统事件派发及其时序，默认门禁不跑（`#[ignore]`）；
//! 手动执行：`cargo test -p umaai --release --test channel -- --ignored --nocapture`。
//! 其余用例只依赖「启动兜底 + 超时」两条确定性路径，取帧一律带显式超时。

#![cfg(feature = "file-channel")]

mod common;

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use umaai::protocol::channel::{FRAME_FINAL_SCORE, FRAME_THIS_TURN, PORTABLE_DIR};
use umaai::{FileChannel, FrameKind, plugin_dir};

use crate::common::Checks;

/// 回合帧原文（内容对通道不透明，只需可区分）
const TURN_FRAME: &str = r#"{"baseGame":{"scenarioId":14,"turn":3}}"#;
/// 变化后的回合帧原文
const TURN_FRAME2: &str = r#"{"baseGame":{"scenarioId":14,"turn":4}}"#;
/// 终局帧原文
const FINAL_FRAME: &str = r#"{"scenarioId":14,"single_mode_chara_id":6264}"#;

/// 临时根目录（名带 pid 与用例名，避免并行用例互相踩文件）
fn temp_root(case: &str) -> PathBuf {
    std::env::temp_dir().join(format!("umaai_channel_it_{case}_{}", std::process::id()))
}

/// 路径常量 + 配置错误出口：目录缺失即报错，目录在但无写入时超时返 `None`
#[test]
fn channel_path_and_config_error() -> Result<()> {
    let mut c = Checks::new("帧通道：路径常量与配置错误");
    let root = temp_root("cfg");
    let _ = fs::remove_dir_all(&root);

    let dir = plugin_dir(&root);
    c.check(dir.starts_with(&root), "插件目录落在注入根目录下");
    c.check(dir.ends_with("SendGameStatusPlugin"), "目录名取自代码常量");
    c.check(dir.to_string_lossy().contains(PORTABLE_DIR), "目录经 .portable");

    // 目录缺失 = 配置问题 → 立即报错，不静默降级成「没有数据」
    c.check(FileChannel::open(&root).is_err(), "目录缺失时 open 报错");

    // 目录存在但育成未开始 → open 成功，限时取帧返 None（不阻塞）
    fs::create_dir_all(&dir)?;
    let mut ch = FileChannel::open(&root)?;
    c.check(
        ch.next_frame(Duration::from_millis(300))?.is_none(),
        "无写入时超时返 None（不阻塞）",
    );
    c.check(ch.drain_notes().is_empty(), "空目录不应有自愈痕迹");

    let _ = fs::remove_dir_all(&root);
    c.finish()
}

/// 启动兜底：目录内预置的两份白名单文件各推一份（残留帧是否采纳由记录层局号闸判定）
#[test]
fn channel_startup_fallback() -> Result<()> {
    let mut c = Checks::new("帧通道：启动兜底");
    let root = temp_root("residual");
    let dir = plugin_dir(&root);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(FRAME_THIS_TURN), TURN_FRAME)?;
    fs::write(dir.join(FRAME_FINAL_SCORE), FINAL_FRAME)?;

    let mut ch = FileChannel::open(&root)?;
    let first = ch.next_frame(Duration::from_secs(2))?;
    let second = ch.next_frame(Duration::from_secs(2))?;
    c.check(
        first.as_ref().map(|f| f.kind) == Some(FrameKind::ThisTurn),
        "先推回合帧（白名单顺序）",
    );
    c.check(
        first.as_ref().map(|f| f.contents.as_str()) == Some(TURN_FRAME),
        "回合帧原文一致",
    );
    c.check(
        second.as_ref().map(|f| f.kind) == Some(FrameKind::FinalScore),
        "再推终局帧",
    );

    let _ = fs::remove_dir_all(&root);
    c.finish()
}

/// 真 watcher 写入链路：写入 → 取帧；同内容覆盖写被去重；两条信道都可达
#[test]
#[ignore = "依赖文件系统事件派发，默认门禁不跑（手动见文件头注释）"]
fn channel_watches_real_writes() -> Result<()> {
    let mut c = Checks::new("帧通道：真 watcher 写入链路");
    let root = temp_root("watch");
    let dir = plugin_dir(&root);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&dir)?;

    let mut ch = FileChannel::open(&root)?;

    // 写入 → 限时取帧（超时即 None → 检查失败，不阻塞）
    fs::write(dir.join(FRAME_THIS_TURN), TURN_FRAME)?;
    let first = ch.next_frame(Duration::from_secs(5))?;
    c.check(
        first.as_ref().map(|f| f.contents.as_str()) == Some(TURN_FRAME),
        "写入后 5s 内收到回合帧",
    );

    // 同内容覆盖写 → 去重（250ms 内不应有新帧）
    fs::write(dir.join(FRAME_THIS_TURN), TURN_FRAME)?;
    c.check(
        ch.next_frame(Duration::from_millis(300))?.is_none(),
        "同内容覆盖写被去重",
    );

    // 内容变化 → 照常入队
    fs::write(dir.join(FRAME_THIS_TURN), TURN_FRAME2)?;
    let second = ch.next_frame(Duration::from_secs(5))?;
    c.check(
        second.as_ref().map(|f| f.contents.as_str()) == Some(TURN_FRAME2),
        "内容变化仍入队",
    );

    // 终局帧信道同样可达
    fs::write(dir.join(FRAME_FINAL_SCORE), FINAL_FRAME)?;
    let third = ch.next_frame(Duration::from_secs(5))?;
    c.check(
        third.as_ref().map(|f| f.kind) == Some(FrameKind::FinalScore),
        "终局帧信道可达",
    );

    let _ = fs::remove_dir_all(&root);
    c.finish()
}
