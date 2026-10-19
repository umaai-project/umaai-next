//! 更新声明：源（代码常量）与频道（本地目标）
//!
//! 分工是刻意的，也是安全纪律：
//!
//! - [`Source`] 由**产品 crate 以编译期常量**给出（更新源不记配置文件，用户改不了）；
//! - [`Channel`] 由**宿主**给出（装到哪个目录），远端清单只提供文件与哈希；
//! - 频道之间互相独立（各自清单、各自成败），「分别更新」由此成立。

use std::path::{Path, PathBuf};

use crate::error::{UpdateError, UpdateResult};

/// 更新源：基址 + 附加请求头（防盗链、UA 等）
///
/// 库不内置任何域名；基址写在产品自己的 crate 里，例如
/// `const UPDATE_BASE: &str = "https://<分发域名>/uma";`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    base: String,
    headers: Vec<(String, String)>,
}

impl Source {
    /// 以基址构造（去掉尾部 `/`；空串或含空白即报错）
    pub fn new(base: impl Into<String>) -> UpdateResult<Self> {
        let base = base.into();
        let trimmed = base.trim_end_matches('/').to_string();
        if trimmed.is_empty() {
            return Err(UpdateError::Manifest {
                target: "source".to_string(),
                detail: "更新源基址为空".to_string(),
            });
        }
        if trimmed.chars().any(char::is_whitespace) {
            return Err(UpdateError::Manifest {
                target: "source".to_string(),
                detail: format!("更新源基址含空白字符: \"{trimmed}\""),
            });
        }
        Ok(Self {
            base: trimmed,
            headers: Vec::new(),
        })
    }

    /// 追加一个请求头（可重复调用）
    #[must_use]
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// 基址
    #[must_use]
    pub fn base(&self) -> &str {
        &self.base
    }

    /// 附加请求头
    #[must_use]
    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    /// 频道清单地址：`<基址>/<频道 id>/update.json`
    #[must_use]
    pub fn manifest_url(&self, channel: &str) -> String {
        format!("{}/{}/{}", self.base, channel, crate::manifest::MANIFEST_NAME)
    }

    /// 频道内文件地址：`<基址>/<频道 id>/<相对路径>`（逐段百分号编码）
    #[must_use]
    pub fn file_url(&self, channel: &str, path: &str) -> String {
        let encoded: Vec<String> = path.split('/').map(encode_segment).collect();
        format!("{}/{}/{}", self.base, channel, encoded.join("/"))
    }
}

/// 频道安装目标
///
/// `#[non_exhaustive]`：自更新频道（程序本体 / 剧本 AI 本体）启用时新增变体，属非破坏演进。
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Target {
    /// 数据目录（宿主注入；桌面形态即发布目录下的数据根 / 小黑板工作目录）
    Dir(PathBuf),
}

impl Target {
    /// 取数据目录
    #[must_use]
    pub fn dir(&self) -> &Path {
        match self {
            Self::Dir(dir) => dir,
        }
    }
}

/// 本地频道声明
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    id: String,
    name: String,
    target: Target,
    schema_version: Option<u32>,
}

impl Channel {
    /// 声明一个数据频道（`id` 同时是远端目录名：只接受 `[A-Za-z0-9_-]`）
    pub fn data(id: impl Into<String>, name: impl Into<String>, dir: impl Into<PathBuf>) -> UpdateResult<Self> {
        let id = id.into();
        validate_id(&id)?;
        Ok(Self {
            id,
            name: name.into(),
            target: Target::Dir(dir.into()),
            schema_version: None,
        })
    }

    /// 声明数据 schema 门控：远端清单的 `schema_version` 必须等于该值，否则拒绝更新
    #[must_use]
    pub fn expect_schema(mut self, version: u32) -> Self {
        self.schema_version = Some(version);
        self
    }

    /// 频道 id（远端目录名）
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// 显示名
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 安装目标
    #[must_use]
    pub fn target(&self) -> &Target {
        &self.target
    }

    /// 数据目录
    #[must_use]
    pub fn dir(&self) -> &Path {
        self.target.dir()
    }

    /// 声明的数据 schema 版本（`None` = 不做门控）
    #[must_use]
    pub fn schema_version(&self) -> Option<u32> {
        self.schema_version
    }
}

/// 频道 id 合法性（用于 URL 段与临时目录名，杜绝路径穿越）
fn validate_id(id: &str) -> UpdateResult<()> {
    let ok =
        !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if ok {
        return Ok(());
    }
    Err(UpdateError::Manifest {
        target: "channel".to_string(),
        detail: format!("频道 id 非法（只允许 [A-Za-z0-9_-]，长度 1-64）: \"{id}\""),
    })
}

/// RFC 3986 未保留字符之外的字节转 `%XX`
fn encode_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        let keep = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~');
        if keep {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：基址规范化与非法基址
    #[test]
    fn test_source_base() {
        let source = Source::new("https://cdn.example/uma/").unwrap();
        assert_eq!(source.base(), "https://cdn.example/uma", "尾部斜杠应去掉");
        assert!(Source::new("").is_err(), "空基址应报错");
        assert!(Source::new("   ").is_err(), "空白基址应报错");
        assert!(Source::new("https://cdn.example/a b").is_err(), "含空白应报错");
    }

    /// 层1：请求头按调用顺序登记（防盗链 Referer 用）
    #[test]
    fn test_source_headers() {
        let source = Source::new("https://cdn.example/uma")
            .unwrap()
            .with_header("Referer", "https://site.example");
        assert_eq!(
            source.headers(),
            &[("Referer".to_string(), "https://site.example".to_string())]
        );
    }

    /// 层1：清单地址与文件地址（文件路径逐段百分号编码）
    #[test]
    fn test_urls() {
        let source = Source::new("https://cdn.example/uma").unwrap();
        assert_eq!(
            source.manifest_url("ai_data"),
            "https://cdn.example/uma/ai_data/update.json"
        );
        assert_eq!(
            source.file_url("ura_data", "events female.br"),
            "https://cdn.example/uma/ura_data/events%20female.br"
        );
        assert_eq!(
            source.file_url("ai_data", "sub/文件.json"),
            "https://cdn.example/uma/ai_data/sub/%E6%96%87%E4%BB%B6.json"
        );
    }

    /// 层1：频道 id 白名单（同时是临时目录名，杜绝路径穿越）
    #[test]
    fn test_channel_id() {
        assert!(Channel::data("ai_data", "AI 数据", "gamedata").is_ok());
        assert!(Channel::data("ura-data", "小黑板", "/tmp/x").is_ok());
        for bad in ["", "../evil", "a/b", "a b", "ai.data", "中文"] {
            assert!(Channel::data(bad, "x", "gamedata").is_err(), "id \"{bad}\" 应被拒绝");
        }
    }

    /// 层1：schema 门控声明与目录读取
    #[test]
    fn test_channel_schema_and_dir() {
        let channel = Channel::data("ai_data", "AI 数据", "gamedata")
            .unwrap()
            .expect_schema(1);
        assert_eq!(channel.schema_version(), Some(1));
        assert_eq!(channel.dir(), Path::new("gamedata"));
        assert_eq!(channel.target().dir(), Path::new("gamedata"));
        let plain = Channel::data("ura_data", "小黑板数据", "release/.portable").unwrap();
        assert_eq!(plain.schema_version(), None);
    }
}
