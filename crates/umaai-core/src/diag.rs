//! 可裁剪的诊断日志宏与进程级开关
//!
//! 与游戏、与算法无关，供各层共用：规则层与搜索层的诊断输出走 [`diag!`](macro@crate::diag)，业务日志
//! 仍直接走 `log::{info, warn, error}`。
//!
//! # 两层开关
//!
//! - **编译期**（本 crate 的 feature `diag`，默认开启）：关闭时宏展开为空，连 `format_args!`
//!   都不执行；下游整块消除只需对 `umaai-core` 用 `default-features = false`。
//! - **运行时**（[`enabled`] / [`set_enabled`] / [`DiagGuard`]）：进程级 `AtomicBool`，供
//!   rollout 期间临时静默；`Relaxed` 序是尽力而为语义，最坏只多/少几条边界日志。
//!
//! feature 开启时宏体引用 `::log`，故**调用方 crate 必须依赖 `log`**（未装 logger 静默丢弃）：
//! `let _g = umaai_core::diag::DiagGuard::suppress(); umaai_core::diag!("回合 {}", 12);`

use std::sync::atomic::{AtomicBool, Ordering};

/// 诊断输出运行时开关（进程级，默认开启）
static DIAG_ENABLED: AtomicBool = AtomicBool::new(true);

/// 诊断输出是否在**编译期**被启用（本 crate 的 feature `diag`）
///
/// 常量求值：关闭该 feature 的构建里取值恒为 `false`，编译器整块消除 [`diag!`](macro@crate::diag) 展开体，
/// 因此关闭态是真正的零运行时开销。
#[inline]
#[must_use]
pub const fn compiled_in() -> bool {
    cfg!(feature = "diag")
}

/// 诊断输出当前是否开启（运行时开关）
#[inline]
#[must_use]
pub fn enabled() -> bool {
    DIAG_ENABLED.load(Ordering::Relaxed)
}

/// 设置诊断输出开关（进程级，跨线程可见）
///
/// 直接调用请优先考虑 [`DiagGuard`]：guard 以 RAII 方式恢复进入前的开关值，
/// 天然免疫提前返回（`Err` / `?`）导致的开关泄漏。
pub fn set_enabled(on: bool) {
    DIAG_ENABLED.store(on, Ordering::Relaxed);
}

/// 诊断输出抑制 guard（RAII）
///
/// 创建时关闭诊断输出，[`Drop`] 时恢复**进入前的值**（栈式嵌套安全：
/// 内层 guard 提前 drop 不会误开外层的抑制状态）。开关是进程级的，因此多局并行时
/// 任一局的抑制会顺带静默其他局的诊断输出。
#[derive(Debug)]
pub struct DiagGuard {
    /// 进入 guard 前的开关值（Drop 时恢复）
    prev: bool,
}

impl DiagGuard {
    /// 关闭诊断输出并返回恢复 guard
    #[must_use = "guard 被 drop 前诊断输出一直关闭，请绑定到变量以覆盖整个作用域"]
    pub fn suppress() -> Self {
        let prev = enabled();
        set_enabled(false);
        Self { prev }
    }
}

impl Drop for DiagGuard {
    fn drop(&mut self) {
        set_enabled(self.prev);
    }
}

/// 可裁剪的诊断日志宏（`feature = "diag"` 开启时的形态）
///
/// 运行时开关关闭时连 `format_args!` 都不执行；feature 关闭时，本宏整体展开为空。
#[cfg(feature = "diag")]
#[macro_export]
macro_rules! diag {
    ($($arg:tt)*) => {
        if $crate::diag::enabled() {
            ::log::info!(target: "diagnostic", $($arg)*);
        }
    };
}

/// 可裁剪的诊断日志宏（`feature = "diag"` 关闭时的形态：零代码）
#[cfg(not(feature = "diag"))]
#[macro_export]
macro_rules! diag {
    ($($arg:tt)*) => {};
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::{DiagGuard, compiled_in, enabled, set_enabled};

    /// 全局开关测试互斥锁：开关是进程级状态，相关测试必须串行执行
    static SWITCH_LOCK: Mutex<()> = Mutex::new(());

    /// 层1：`compiled_in()` 与本 crate 的 feature 一致
    #[test]
    fn test_compiled_in_follows_feature() {
        assert_eq!(compiled_in(), cfg!(feature = "diag"));
    }

    /// 层1：guard 抑制后 `enabled()` 为 false，drop 后恢复进入前的值
    #[test]
    fn test_guard_suppress_and_restore() {
        // unwrap：测试代码，锁中毒只可能来自其他测试 panic，无恢复意义
        let _serial = SWITCH_LOCK.lock().unwrap();
        assert!(enabled(), "前置条件：诊断开关默认开启");
        {
            let _guard = DiagGuard::suppress();
            assert!(!enabled(), "guard 作用域内诊断输出必须被抑制");
        }
        assert!(enabled(), "guard drop 后必须恢复开启");
    }

    /// 层1：guard 恢复的是**进入前的值**而非硬编码 true（栈式嵌套安全）
    #[test]
    fn test_guard_restores_prev_value() {
        // unwrap：测试代码，锁中毒只可能来自其他测试 panic，无恢复意义
        let _serial = SWITCH_LOCK.lock().unwrap();
        set_enabled(false);
        {
            let _guard = DiagGuard::suppress();
            assert!(!enabled(), "内层 guard 作用域内仍为关闭");
        }
        assert!(!enabled(), "guard 应恢复进入前的 false，而非硬编码 true");
        set_enabled(true);
        assert!(enabled(), "清理：恢复默认开启");
    }

    /// 层1：嵌套 guard 内层 drop 后外层抑制继续保持
    #[test]
    fn test_nested_guards() {
        // unwrap：测试代码，锁中毒只可能来自其他测试 panic，无恢复意义
        let _serial = SWITCH_LOCK.lock().unwrap();
        {
            let _outer = DiagGuard::suppress();
            {
                let _inner = DiagGuard::suppress();
                assert!(!enabled());
            }
            assert!(!enabled(), "内层 drop 后外层的抑制必须继续保持");
        }
        assert!(enabled(), "全部 guard drop 后恢复开启");
    }
}
