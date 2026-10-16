//! 渲染原语：认数据不认剧本的呈现工具件
//!
//! 本模块只出**与剧本无关**的渲染件；剧本专属面板由剧本仓用这里的原语自己拼
//! （形态见 `docs/plan/miko_next_plan.md` §2「剧本仓形态」）。
//!
//! 边界纪律：**只有 [`table`] 接触表格库**，换库只改那一个文件；**只有 [`term`] 读环境
//! 与探测终端**，其余模块只拿判定结果。

pub mod explain;
pub mod panel;
pub mod style;
pub mod table;
pub mod term;
