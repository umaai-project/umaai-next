//! 哈希工具（sha256，逐块流式）
//!
//! 判定与校验都走同一套实现：本地文件、下载文件、远端清单登记值三者可比。

use std::{fs::File, io::Read, path::Path};

use sha2::{Digest, Sha256};

use crate::error::{UpdateError, UpdateResult};

/// 读文件的块大小（64 KiB：`text_data_dict.json` 5.5 MB 也只占一块缓冲）
const CHUNK: usize = 64 * 1024;

/// 计算字节串的 sha256（小写十六进制）
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    to_hex(&hasher.finalize())
}

/// 计算文件的 sha256（小写十六进制），逐块读取不整文件入内存
pub fn sha256_file(path: &Path) -> UpdateResult<String> {
    let mut file = File::open(path).map_err(|source| UpdateError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0_u8; CHUNK];
    loop {
        let n = file.read(&mut buf).map_err(|source| UpdateError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(to_hex(&hasher.finalize()))
}

/// 是否为 64 位小写十六进制的 sha256 文本
#[must_use]
pub fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// 字节串转小写十六进制
fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 层1：已知向量（空串与 "abc"）
    #[test]
    fn test_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// 层1：文件哈希与字节哈希一致（跨块边界也一致）
    #[test]
    fn test_file_hash_matches_bytes() {
        let path = std::env::temp_dir().join(format!("umaai-update-hash-{}", std::process::id()));
        // 跨 64 KiB 块边界：多一块尾巴
        let bytes: Vec<u8> = (0..(CHUNK + 1)).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(sha256_file(&path).unwrap(), sha256_hex(&bytes));
        let _ = std::fs::remove_file(&path);
    }

    /// 层1：哈希文本格式判定
    #[test]
    fn test_is_sha256_hex() {
        assert!(is_sha256_hex(&sha256_hex(b"x")));
        assert!(!is_sha256_hex(""));
        assert!(!is_sha256_hex(&"a".repeat(63)));
        assert!(!is_sha256_hex(&"A".repeat(64)), "大写不接受");
        assert!(!is_sha256_hex(&"g".repeat(64)), "非十六进制字符不接受");
    }
}
