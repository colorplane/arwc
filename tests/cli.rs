mod common;

use common::default_arw;
use compress_arw::{decode, extract_preview, inspect};
use std::fs;
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_compress-arw"))
}

fn tmp(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "compress-arw-{}-{}-{name}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    p
}

#[test]
fn cli_encode_preview_decode() {
    let dir = tmp("cli");
    fs::create_dir_all(&dir).unwrap();
    let arw_path = dir.join("in.ARW");
    let jpg_path = dir.join("out.arwc.jpg");
    let prev_path = dir.join("prev.jpg");
    let back_path = dir.join("back.ARW");
    let arw = default_arw();
    fs::write(&arw_path, &arw).unwrap();

    assert!(bin()
        .args([
            "encode",
            arw_path.to_str().unwrap(),
            "-o",
            jpg_path.to_str().unwrap(),
            "-l",
            "3"
        ])
        .status()
        .unwrap()
        .success());

    let jpg = fs::read(&jpg_path).unwrap();
    assert_eq!(&jpg[..2], &[0xff, 0xd8]);
    assert_eq!(decode(&jpg).unwrap(), arw);

    assert!(bin()
        .args([
            "preview",
            jpg_path.to_str().unwrap(),
            "-o",
            prev_path.to_str().unwrap()
        ])
        .status()
        .unwrap()
        .success());
    let prev = fs::read(&prev_path).unwrap();
    assert_eq!(prev, extract_preview(&jpg).unwrap());
    assert_eq!(prev.len(), inspect(&jpg).unwrap().preview_bytes as usize);

    assert!(bin()
        .args([
            "decode",
            jpg_path.to_str().unwrap(),
            "-o",
            back_path.to_str().unwrap()
        ])
        .status()
        .unwrap()
        .success());
    assert_eq!(fs::read(&back_path).unwrap(), arw);

    let info = bin()
        .args(["info", jpg_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(info.status.success());
    let stdout = String::from_utf8_lossy(&info.stdout);
    assert!(stdout.contains("JpegContainer"));
    assert!(stdout.contains("encoded         true"));

    let json = bin()
        .args(["info", "--json", jpg_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(json.status.success());
    let body = String::from_utf8_lossy(&json.stdout);
    assert!(body.contains("\"kind\":\"jpeg_container\""));
    assert!(body.contains("\"encoded\":true"));
    assert!(body.contains("\"orig_bytes\":"));
    assert!(body.contains("\"orig_sha1\":\""));
    let info_txt = String::from_utf8_lossy(&info.stdout);
    assert!(info_txt.contains("orig_bytes"));
    assert!(info_txt.contains("orig_sha1"));
    assert!(info_txt.contains("ratio"));

    let prog = bin()
        .args([
            "encode",
            arw_path.to_str().unwrap(),
            "-o",
            dir.join("prog.arwc.jpg").to_str().unwrap(),
            "-l",
            "3",
            "--progress",
        ])
        .output()
        .unwrap();
    assert!(prog.status.success());
    let err = String::from_utf8_lossy(&prog.stderr);
    assert!(err.contains("progress 0"), "{err}");
    assert!(err.contains("progress 100"), "{err}");
    let mut max_mid = 0u8;
    for line in err.lines() {
        if let Some(rest) = line.strip_prefix("progress ") {
            if let Ok(n) = rest.trim().parse::<u8>() {
                if n < 100 {
                    max_mid = max_mid.max(n);
                }
            }
        }
    }
    assert!(
        max_mid > 5,
        "progress was stuck at {max_mid}% (u8 overflow?)\n{err}"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cli_encode_defaults_to_arwc_jpg() {
    let dir = tmp("cli-default-ext");
    fs::create_dir_all(&dir).unwrap();
    let arw_path = dir.join("shot.ARW");
    fs::write(&arw_path, default_arw()).unwrap();

    assert!(bin()
        .args(["encode", arw_path.to_str().unwrap(), "-l", "3"])
        .status()
        .unwrap()
        .success());

    let out = dir.join("shot.ARWC.JPG");
    assert!(out.is_file(), "expected {}", out.display());
    assert_eq!(&fs::read(&out).unwrap()[..2], &[0xff, 0xd8]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn cli_decode_defaults_to_arw() {
    let dir = tmp("cli-decode-default");
    fs::create_dir_all(&dir).unwrap();
    let arw_path = dir.join("shot.ARW");
    let arw = default_arw();
    fs::write(&arw_path, &arw).unwrap();
    assert!(bin()
        .args(["encode", arw_path.to_str().unwrap(), "-l", "3"])
        .status()
        .unwrap()
        .success());
    fs::remove_file(&arw_path).unwrap();
    assert!(bin()
        .args(["decode", dir.join("shot.ARWC.JPG").to_str().unwrap()])
        .status()
        .unwrap()
        .success());
    assert_eq!(fs::read(&arw_path).unwrap(), arw);
    let _ = fs::remove_dir_all(&dir);
}
