//! 工具件瘦 bin：入口逻辑都在 `umaai_tools` 库里
//!
//! 放库里是为了让集成测试与外部脚本直接调 [`umaai_tools::cli::run_from_args`] 并注入
//! 输出目标；bin 只负责收参数、报告失败、给退出码。

use std::process::ExitCode;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    let mut out = std::io::stdout();
    match umaai_tools::cli::run_from_args(&argv, &mut out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("错误: {err:#}");
            ExitCode::FAILURE
        }
    }
}
