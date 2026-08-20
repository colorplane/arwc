use clap::{Parser, Subcommand};
use compress_arw::{
    decode, decoded_output_path, encode_with_progress, encoded_output_path, extract_preview,
    inspect, Kind, DEFAULT_LEVEL,
};
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "compress-arw",
    about = "Lossless Sony ARW compression as a JPEG container (.ARWC.JPG)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Write a .ARWC.JPG (camera preview + EXIF, compressed raw after EOI).
    Encode {
        input: PathBuf,
        /// Defaults to <stem>.ARWC.JPG next to the input.
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(short, long, default_value_t = DEFAULT_LEVEL)]
        level: i32,
        /// Print `progress <0-100>` lines on stderr.
        #[arg(long)]
        progress: bool,
    },
    /// Reconstruct the original ARW.
    Decode {
        input: PathBuf,
        /// Defaults to <stem>.ARW (from *.ARWC.JPG).
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Copy the leading JPEG (no zstd, no raw). Safe on a prefix of the file.
    Preview {
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    Info {
        input: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    match Cli::parse().cmd {
        Cmd::Encode {
            input,
            output,
            level,
            progress,
        } => {
            let output = output.unwrap_or_else(|| encoded_output_path(&input));
            let report = |pct: u8| {
                if progress {
                    let _ = writeln!(io::stderr(), "progress {pct}");
                    let _ = io::stderr().flush();
                }
            };
            report(0);
            let arw = fs::read(&input)?;
            report(3);
            let out = encode_with_progress(&arw, level, |pct| {
                report(3 + (u16::from(pct) * 94 / 100) as u8);
            })?;
            fs::write(&output, &out)?;
            report(100);
            eprintln!(
                "wrote {} ({} → {} bytes, {:.1}% of original)",
                output.display(),
                arw.len(),
                out.len(),
                100.0 * out.len() as f64 / arw.len() as f64
            );
        }
        Cmd::Decode { input, output } => {
            let output = output.unwrap_or_else(|| decoded_output_path(&input));
            let data = fs::read(&input)?;
            let verified = inspect(&data).ok().and_then(|i| i.orig_sha1).is_some();
            let arw = decode(&data)?;
            fs::write(&output, &arw)?;
            if verified {
                eprintln!(
                    "wrote {} ({} bytes, SHA-1 verified)",
                    output.display(),
                    arw.len()
                );
            } else {
                eprintln!("wrote {} ({} bytes)", output.display(), arw.len());
            }
        }
        Cmd::Preview { input, output } => {
            let data = fs::read(input)?;
            let jpeg = extract_preview(&data)?;
            fs::write(output, jpeg)?;
        }
        Cmd::Info { input, json } => {
            let data = fs::read(&input)?;
            let info = inspect(&data)?;
            if json {
                let kind = match info.kind {
                    Kind::JpegContainer => "jpeg_container",
                    Kind::SonyArw => "sony_arw",
                };
                let sha1 = info
                    .orig_sha1_hex()
                    .map(|h| format!("\"{h}\""))
                    .unwrap_or_else(|| "null".into());
                let orig = info
                    .orig_bytes
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "null".into());
                println!(
                    "{{\"kind\":\"{kind}\",\"encoded\":{},\"width\":{},\"height\":{},\"orientation\":{},\"preview_bytes\":{},\"file_bytes\":{},\"orig_bytes\":{orig},\"orig_sha1\":{sha1}}}",
                    info.encoded,
                    info.width,
                    info.height,
                    info.orientation,
                    info.preview_bytes,
                    data.len()
                );
            } else {
                println!("kind            {:?}", info.kind);
                println!("encoded         {}", info.encoded);
                println!("size            {}×{}", info.width, info.height);
                println!("orientation     {}", info.orientation);
                println!("preview_bytes   {}", info.preview_bytes);
                println!("file_bytes      {}", data.len());
                if let Some(n) = info.orig_bytes {
                    println!("orig_bytes      {n}");
                    if info.encoded && n > 0 {
                        println!(
                            "ratio           {:.2}:1 ({:.1}% of original)",
                            n as f64 / data.len() as f64,
                            100.0 * data.len() as f64 / n as f64
                        );
                    }
                }
                if let Some(h) = info.orig_sha1_hex() {
                    println!("orig_sha1       {h}");
                }
            }
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    if let Err(e) = run() {
        let _ = writeln!(io::stderr(), "{e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
