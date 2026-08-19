//! Roundtrip SHA-256 checks against
//! <https://github.com/colorplane/CompressARWTestData>
//!
//! Clone into `testdata/` (gitignored) or set `COMPRESS_ARW_TESTDATA`:
//! `git clone --depth 1 https://github.com/colorplane/CompressARWTestData testdata`

use compress_arw::{decode, encode_with_level};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

fn testdata_root() -> PathBuf {
    if let Ok(p) = std::env::var("COMPRESS_ARW_TESTDATA") {
        return PathBuf::from(p);
    }
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for p in [
        crate_dir.join("testdata"),
        crate_dir.join("CompressARWTestData"),
        crate_dir.join("../CompressARWTestData"),
    ] {
        if p.is_dir() {
            return p;
        }
    }
    crate_dir.join("testdata")
}

fn collect_arw(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().is_some_and(|n| n == ".git") {
            continue;
        }
        if path.is_dir() {
            collect_arw(&path, out);
            continue;
        }
        let is_arw = path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("arw"));
        if is_arw {
            out.push(path);
        }
    }
}

fn github_arw_files() -> Vec<PathBuf> {
    let root = testdata_root();
    let mut files = Vec::new();
    collect_arw(&root, &mut files);
    files.sort();
    files
}

fn require_arw_files() -> Vec<PathBuf> {
    let files = github_arw_files();
    assert!(
        !files.is_empty(),
        "no .ARW files in {}. Clone with:\n  git clone --depth 1 https://github.com/colorplane/CompressARWTestData testdata",
        testdata_root().display()
    );
    files
}

fn tmp_dir(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "compress-arw-realdata-{}-{}-{name}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_compress-arw"))
}

#[test]
fn github_testdata_library_and_cli_roundtrip_same_sha256() {
    let files = require_arw_files();
    let work = tmp_dir("roundtrip");

    for arw_path in &files {
        let original = fs::read(arw_path).unwrap_or_else(|e| {
            panic!("read {}: {e}", arw_path.display())
        });
        let want = sha256_hex(&original);

        let encoded = encode_with_level(&original, 3).unwrap_or_else(|e| {
            panic!("encode {}: {e}", arw_path.display())
        });
        let decoded = decode(&encoded).unwrap_or_else(|e| {
            panic!("decode {}: {e}", arw_path.display())
        });
        let got_lib = sha256_hex(&decoded);
        assert_eq!(
            got_lib, want,
            "library roundtrip SHA-256 mismatch for {}",
            arw_path.display()
        );
        assert_eq!(
            decoded, original,
            "library roundtrip bytes mismatch for {}",
            arw_path.display()
        );

        let stem = arw_path.file_stem().unwrap().to_string_lossy();
        let in_arw = work.join(format!("{stem}.ARW"));
        let out_jpg = work.join(format!("{stem}.ARWC.JPG"));
        let back_arw = work.join(format!("{stem}-back.ARW"));
        fs::write(&in_arw, &original).unwrap();

        let enc_status = bin()
            .args([
                "encode",
                in_arw.to_str().unwrap(),
                "-o",
                out_jpg.to_str().unwrap(),
                "-l",
                "3",
            ])
            .status()
            .unwrap();
        assert!(
            enc_status.success(),
            "cli encode failed for {}",
            arw_path.display()
        );

        let dec_status = bin()
            .args([
                "decode",
                out_jpg.to_str().unwrap(),
                "-o",
                back_arw.to_str().unwrap(),
            ])
            .status()
            .unwrap();
        assert!(
            dec_status.success(),
            "cli decode failed for {}",
            arw_path.display()
        );

        let from_cli = fs::read(&back_arw).unwrap();
        let got_cli = sha256_hex(&from_cli);
        assert_eq!(
            got_cli, want,
            "CLI roundtrip SHA-256 mismatch for {}\n  original {want}\n  decoded  {got_cli}",
            arw_path.display()
        );
        assert_eq!(
            from_cli, original,
            "CLI roundtrip bytes mismatch for {}",
            arw_path.display()
        );

        eprintln!(
            "ok {}  sha256={want}  {} → {} bytes",
            arw_path
                .strip_prefix(testdata_root())
                .unwrap_or(arw_path)
                .display(),
            original.len(),
            encoded.len()
        );
    }

    let _ = fs::remove_dir_all(&work);
}
