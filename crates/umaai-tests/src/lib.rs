//! # umaai-tests（集成 / 端到端测试，`publish = false`）
//!
//! 测试支撑与集成测试的家：
//!
//! - [`Checker`]：打印 + 聚合，失败返回 `Err`（**不用 `assert`**，保持 `cargo test` 有失败信号）。
//! - 共享夹具：替代旧库 `#[path = "..."]` 跨目录引入的写法。
//! - baseline 加载 / 比对：`tests/baseline.rs` + 基线数据文件（自动判定）。
//!
//! 测试分层与约定见 `docs/plan/miko_next_plan.md` 的「测试策略」节。
#![warn(missing_docs)]

use anyhow::{Result, bail};

/// 集成测试检查器：逐条打印结果并聚合失败项
///
/// 用法：`let mut c = Checker::new("标题"); c.check(cond, "说明"); c.finish()?;`
/// 失败不打断流程，`finish()` 汇总为 `Err`，保证 `cargo test` 有失败信号。
pub struct Checker {
    title: String,
    failures: Vec<String>,
}

impl Checker {
    /// 建立检查器
    pub fn new(title: &str) -> Self {
        println!("== {title} ==");
        Self {
            title: title.to_string(),
            failures: vec![],
        }
    }

    /// 记录一条检查：通过打 `ok`，失败打 `FAIL` 并记录说明
    pub fn check(&mut self, ok: bool, label: &str) -> &mut Self {
        if ok {
            println!("ok   {label}");
        } else {
            println!("FAIL {label}");
            self.failures.push(label.to_string());
        }
        self
    }

    /// 结束：全部通过返回 `Ok`，否则聚合失败项返回 `Err`
    pub fn finish(self) -> Result<()> {
        if self.failures.is_empty() {
            println!("== {} 全部通过 ==", self.title);
            Ok(())
        } else {
            bail!(
                "{} 失败 {} 项: {}",
                self.title,
                self.failures.len(),
                self.failures.join("; ")
            )
        }
    }
}
