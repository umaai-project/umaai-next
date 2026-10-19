//! 检查与更新：频道独立、失败互不阻断
//!
//! 更新一条频道的固定顺序（也是全部安全性的来源）：
//!
//! 1. 拉清单 → 校验协议 / 路径 / schema 门控；
//! 2. 逐文件与本地 sha256 比对，算出待更新集合（**无状态判定**，可自愈）；
//! 3. 待更新文件**全部下载到工作目录下的临时目录**并逐个校验；
//! 4. 全部通过后才动目标目录：先备份被覆盖的旧文件，再逐个替换；
//! 5. 替换成功 → 删除整个临时目录（含备份）；失败 → 按备份回滚已替换文件。
//!
//! 失败时的临时目录**保留**（便于排查；下次运行会先清理同名目录），成功后删除。

use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use crate::{
    channel::{Channel, Source},
    error::{UpdateError, UpdateResult},
    hash,
    manifest::{FileEntry, Manifest},
    transport::Transport,
};

/// 工作目录下的临时目录名（下载与备份都在这里）
pub const WORK_DIR_NAME: &str = ".umaai-update";

/// 进度上报的最小字节间隔（避免小文件刷屏、大文件也够细）
const PROGRESS_STEP: u64 = 256 * 1024;

/// 更新选项
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    work_dir: PathBuf,
}

impl Options {
    /// 指定工作目录（临时目录建在 `<工作目录>/.umaai-update/` 下）
    #[must_use]
    pub fn new(work_dir: impl Into<PathBuf>) -> Self {
        Self { work_dir: work_dir.into() }
    }

    /// 工作目录
    #[must_use]
    pub fn work_dir(&self) -> &Path {
        &self.work_dir
    }

    /// 临时目录根
    #[must_use]
    pub fn temp_root(&self) -> PathBuf {
        self.work_dir.join(WORK_DIR_NAME)
    }
}

/// 下载进度（宿主自行呈现；库不打印任何东西）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress<'a> {
    /// 频道 id
    pub channel: &'a str,
    /// 文件相对路径
    pub path: &'a str,
    /// 已写入字节数
    pub done: u64,
    /// 该文件总字节数（清单登记值）
    pub total: u64,
}

/// 单频道检查结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckOutcome {
    /// 远端清单声明的发布版本
    pub version: String,
    /// 远端清单全部文件
    pub files: Vec<FileEntry>,
    /// 需要更新的文件（本地缺失或哈希不同）
    pub pending: Vec<FileEntry>,
    /// 需要更新的总字节数
    pub pending_bytes: u64,
}

impl CheckOutcome {
    /// 是否已是最新
    #[must_use]
    pub fn up_to_date(&self) -> bool {
        self.pending.is_empty()
    }
}

/// 单频道检查条目（`result` 为 `Err` 表示该频道检查失败，不影响其他频道）
#[derive(Debug)]
pub struct ChannelCheck {
    /// 频道 id
    pub id: String,
    /// 频道显示名
    pub name: String,
    /// 检查结果
    pub result: UpdateResult<CheckOutcome>,
}

/// 检查报告（按声明顺序逐频道）
#[derive(Debug)]
pub struct CheckReport {
    /// 各频道结果
    pub channels: Vec<ChannelCheck>,
}

impl CheckReport {
    /// 存在待更新的频道数
    #[must_use]
    pub fn pending(&self) -> usize {
        self.channels
            .iter()
            .filter(|c| c.result.as_ref().is_ok_and(|o| !o.up_to_date()))
            .count()
    }

    /// 检查失败的频道数
    #[must_use]
    pub fn failed(&self) -> usize {
        self.channels.iter().filter(|c| c.result.is_err()).count()
    }

    /// 全部频道都检查成功且已是最新
    #[must_use]
    pub fn up_to_date(&self) -> bool {
        self.failed() == 0 && self.pending() == 0
    }
}

/// 单频道更新结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelUpdateOutcome {
    /// 已是最新，未动任何文件
    UpToDate {
        /// 远端清单声明的发布版本
        version: String,
    },
    /// 已更新
    Updated {
        /// 远端清单声明的发布版本
        version: String,
        /// 被替换的文件相对路径
        files: Vec<String>,
        /// 被替换的总字节数
        bytes: u64,
    },
}

/// 单频道更新条目（`result` 为 `Err` 表示该频道更新失败，不影响其他频道）
#[derive(Debug)]
pub struct ChannelUpdate {
    /// 频道 id
    pub id: String,
    /// 频道显示名
    pub name: String,
    /// 更新结果
    pub result: UpdateResult<ChannelUpdateOutcome>,
}

/// 更新报告（按声明顺序逐频道）
#[derive(Debug)]
pub struct UpdateReport {
    /// 各频道结果
    pub channels: Vec<ChannelUpdate>,
}

impl UpdateReport {
    /// 更新失败的频道数
    #[must_use]
    pub fn failed(&self) -> usize {
        self.channels.iter().filter(|c| c.result.is_err()).count()
    }

    /// 发生实际替换的频道数
    #[must_use]
    pub fn updated(&self) -> usize {
        self.channels
            .iter()
            .filter(|c| matches!(c.result, Ok(ChannelUpdateOutcome::Updated { .. })))
            .count()
    }

    /// 全部频道都成功（含「已是最新」）
    #[must_use]
    pub fn all_ok(&self) -> bool {
        self.failed() == 0
    }
}

/// 检查各频道是否有更新（只读，不下载文件、不写盘）
#[must_use]
pub fn check(source: &Source, channels: &[Channel], transport: &dyn Transport) -> CheckReport {
    let mut out = Vec::with_capacity(channels.len());
    for channel in channels {
        out.push(ChannelCheck {
            id: channel.id().to_string(),
            name: channel.name().to_string(),
            result: check_channel(source, channel, transport),
        });
    }
    CheckReport { channels: out }
}

/// 更新各频道（下载 → 校验 → 替换；单频道失败不影响其他频道）
pub fn update(
    source: &Source,
    channels: &[Channel],
    transport: &dyn Transport,
    options: &Options,
    progress: &mut dyn FnMut(Progress<'_>),
) -> UpdateReport {
    let mut out = Vec::with_capacity(channels.len());
    for channel in channels {
        out.push(ChannelUpdate {
            id: channel.id().to_string(),
            name: channel.name().to_string(),
            result: update_channel(source, channel, transport, options, progress),
        });
    }
    UpdateReport { channels: out }
}

/// 单频道检查
fn check_channel(source: &Source, channel: &Channel, transport: &dyn Transport) -> UpdateResult<CheckOutcome> {
    let manifest = fetch_manifest(source, channel, transport)?;
    let pending = pending_files(channel.dir(), &manifest)?;
    let pending_bytes = pending.iter().map(|f| f.size).sum();
    Ok(CheckOutcome {
        version: manifest.version.clone(),
        files: manifest.files.clone(),
        pending,
        pending_bytes,
    })
}

/// 单频道更新
fn update_channel(
    source: &Source,
    channel: &Channel,
    transport: &dyn Transport,
    options: &Options,
    progress: &mut dyn FnMut(Progress<'_>),
) -> UpdateResult<ChannelUpdateOutcome> {
    let manifest = fetch_manifest(source, channel, transport)?;
    let dir = channel.dir();
    let pending = pending_files(dir, &manifest)?;
    if pending.is_empty() {
        return Ok(ChannelUpdateOutcome::UpToDate {
            version: manifest.version.clone(),
        });
    }

    let temp = options.temp_root().join(channel.id());
    if temp.exists() {
        fs::remove_dir_all(&temp).map_err(io_at(&temp))?;
    }
    let files_dir = temp.join("files");
    let backup_dir = temp.join("backup");

    // 全部下载并逐个校验：任何失败都在动目标目录之前发生
    let mut staged: Vec<(FileEntry, PathBuf)> = Vec::with_capacity(pending.len());
    for entry in &pending {
        let dest = files_dir.join(&entry.path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(io_at(parent))?;
        }
        let url = source.file_url(channel.id(), &entry.path);
        let mut file = fs::File::create(&dest).map_err(io_at(&dest))?;
        {
            let mut sink = ProgressSink::new(&mut file, channel.id(), &entry.path, entry.size, progress);
            sink.started();
            transport.download(&url, source.headers(), &mut sink)?;
            sink.finished();
        }
        let actual = hash::sha256_file(&dest)?;
        if actual != entry.sha256 {
            return Err(UpdateError::Hash {
                path: entry.path.clone(),
                expected: entry.sha256.clone(),
                actual,
            });
        }
        staged.push((entry.clone(), dest));
    }

    // 全部校验通过后才动目标目录；替换失败按备份回滚
    install_all(dir, &staged, &backup_dir)?;

    let bytes = pending.iter().map(|f| f.size).sum();
    let files = pending.iter().map(|f| f.path.clone()).collect();
    let _ = fs::remove_dir_all(&temp);
    // 临时目录根空了也一并清掉（`remove_dir` 只删空目录，失败无副作用）
    let _ = fs::remove_dir(options.temp_root());
    Ok(ChannelUpdateOutcome::Updated {
        version: manifest.version.clone(),
        files,
        bytes,
    })
}

/// 拉取并校验清单（含 schema 门控）
fn fetch_manifest(source: &Source, channel: &Channel, transport: &dyn Transport) -> UpdateResult<Manifest> {
    let url = source.manifest_url(channel.id());
    let mut buf = Vec::new();
    transport.download(&url, source.headers(), &mut buf)?;
    let manifest = Manifest::from_bytes(&buf)?;
    gate_schema(channel, &manifest)?;
    Ok(manifest)
}

/// 数据 schema 门控：频道声明了期望版本时，远端清单必须声明且相等
fn gate_schema(channel: &Channel, manifest: &Manifest) -> UpdateResult<()> {
    let Some(expected) = channel.schema_version() else {
        return Ok(());
    };
    match manifest.schema_version {
        Some(found) if found == expected => Ok(()),
        Some(found) => Err(UpdateError::Schema {
            channel: channel.id().to_string(),
            expected,
            found,
        }),
        None => Err(UpdateError::SchemaMissing {
            channel: channel.id().to_string(),
            expected,
        }),
    }
}

/// 与本地比对，算出需要更新的文件
fn pending_files(dir: &Path, manifest: &Manifest) -> UpdateResult<Vec<FileEntry>> {
    let mut pending = Vec::new();
    for entry in &manifest.files {
        if local_matches(&dir.join(&entry.path), entry)? {
            continue;
        }
        pending.push(entry.clone());
    }
    Ok(pending)
}

/// 本地文件是否与清单登记一致（大小先比、再比 sha256）
fn local_matches(path: &Path, entry: &FileEntry) -> UpdateResult<bool> {
    match fs::metadata(path) {
        Ok(meta) if meta.is_file() && meta.len() == entry.size => Ok(hash::sha256_file(path)? == entry.sha256),
        Ok(_) => Ok(false),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(UpdateError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// 已替换的文件（回滚用）：目标路径 + 旧文件备份（原本不存在时为 `None`）
struct Placed {
    target: PathBuf,
    backup: Option<PathBuf>,
}

/// 逐个替换：先备份旧文件，再落新文件；任一失败即回滚已替换项
fn install_all(dir: &Path, staged: &[(FileEntry, PathBuf)], backup_root: &Path) -> UpdateResult<()> {
    let mut placed: Vec<Placed> = Vec::with_capacity(staged.len());
    for (entry, src) in staged {
        let target = dir.join(&entry.path);
        if !target.starts_with(dir) {
            return Err(fail_with_rollback(
                &placed,
                UpdateError::Manifest {
                    target: entry.path.clone(),
                    detail: "目标路径越出数据目录".to_string(),
                },
            ));
        }
        let backup = match backup_existing(&target, backup_root, &entry.path) {
            Ok(backup) => backup,
            Err(err) => return Err(fail_with_rollback(&placed, err)),
        };
        if let Err(err) = place_file(src, &target) {
            return Err(fail_with_rollback(&placed, err));
        }
        placed.push(Placed { target, backup });
    }
    Ok(())
}

/// 旧文件备份到 `<临时目录>/backup/<相对路径>`（原本不存在则返回 `None`）
fn backup_existing(target: &Path, backup_root: &Path, rel: &str) -> UpdateResult<Option<PathBuf>> {
    if !target.exists() {
        return Ok(None);
    }
    let backup = backup_root.join(rel);
    if let Some(parent) = backup.parent() {
        fs::create_dir_all(parent).map_err(io_at(parent))?;
    }
    fs::copy(target, &backup).map_err(io_at(target))?;
    Ok(Some(backup))
}

/// 落一个文件：先复制到同目录暂存名，再 rename 就位（避免目标被写坏）
fn place_file(src: &Path, target: &Path) -> UpdateResult<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(io_at(parent))?;
    }
    let staging = staging_path(target);
    if let Err(source) = fs::copy(src, &staging) {
        let _ = fs::remove_file(&staging);
        return Err(UpdateError::Io { path: staging, source });
    }
    if let Err(source) = fs::rename(&staging, target) {
        let _ = fs::remove_file(&staging);
        return Err(UpdateError::Io {
            path: target.to_path_buf(),
            source,
        });
    }
    Ok(())
}

/// 暂存文件名：`.<文件名>.umaai-new`（与目标同目录，保证 rename 不跨设备）
fn staging_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "umaai-update".to_string());
    target.with_file_name(format!(".{name}.umaai-new"))
}

/// 回滚已替换的文件：有备份就还原，原本不存在就删除
fn rollback(placed: &[Placed]) -> Vec<String> {
    let mut failures = Vec::new();
    for item in placed.iter().rev() {
        let result = match &item.backup {
            Some(backup) => fs::copy(backup, &item.target).map(|_| ()),
            None => match fs::remove_file(&item.target) {
                Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
                other => other,
            },
        };
        if let Err(err) = result {
            failures.push(format!("{}: {err}", item.target.display()));
        }
    }
    failures
}

/// 回滚失败则换成「回滚不完整」错误，否则原样返回
fn fail_with_rollback(placed: &[Placed], origin: UpdateError) -> UpdateError {
    let failures = rollback(placed);
    if failures.is_empty() {
        return origin;
    }
    UpdateError::Rollback {
        detail: failures.join("; "),
        origin: origin.to_string(),
    }
}

/// 带 `io::Error` 标注的便捷构造
fn io_at(path: &Path) -> impl FnOnce(io::Error) -> UpdateError + '_ {
    move |source| UpdateError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// 边写边报进度的输出流包装
struct ProgressSink<'a> {
    inner: &'a mut dyn Write,
    channel: &'a str,
    path: &'a str,
    total: u64,
    done: u64,
    reported: u64,
    callback: &'a mut dyn FnMut(Progress<'_>),
}

impl<'a> ProgressSink<'a> {
    fn new(
        inner: &'a mut dyn Write,
        channel: &'a str,
        path: &'a str,
        total: u64,
        callback: &'a mut dyn FnMut(Progress<'_>),
    ) -> Self {
        Self {
            inner,
            channel,
            path,
            total,
            done: 0,
            reported: 0,
            callback,
        }
    }

    /// 开始时先报一次（`0 / total`）
    fn started(&mut self) {
        self.emit();
    }

    /// 结束时再报一次（保证末值可见）
    fn finished(&mut self) {
        self.emit();
    }

    fn emit(&mut self) {
        let (channel, path, done, total) = (self.channel, self.path, self.done, self.total);
        (self.callback)(Progress { channel, path, done, total });
        self.reported = done;
    }
}

impl Write for ProgressSink<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buf)?;
        self.done += written as u64;
        if self.done - self.reported >= PROGRESS_STEP || self.done >= self.total {
            self.emit();
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{hash::sha256_hex, manifest::PROTOCOL_VERSION, testing::TestTransport};

    const BASE: &str = "https://cdn.example/uma";

    /// 建一个唯一的临时目录
    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("umaai-update-flow-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 造一份清单 JSON（`schema` 为 `None` 是不声明 schema 版本）
    fn manifest_json(files: &[(&str, &[u8])], schema: Option<u32>) -> String {
        let entries: Vec<String> = files
            .iter()
            .map(|(path, bytes)| {
                format!(
                    r#"{{"path":"{path}","size":{},"sha256":"{}"}}"#,
                    bytes.len(),
                    sha256_hex(bytes)
                )
            })
            .collect();
        let schema = schema.map_or(String::new(), |v| format!(r#""schema_version":{v},"#));
        format!(
            r#"{{"protocol":{PROTOCOL_VERSION},"version":"v1",{schema}"files":[{}]}}"#,
            entries.join(",")
        )
    }

    /// 层1：临时目录落在工作目录下
    #[test]
    fn test_options_temp_root() {
        let options = Options::new("/work");
        assert_eq!(options.work_dir(), Path::new("/work"));
        assert_eq!(options.temp_root(), Path::new("/work").join(WORK_DIR_NAME));
    }

    /// 层1：暂存名与目标同目录（保证 rename 不跨设备）
    #[test]
    fn test_staging_path() {
        assert_eq!(
            staging_path(Path::new("/data/cardDB.json")),
            Path::new("/data/.cardDB.json.umaai-new")
        );
    }

    /// 层1：本地一致性判定 —— 命中 / 大小不符 / 缺失 / 同名目录都不算命中
    #[test]
    fn test_local_matches() {
        let dir = tmp("local-matches");
        let file = dir.join("a.txt");
        fs::write(&file, b"abc").unwrap();
        let entry = FileEntry {
            path: "a.txt".to_string(),
            size: 3,
            sha256: sha256_hex(b"abc"),
        };
        assert!(local_matches(&file, &entry).unwrap(), "内容一致应命中");
        let wrong_size = FileEntry {
            path: "a.txt".to_string(),
            size: 4,
            sha256: entry.sha256.clone(),
        };
        assert!(!local_matches(&file, &wrong_size).unwrap(), "大小不符不算命中");
        let wrong_hash = FileEntry {
            path: "a.txt".to_string(),
            size: 3,
            sha256: sha256_hex(b"xxx"),
        };
        assert!(!local_matches(&file, &wrong_hash).unwrap(), "哈希不符不算命中");
        assert!(
            !local_matches(&dir.join("missing.txt"), &entry).unwrap(),
            "缺失文件不算命中"
        );
        assert!(!local_matches(&dir, &entry).unwrap(), "目录不算命中");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 层1：schema 门控 —— 不声明、相等、不等三种情况
    #[test]
    fn test_schema_gate() {
        let dir = tmp("schema-gate");
        let channel = Channel::data("ai_data", "AI 数据", &dir).unwrap().expect_schema(1);
        let source = Source::new(BASE).unwrap();

        let declared = manifest_json(&[("a.txt", b"abc")], Some(1));
        let transport = TestTransport::new().serve_str(source.manifest_url("ai_data"), &declared);
        let report = check(&source, std::slice::from_ref(&channel), &transport);
        let outcome = report.channels[0].result.as_ref().expect("schema 相等应通过门控");
        assert_eq!(outcome.pending.len(), 1, "本地为空应列出 1 个待更新文件");

        let mismatched = manifest_json(&[("a.txt", b"abc")], Some(2));
        let transport = TestTransport::new().serve_str(source.manifest_url("ai_data"), &mismatched);
        let report = check(&source, std::slice::from_ref(&channel), &transport);
        let err = report.channels[0].result.as_ref().expect_err("schema 不等应失败");
        assert!(format!("{err}").contains("schema"), "错误应点名 schema: {err}");

        let missing = manifest_json(&[("a.txt", b"abc")], None);
        let transport = TestTransport::new().serve_str(source.manifest_url("ai_data"), &missing);
        let report = check(&source, std::slice::from_ref(&channel), &transport);
        assert!(report.channels[0].result.is_err(), "未声明 schema 应失败");
        let _ = fs::remove_dir_all(&dir);
    }

    /// 层1：本地与清单逐项一致即为最新
    #[test]
    fn test_check_up_to_date() {
        let dir = tmp("up-to-date");
        fs::write(dir.join("a.txt"), b"abc").unwrap();
        let source = Source::new(BASE).unwrap();
        let channel = Channel::data("ai_data", "AI 数据", &dir).unwrap();
        let manifest = manifest_json(&[("a.txt", b"abc")], None);
        let transport = TestTransport::new().serve_str(source.manifest_url("ai_data"), &manifest);

        let report = check(&source, std::slice::from_ref(&channel), &transport);
        let outcome = report.channels[0].result.as_ref().unwrap();
        assert!(outcome.up_to_date(), "内容一致应为最新");
        assert_eq!(outcome.pending_bytes, 0);
        assert!(report.up_to_date());
        assert_eq!(report.failed(), 0);
        let _ = fs::remove_dir_all(&dir);
    }
}
