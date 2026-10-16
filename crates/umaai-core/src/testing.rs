//! 测试支撑（不承诺稳定 API）
//!
//! 供本 crate 与集成测试共用的夹具定位函数；跨 crate 测试见 `umaai-tests`。

use std::path::{Path, PathBuf};

use crate::{UmaError, UmaResult};

/// workspace 根目录（由本 crate 的 `CARGO_MANIFEST_DIR` 上溯两级）
pub fn workspace_root() -> UmaResult<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| UmaError::Internal("无法定位 workspace 根目录".to_string()))
}

/// 数据根目录（`<workspace>/gamedata`）
pub fn data_root() -> UmaResult<PathBuf> {
    Ok(workspace_root()?.join("gamedata"))
}
