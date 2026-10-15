//! # umaai-cli（print 前端）
//!
//! 终端依赖（着色 / 表格 / 进度 / 交互 / 日志）**只允许**出现在本 crate。
//! 实现 `Frontend`，把事件渲染成人读文本 / 表格。
//!
//! 入口层功能也落在这里，例如「更新数据」（数据/程序更新的检查与执行，见方案 v2 的
//! 「更新与分发」节）——但**更新逻辑不得进入引擎 lib**。
//!
//! 说明：bin 名暂用 `umaai-cli`；发布时 `umaai.exe` 的最终产物名在打包阶段另定。

fn main() -> anyhow::Result<()> {
    println!("umaai-cli skeleton");
    Ok(())
}
