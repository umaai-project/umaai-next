//! 频道更新清单（远端 `<源基址>/<频道 id>/update.json`）
//!
//! 清单是**唯一的版本真源**：客户端不看本地版本文件，只按「清单登记的 sha256 与本地文件比对」
//! 判定是否需要更新。协议纪律四条：
//!
//! 1. `protocol` 只认自己支持的值（不认识即拒绝并提示升级程序）；
//! 2. 未知字段**宽容忽略**（协议可前向演进），缺必需字段即报错；
//! 3. `files[].path` 必须是安全的相对路径（拒绝绝对路径、`..`、反斜杠与盘符）；
//! 4. 清单**不携带安装路径**——装到哪由本地 [`Channel`](crate::Channel) 决定。

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    error::{UpdateError, UpdateResult},
    hash,
};

/// 清单协议版本（客户端只认自己支持的版本；加字段不算升级，语义变化才升）
pub const PROTOCOL_VERSION: u32 = 1;

/// 清单文件名（放频道目录下；`build` 时自动跳过，不参与更新）
pub const MANIFEST_NAME: &str = "update.json";

/// 单个待更新文件
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// 相对路径（以 `/` 分隔，只能是普通文件）
    pub path: String,
    /// 字节数
    pub size: u64,
    /// sha256（64 位小写十六进制）
    pub sha256: String,
}

/// 频道更新清单
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// 清单协议版本，须等于 [`PROTOCOL_VERSION`]
    pub protocol: u32,
    /// 发布版本（仅显示与报告用，不参与判定）
    #[serde(default)]
    pub version: String,
    /// 发布时间文本（仅显示）
    #[serde(default)]
    pub date: Option<String>,
    /// 数据 schema 版本（仅数据频道使用；与程序期望值比对）
    #[serde(default)]
    pub schema_version: Option<u32>,
    /// 文件清单（非空）
    pub files: Vec<FileEntry>,
}

impl Manifest {
    /// 解析 JSON 文本并校验
    pub fn parse(text: &str) -> UpdateResult<Self> {
        let manifest: Self = serde_json::from_str(text).map_err(|err| UpdateError::Manifest {
            target: MANIFEST_NAME.to_string(),
            detail: format!("解析失败: {err}"),
        })?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// 解析 JSON 字节串并校验
    pub fn from_bytes(bytes: &[u8]) -> UpdateResult<Self> {
        let text = std::str::from_utf8(bytes).map_err(|err| UpdateError::Manifest {
            target: MANIFEST_NAME.to_string(),
            detail: format!("不是 UTF-8 文本: {err}"),
        })?;
        Self::parse(text)
    }

    /// 校验协议版本、文件表与路径安全
    pub fn validate(&self) -> UpdateResult<()> {
        if self.protocol != PROTOCOL_VERSION {
            return Err(UpdateError::Manifest {
                target: format!("{MANIFEST_NAME}/protocol"),
                detail: format!(
                    "协议版本 {} 不被支持（本程序支持 {PROTOCOL_VERSION}，需更新程序）",
                    self.protocol
                ),
            });
        }
        if self.files.is_empty() {
            return Err(UpdateError::Manifest {
                target: format!("{MANIFEST_NAME}/files"),
                detail: "文件清单为空".to_string(),
            });
        }
        let mut seen: Vec<&str> = Vec::with_capacity(self.files.len());
        for entry in &self.files {
            validate_path(&entry.path)?;
            if !hash::is_sha256_hex(&entry.sha256) {
                return Err(UpdateError::Manifest {
                    target: format!("{MANIFEST_NAME}/files/{}/sha256", entry.path),
                    detail: format!("不是 64 位小写十六进制: {}", entry.sha256),
                });
            }
            if seen.contains(&entry.path.as_str()) {
                return Err(UpdateError::Manifest {
                    target: format!("{MANIFEST_NAME}/files"),
                    detail: format!("路径重复: {}", entry.path),
                });
            }
            seen.push(&entry.path);
        }
        Ok(())
    }

    /// 文件总字节数（进度与报告用）
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    /// 按相对路径取条目
    #[must_use]
    pub fn file(&self, path: &str) -> Option<&FileEntry> {
        self.files.iter().find(|f| f.path == path)
    }

    /// 序列化为带缩进的 JSON 文本（发布侧写盘用，末尾带换行）
    pub fn to_json(&self) -> UpdateResult<String> {
        let mut text = serde_json::to_string_pretty(self).map_err(|err| UpdateError::Manifest {
            target: MANIFEST_NAME.to_string(),
            detail: format!("序列化失败: {err}"),
        })?;
        text.push('\n');
        Ok(text)
    }
}

/// 发布侧清单生成参数（对应 [`Manifest`] 的显示字段）
#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// 发布版本（如 `2026.10.19`）
    pub version: String,
    /// 发布时间文本
    pub date: Option<String>,
    /// 数据 schema 版本（仅数据频道）
    pub schema_version: Option<u32>,
}

impl BuildOptions {
    /// 以发布版本构造
    pub fn new(version: impl Into<String>) -> Self {
        Self {
            version: version.into(),
            date: None,
            schema_version: None,
        }
    }

    /// 记发布时间
    #[must_use]
    pub fn with_date(mut self, date: impl Into<String>) -> Self {
        self.date = Some(date.into());
        self
    }

    /// 记数据 schema 版本
    #[must_use]
    pub fn with_schema_version(mut self, version: u32) -> Self {
        self.schema_version = Some(version);
        self
    }
}

/// 递归扫描目录生成清单（**发布侧复用同一 schema**，避免手工维护 filelist 漂移）
///
/// 只收普通文件；符号链接等一律报错。`update.json` 自身跳过。路径按字典序排序，
/// 故同一目录两次生成结果逐位一致。
pub fn build(root: &Path, options: BuildOptions) -> UpdateResult<Manifest> {
    let mut files = Vec::new();
    collect(root, root, &mut files)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let manifest = Manifest {
        protocol: PROTOCOL_VERSION,
        version: options.version,
        date: options.date,
        schema_version: options.schema_version,
        files,
    };
    manifest.validate()?;
    Ok(manifest)
}

/// 校验清单内的相对路径：非空、无前导 `/`、无反斜杠与冒号、段内不含 `.` / `..`
pub fn validate_path(path: &str) -> UpdateResult<()> {
    let bad = |detail: String| UpdateError::Manifest {
        target: format!("{MANIFEST_NAME}/files/{path}"),
        detail,
    };
    if path.is_empty() {
        return Err(bad("路径为空".to_string()));
    }
    if path.starts_with('/') {
        return Err(bad("不接受绝对路径".to_string()));
    }
    if path.contains('\\') {
        return Err(bad("不接受反斜杠（用 `/` 分隔）".to_string()));
    }
    if path.contains(':') {
        return Err(bad("不接受盘符或冒号".to_string()));
    }
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(bad(format!("非法路径段 \"{part}\"")));
        }
    }
    Ok(())
}

/// 递归收集普通文件（跳过清单自身）
fn collect(root: &Path, dir: &Path, out: &mut Vec<FileEntry>) -> UpdateResult<()> {
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |source| UpdateError::Io { path, source }
    };
    let mut names: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(io(dir))?
        .map(|entry| entry.map(|e| e.path()).map_err(io(dir)))
        .collect::<UpdateResult<_>>()?;
    names.sort();
    for path in names {
        let file_type = fs::symlink_metadata(&path).map_err(io(&path))?.file_type();
        if file_type.is_dir() {
            collect(root, &path, out)?;
        } else if file_type.is_file() {
            if path.file_name().is_some_and(|name| name == MANIFEST_NAME) {
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .map_err(|_| UpdateError::Manifest {
                    target: path.display().to_string(),
                    detail: "不在扫描根目录内".to_string(),
                })?
                .to_string_lossy()
                .replace('\\', "/");
            validate_path(&rel)?;
            let size = fs::metadata(&path).map_err(io(&path))?.len();
            let sha256 = hash::sha256_file(&path)?;
            out.push(FileEntry { path: rel, size, sha256 });
        } else {
            return Err(UpdateError::Manifest {
                target: path.display().to_string(),
                detail: "只支持普通文件与目录（拒绝符号链接等）".to_string(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::sha256_hex;

    /// 建一个唯一的临时目录
    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("umaai-update-manifest-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 造一条合法条目
    fn entry(path: &str, size: u64, sha256: &str) -> FileEntry {
        FileEntry {
            path: path.to_string(),
            size,
            sha256: sha256.to_string(),
        }
    }

    /// 层1：协议版本不认识必须拒绝（提示升级程序）
    #[test]
    fn test_protocol_gate() {
        let json = format!(
            r#"{{"protocol": 2, "files": [{{"path":"a","size":1,"sha256":"{}"}}]}}"#,
            sha256_hex(b"a")
        );
        let err = Manifest::parse(&json).expect_err("协议版本 2 应被拒绝");
        assert!(format!("{err}").contains("协议版本"), "错误应点名协议版本: {err}");
    }

    /// 层1：未知字段宽容忽略（协议前向演进），缺必需字段报错
    #[test]
    fn test_unknown_field_tolerated_missing_field_rejected() {
        let ok = format!(
            r#"{{"protocol": 1, "note": "以后加的字段", "files": [{{"path":"a.txt","size":3,"sha256":"{}"}}]}}"#,
            sha256_hex(b"abc")
        );
        assert!(Manifest::parse(&ok).is_ok(), "未知字段应被忽略");

        let missing_files = r#"{"protocol": 1}"#;
        assert!(Manifest::parse(missing_files).is_err(), "缺 files 应报错");

        let missing_sha = r#"{"protocol": 1, "files": [{"path":"a.txt","size":3}]}"#;
        assert!(Manifest::parse(missing_sha).is_err(), "缺 sha256 应报错");
    }

    /// 层1：空文件表、哈希格式、重复路径都要拒绝
    #[test]
    fn test_empty_hash_format_and_duplicate() {
        let empty = r#"{"protocol": 1, "files": []}"#;
        assert!(Manifest::parse(empty).is_err(), "空文件表应报错");

        let bad_hash = r#"{"protocol": 1, "files": [{"path":"a.txt","size":3,"sha256":"ABC"}]}"#;
        assert!(Manifest::parse(bad_hash).is_err(), "非 64 位小写十六进制应报错");

        let sha = sha256_hex(b"abc");
        let dup = format!(
            r#"{{"protocol": 1, "files": [{{"path":"a.txt","size":3,"sha256":"{sha}"}},{{"path":"a.txt","size":3,"sha256":"{sha}"}}]}}"#
        );
        assert!(Manifest::parse(&dup).is_err(), "重复路径应报错");
    }

    /// 层1：路径安全 —— 绝对路径 / `..` / 反斜杠 / 盘符逐项拒绝
    #[test]
    fn test_path_safety() {
        for bad in [
            "",
            "/abs.txt",
            "a/../b.txt",
            "..",
            "a//b.txt",
            "a\\b.txt",
            "C:/x.txt",
            "a/./b.txt",
        ] {
            assert!(validate_path(bad).is_err(), "路径 \"{bad}\" 应被拒绝");
        }
        for good in ["a.txt", "data/a.txt", "a.b-c_d/e.txt"] {
            assert!(validate_path(good).is_ok(), "路径 \"{good}\" 应通过");
        }
        let json = format!(
            r#"{{"protocol": 1, "files": [{{"path":"../escape.txt","size":1,"sha256":"{}"}}]}}"#,
            sha256_hex(b"a")
        );
        assert!(Manifest::parse(&json).is_err(), "清单里的穿越路径应报错");
    }

    /// 层1：扫描目录生成清单 —— 稳定有序、跳过清单自身、逐文件记录哈希
    #[test]
    fn test_build_is_deterministic_and_skips_manifest() {
        let root = tmp("build");
        fs::write(root.join("a.txt"), b"aaa").unwrap();
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("sub/b.txt"), b"bbbb").unwrap();
        fs::write(root.join(MANIFEST_NAME), b"{}").unwrap();

        let options = BuildOptions::new("2026.10.19")
            .with_date("2026-10-19 10:00:00")
            .with_schema_version(1);
        let manifest = build(&root, options.clone()).unwrap();
        assert_eq!(manifest.protocol, PROTOCOL_VERSION);
        assert_eq!(manifest.version, "2026.10.19");
        assert_eq!(manifest.schema_version, Some(1));
        assert_eq!(manifest.files.len(), 2, "清单自身不参与更新");
        assert_eq!(manifest.files[0].path, "a.txt");
        assert_eq!(manifest.files[1].path, "sub/b.txt");
        assert_eq!(manifest.files[0].sha256, sha256_hex(b"aaa"));
        assert_eq!(manifest.total_bytes(), 7);

        let again = build(&root, options).unwrap();
        assert_eq!(manifest, again, "同一目录两次生成结果应逐位一致");
        let _ = fs::remove_dir_all(&root);
    }

    /// 层1：清单自身写盘再读回，字段不丢
    #[test]
    fn test_json_round_trip() {
        let manifest = Manifest {
            protocol: PROTOCOL_VERSION,
            version: "v1".to_string(),
            date: Some("2026-10-19".to_string()),
            schema_version: None,
            files: vec![entry("a.txt", 3, &sha256_hex(b"abc"))],
        };
        let text = manifest.to_json().unwrap();
        assert!(text.ends_with('\n'), "写盘文本应带换行");
        assert_eq!(Manifest::parse(&text).unwrap(), manifest);
        assert!(Manifest::from_bytes(text.as_bytes()).is_ok());
    }

    /// 层1：符号链接不进清单（发布目录被动手脚要看得见）
    #[cfg(unix)]
    #[test]
    fn test_build_rejects_symlink() {
        let root = tmp("symlink");
        fs::write(root.join("real.txt"), b"x").unwrap();
        std::os::unix::fs::symlink(root.join("real.txt"), root.join("link.txt")).unwrap();
        assert!(build(&root, BuildOptions::new("v")).is_err(), "符号链接应报错");
        let _ = fs::remove_dir_all(&root);
    }
}
