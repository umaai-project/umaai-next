//! `crates/umaai` 集成测试共用工具
//!
//! 本 crate 不能依赖 `umaai-tests`（那会形成循环），故本地留一份与
//! `umaai-tests::Checker` 同语义的极简版：逐条打印 + 聚合，失败返回 `Err`。

#![allow(dead_code)]

use anyhow::{Result, bail};

/// 打印 + 聚合的检查器
pub struct Checks {
    title: &'static str,
    failures: Vec<String>,
}

impl Checks {
    /// 建立检查器
    pub fn new(title: &'static str) -> Self {
        println!("== {title} ==");
        Self { title, failures: vec![] }
    }

    /// 记录一条检查
    pub fn check(&mut self, ok: bool, label: &str) {
        if ok {
            println!("ok   {label}");
        } else {
            println!("FAIL {label}");
            self.failures.push(label.to_string());
        }
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
