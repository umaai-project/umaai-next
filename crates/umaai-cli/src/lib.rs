//! # umaai-cli（控制台工具 + 样例实现）
//!
//! 终端依赖（着色 / 表格 / 宽度探测）**只允许**出现在本 crate。本 crate 有两个用途：
//!
//! - **工具件**（[`render`]）：与剧本无关的呈现原语——语义着色、领域词元、面板与表格
//!   渲染、终端宽度探测。剧本仓实现自己的回合面板时直接复用，不必重复踩 ANSI / CJK
//!   宽度与折行的坑。
//! - **样例实现**（[`frontend`]）：[`PrintFrontend`] 把引擎事件渲染成人读文本，是
//!   「一局到底」与「一帧一决策」两条链路的呈现参考。
//!
//! 入口层（[`cli`]，参数解析 + 选择器映射 + 运行装配）是**本仓 CLI 自己**的装配点；
//! 剧本仓自带 bin 做自己的装配，通常只取工具件与样例实现。剧本专属面板（地域 /
//! 诀窍 / 泡汤等）不进本 crate，由剧本仓用 [`render`] 的原语自己拼。
//!
//! **稳定性**：本 crate 的 pub API 不承诺稳定，外部（剧本仓）按 git `rev` 钉住使用。
//!
//! [`PrintFrontend`]: frontend::PrintFrontend
#![warn(missing_docs)]

pub mod cli;
pub mod frontend;
pub mod render;

#[cfg(test)]
mod testdata;
