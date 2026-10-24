//! Ghép tay để thử hoặc đo: `cargo run --release -p utd-mp4 --example merge -- <ra.mp4> <hình.mp4>
//! <tiếng.m4a>` (chỉ một file tiếng thì ra m4a). In báo cáo và thời gian; chưa hỗ trợ thì in lý do.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use utd_mp4::{Brand, Input, Kind, merge};

fn main() -> ExitCode {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let (inputs, brand) = match args.as_slice() {
        [_, video, audio] => (
            vec![
                Input {
                    path: video.clone(),
                    kind: Kind::Video,
                },
                Input {
                    path: audio.clone(),
                    kind: Kind::Audio,
                },
            ],
            Brand::Mp4,
        ),
        [_, audio] => (
            vec![Input {
                path: audio.clone(),
                kind: Kind::Audio,
            }],
            Brand::M4a,
        ),
        _ => {
            eprintln!("cách dùng: merge <ra> <hình> <tiếng> | merge <ra> <tiếng>");
            return ExitCode::FAILURE;
        }
    };
    let started = Instant::now();
    match merge(&inputs, &args[0], brand, &AtomicBool::new(false)) {
        Ok(report) => {
            println!("{report:?} trong {:?}", started.elapsed());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
