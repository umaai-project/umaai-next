//! 本仓 CLI 的 thin bin：入口逻辑都在 `umaai_cli` 库里
//!
//! 放在库里是为了让集成测试与下游直接调 [`umaai_cli::cli::run_from_args`] 并注入输出目标；
//! bin 只负责收参数、报告失败、给退出码。
//!
//! 更新器（数据 / 程序更新）是入口层职责，归 P6′；bin 名暂用 `umaai-cli`，发布产物的
//! 最终名字在打包阶段定。

use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    let mut out = std::io::stdout();
    match umaai_cli::cli::run_from_args(&argv, &mut out) {
        Ok(()) => {
            let _ = out.flush();
            ExitCode::SUCCESS
        }
        Err(err) => {
            // 引擎在下发 `on_error` 之后仍把错误返回给调用方，此处统一打印一次因果链
            eprintln!("错误: {err:#}");
            ExitCode::FAILURE
        }
    }
}
