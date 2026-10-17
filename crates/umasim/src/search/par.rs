//! 并行后端（crate 特性 `parallel`）
//!
//! 搜索只在两处需要并行：候选之间、同一候选的 rollout 之间；两处都收在本模块的一对函数里。
//! 特性 `parallel` 开启时走 rayon，关闭时逐字退化为串行 `map`——两条路径的收集顺序一致，
//! 结果逐位相同，故调用方无需为特性关掉写第二套代码。

use anyhow::Result;

/// 本构建是否编入并行后端（由 crate 特性 `parallel` 决定）
///
/// 关闭特性后 [`FlatSearch`](super::flat_search::FlatSearch) 的并行开关恒为 `false`，
/// 宿主可用本函数确认拿到的构建到底是并行还是串行。
pub fn supported() -> bool {
    cfg!(feature = "parallel")
}

/// 映射切片元素并**保序**收集结果
///
/// 顺序是契约：逐项累加的浮点求和顺序一变，所有历史基准失效。
#[cfg(feature = "parallel")]
pub(super) fn try_map_slice<T, R, F>(items: &[T], f: F) -> Result<Vec<R>>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> Result<R> + Send + Sync,
{
    use rayon::prelude::*;

    items.par_iter().map(f).collect()
}

/// 映射切片元素并**保序**收集结果（串行降级：特性 `parallel` 关闭）
#[cfg(not(feature = "parallel"))]
pub(super) fn try_map_slice<T, R, F>(items: &[T], f: F) -> Result<Vec<R>>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> Result<R> + Send + Sync,
{
    items.iter().map(f).collect()
}

/// 映射下标 `0..n` 并**保序**收集结果
#[cfg(feature = "parallel")]
pub(super) fn map_range<R, F>(n: usize, f: F) -> Vec<R>
where
    R: Send,
    F: Fn(usize) -> R + Send + Sync,
{
    use rayon::prelude::*;

    (0..n).into_par_iter().map(f).collect()
}

/// 映射下标 `0..n` 并**保序**收集结果（串行降级：特性 `parallel` 关闭）
#[cfg(not(feature = "parallel"))]
pub(super) fn map_range<R, F>(n: usize, f: F) -> Vec<R>
where
    R: Send,
    F: Fn(usize) -> R + Send + Sync,
{
    (0..n).map(f).collect()
}
