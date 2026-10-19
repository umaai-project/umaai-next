//! # umaai-update（分发客户端公共库）
//!
//! 把「更新」拆成可测的四件事：**清单协议 → 传输 → 校验 → 安装**。
//!
//! - 零引擎依赖：不依赖 `umaai-core` / `umasim` / `umaai`，也不碰终端与呈现；
//! - 更新源是调用方的编译期常量（[`Source`]），库不读配置、不内置域名；
//! - 目标目录由宿主注入（[`Target`]），远端清单**不能**指定安装路径；
//! - 判定无状态：逐文件 sha256 与本地比对，不依赖本地版本文件；
//! - 安装口径：全部下载到工作目录下的临时目录 → 全部校验 → 替换（失败回滚）→ 成功后删除临时目录；
//! - 频道之间互相独立（各自清单、各自成败），「AI 数据 / 小黑板数据分别更新」由此成立。
//!
//! 典型用法（宿主持有源常量与频道声明）：
//!
//! ```no_run
//! use umaai_update::{Channel, Options, Source, TestTransport, check, update};
//!
//! let source = Source::new("https://<分发域名>/uma")?.with_header("Referer", "https://<站点>");
//! let channels = [
//!     Channel::data("ai_data", "AI 数据", "gamedata")?.expect_schema(1),
//!     Channel::data("ura_data", "小黑板数据", "release/.portable")?,
//! ];
//! let transport = TestTransport::new(); // 正式形态用 `HttpTransport`
//! let report = check(&source, &channels, &transport);
//! if !report.up_to_date() {
//!     let mut last = 0;
//!     let _updated = update(
//!         &source,
//!         &channels,
//!         &transport,
//!         &Options::new("."),
//!         &mut |p| {
//!             if p.done != last {
//!                 last = p.done;
//!             }
//!         },
//!     );
//! }
//! # Ok::<(), umaai_update::UpdateError>(())
//! ```
//!
//! 方案与拍板记录见 `docs/plan/tools_update.md`。

#![warn(missing_docs)]

pub mod channel;
pub mod error;
pub mod hash;
pub mod manifest;
pub mod testing;
pub mod transport;
pub mod updater;

pub use channel::{Channel, Source, Target};
pub use error::{UpdateError, UpdateResult};
pub use hash::{sha256_file, sha256_hex};
pub use manifest::{BuildOptions, FileEntry, MANIFEST_NAME, Manifest, PROTOCOL_VERSION};
pub use testing::TestTransport;
#[cfg(feature = "http")]
pub use transport::HttpTransport;
pub use transport::Transport;
pub use updater::{
    ChannelCheck, ChannelUpdate, ChannelUpdateOutcome, CheckOutcome, CheckReport, Options, Progress, UpdateReport,
    check, update,
};
