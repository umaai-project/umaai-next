//! 终端样例外壳：装配一局并打印
//!
//! 真实剧本仓的入口按宿主需要自定（终端 / Web / Android 各自装配自己的 bin / lib）；本 bin
//! 只是主线自测外壳，复用 `umaai-cli` 的呈现件（样例面，pub 面不承诺稳定）。
//!
//! 用法：`cargo run --release -- <数据根>`（缺省 `gamedata`）

use std::process::ExitCode;

use anyhow::{Context, Result};
use umaai::Config;
use umaai::runner::Runner;
use umaai_cli::frontend::{PrintFrontend, PrintOptions, Verbosity};
use umaai_cli::render::term;
use umaai_core::Data;
use umaai_scenario::build_game;
use umasim::trainer::RandomTrainer;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("错误: {err:#}");
            ExitCode::FAILURE
        }
    }
}

/// 装配一局并跑到底（配置与数据根都由宿主注入）
fn run() -> Result<()> {
    let data_root = std::env::args().nth(1).unwrap_or_else(|| "gamedata".to_string());
    // 配置两份同名不同位：默认层在数据根、覆盖层在当前目录（与引擎约定一致）
    let cfg = Config::load_with_overlay_file(&data_root, ".").context("载入配置失败")?;
    Data::init(&data_root)?;

    let consts = &Data::get()?.consts;
    let mut out = std::io::stdout();
    let opts = PrintOptions {
        verbosity: Verbosity::Normal,
        width: term::resolve_width(None),
        color: false,
    };
    let mut fe = PrintFrontend::new(&mut out, opts, consts);
    let game = build_game(&cfg)?;
    // 规则主种子由 Runner 注入（`cfg.sim.seed`）：引擎两轴与剧本策略轴都按它派生
    let mut runner = Runner::new(&cfg, &mut fe)?;
    runner.run_game(game, &RandomTrainer, None)?;
    Ok(())
}
