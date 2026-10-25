//! 测试支撑（不承诺稳定 API）
//!
//! workspace 化后测试进程的 cwd 在各 crate 目录，数据根按「crate 清单目录上溯两级＝仓根」
//! 定位。注意引擎 `umaai-core::testing` 的 `env!` 固化的是**引擎仓**路径，跨仓不可用；
//! 本模块在仓内成员间通用（成员都在 `crates/` 下一层，任一成员上溯两级都得到同一仓根）。

use std::path::{Path, PathBuf};

/// 仓根目录（任一成员 crate 的 `CARGO_MANIFEST_DIR` 上溯两级）
pub fn repo_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .expect("crate 目录上溯两级即仓根（成员都在 crates/ 下一层）")
}

/// 统一数据目录（`<仓根>/data`：引擎静态数据 + 剧本资产 + 双配置默认层）
pub fn data_dir() -> PathBuf {
    repo_root().join("data")
}
