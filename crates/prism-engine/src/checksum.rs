//! Streaming file checksums (MD5, SHA-1, SHA-256).

use std::io::Read;
use std::path::Path;

use sha2::Digest;

use crate::request::ChecksumAlgo;

/// Lower-case hex encoding.
pub(crate) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 15) as usize] as char);
    }
    s
}

fn hash_reader<D: Digest>(
    mut r: impl Read,
    mut cancelled: impl FnMut() -> bool,
) -> std::io::Result<String> {
    let mut h = D::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if cancelled() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "checksum cancelled",
            ));
        }
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

/// Hashes a file (blocking; call from `spawn_blocking`). Returns lower-case hex.
pub(crate) fn hash_file(
    path: &Path,
    algo: ChecksumAlgo,
    cancelled: impl FnMut() -> bool,
) -> std::io::Result<String> {
    let f = std::fs::File::open(path)?;
    match algo {
        ChecksumAlgo::Md5 => hash_reader::<md5::Md5>(f, cancelled),
        ChecksumAlgo::Sha1 => hash_reader::<sha1::Sha1>(f, cancelled),
        ChecksumAlgo::Sha256 => hash_reader::<sha2::Sha256>(f, cancelled),
    }
}

/// Normalizes a user-supplied hex digest for comparison.
pub(crate) fn normalize_hex(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_digests() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("abc");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(
            hash_file(&p, ChecksumAlgo::Md5, || false).unwrap(),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            hash_file(&p, ChecksumAlgo::Sha1, || false).unwrap(),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hash_file(&p, ChecksumAlgo::Sha256, || false).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(normalize_hex(" ABcd "), "abcd");
    }

    #[test]
    fn hashing_stops_between_chunks_when_cancelled() {
        let data = std::io::Cursor::new(vec![7; 4 << 20]);
        let mut checks = 0;
        let result = hash_reader::<sha2::Sha256>(data, || {
            checks += 1;
            checks > 2
        });
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::Interrupted);
        assert_eq!(checks, 3);
    }
}
