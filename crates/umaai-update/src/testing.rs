//! 本地测试模式：内存假传输（不联网、不碰磁盘）
//!
//! 给三类消费者用：库自身的集成测试、宿主自测（结果可复现）、离线演练（把远端内容
//! 先准备好再跑一遍更新流程）。它不是「本地目录源」——正式离线导入由宿主自己实现
//! [`Transport`]。

use std::{cell::RefCell, collections::BTreeMap, io::Write};

use crate::{
    error::{UpdateError, UpdateResult},
    transport::Transport,
};

/// 按 URL 提供内容的假传输；可注入失败，并记录被请求过的 URL
#[derive(Debug, Default)]
pub struct TestTransport {
    files: BTreeMap<String, Vec<u8>>,
    failures: BTreeMap<String, String>,
    requests: RefCell<Vec<String>>,
}

impl TestTransport {
    /// 建一个空假源
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 挂一个 URL 的内容
    #[must_use]
    pub fn serve(mut self, url: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        self.files.insert(url.into(), bytes.into());
        self
    }

    /// 挂一个 URL 的文本内容
    #[must_use]
    pub fn serve_str(self, url: impl Into<String>, text: &str) -> Self {
        self.serve(url, text.as_bytes().to_vec())
    }

    /// 让某个 URL 在下载时报错（模拟网络失败 / 404）
    #[must_use]
    pub fn fail(mut self, url: impl Into<String>, detail: &str) -> Self {
        self.failures.insert(url.into(), detail.to_string());
        self
    }

    /// 已收到的请求顺序（断言「只拉变化文件」「请求了哪些地址」用）
    #[must_use]
    pub fn requests(&self) -> Vec<String> {
        self.requests.borrow().clone()
    }
}

impl Transport for TestTransport {
    fn download(&self, url: &str, _headers: &[(String, String)], sink: &mut dyn Write) -> UpdateResult<u64> {
        self.requests.borrow_mut().push(url.to_string());
        if let Some(detail) = self.failures.get(url) {
            return Err(UpdateError::Http {
                url: url.to_string(),
                detail: detail.clone(),
            });
        }
        let Some(bytes) = self.files.get(url) else {
            return Err(UpdateError::Http {
                url: url.to_string(),
                detail: "假源没有该 URL".to_string(),
            });
        };
        sink.write_all(bytes).map_err(|source| UpdateError::Io {
            path: std::path::PathBuf::from("<测试输出流>"),
            source,
        })?;
        Ok(bytes.len() as u64)
    }
}
