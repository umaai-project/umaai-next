//! 集成测试冒烟：确认测试 crate 与 workspace 装配可用。
//!
//! 按项目约定，集成测试用 `println!` 输出供检查，不用 `assert`。

#[test]
fn smoke() {
    println!("umaai-tests: smoke ok");
}
