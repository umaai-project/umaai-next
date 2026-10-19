//! 传输层：默认 HTTP（`http` 特性，默认开），宿主也可自备实现
//!
//! 只保留两种形态——**HTTP** 与**本地测试模式**（[`crate::testing::TestTransport`]，内存假源）：
//! 离线导入、内网镜像等场景由宿主自己实现 [`Transport`]，库不内置本地目录源。

use std::io::Write;

use crate::error::UpdateResult;

/// 传输抽象：把 `url` 的内容写进 `sink`，返回写入字节数
///
/// 实现方负责请求头（`headers` 由 [`Source`](crate::Source) 携带，如防盗链 `Referer`）。
/// 非 2xx 状态码必须报错，不得把错误页当内容写进 `sink`。
pub trait Transport {
    /// 下载 `url` 到 `sink`
    fn download(&self, url: &str, headers: &[(String, String)], sink: &mut dyn Write) -> UpdateResult<u64>;
}

#[cfg(feature = "http")]
pub use http_impl::HttpTransport;

#[cfg(feature = "http")]
mod http_impl {
    use std::{io, time::Duration};

    use super::Transport;
    use crate::error::{UpdateError, UpdateResult};

    /// 默认连接超时（启动检查类短检查可再收紧）
    pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
    /// 默认整体超时（覆盖大文件下载）
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

    /// 默认 HTTP 传输（ureq + rustls，阻塞式，不引异步运行时）
    #[derive(Debug, Clone)]
    pub struct HttpTransport {
        agent: ureq::Agent,
    }

    impl HttpTransport {
        /// 以默认超时构造
        #[must_use]
        pub fn new() -> Self {
            Self::with_timeouts(DEFAULT_CONNECT_TIMEOUT, DEFAULT_TIMEOUT)
        }

        /// 以自定义「连接超时 / 整体超时」构造
        #[must_use]
        pub fn with_timeouts(connect: Duration, overall: Duration) -> Self {
            let config = ureq::Agent::config_builder()
                .timeout_connect(Some(connect))
                .timeout_global(Some(overall))
                .user_agent(concat!("umaai-update/", env!("CARGO_PKG_VERSION")))
                .build();
            Self {
                agent: ureq::Agent::new_with_config(config),
            }
        }
    }

    impl Default for HttpTransport {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Transport for HttpTransport {
        fn download(&self, url: &str, headers: &[(String, String)], sink: &mut dyn io::Write) -> UpdateResult<u64> {
            let mut request = self.agent.get(url);
            for (name, value) in headers {
                request = request.header(name.as_str(), value.as_str());
            }
            let mut response = request.call().map_err(|err| UpdateError::Http {
                url: url.to_string(),
                detail: err.to_string(),
            })?;
            let mut reader = response.body_mut().as_reader();
            io::copy(&mut reader, sink).map_err(|err| UpdateError::Http {
                url: url.to_string(),
                detail: format!("读取响应体失败: {err}"),
            })
        }
    }
}
