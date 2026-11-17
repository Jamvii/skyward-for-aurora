//! A small controller used while bringing the AERA runtime up: start the
//! jail, open the built-in start page, poll for one frame, write it as a
//! PPM and print the worker's status.

use skyward_aera::{Kind, Process};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn main() {
    let mut args = std::env::args_os().skip(1);
    let Some(runtime) = args.next() else {
        eprintln!("usage: aera-probe <runtime> [output.ppm]");
        std::process::exit(2);
    };
    let out = args.next().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp/aera-frame.ppm"));
    let url = args.next().map(|u| u.to_string_lossy().into_owned()).unwrap_or_else(|| "aera://start".into());
    let mut process = match Process::start(PathBuf::from(runtime).as_path()) {
        Ok(process) => process,
        Err(error) => {
            eprintln!("start: {error:#}");
            std::process::exit(1);
        }
    };
    let session = process.session();
    if !session.open(&url) {
        eprintln!("open: {}", session.status());
        std::process::exit(1);
    }
    let until = Instant::now() + Duration::from_secs(20);
    let mut wrote = false;
    let mut frames = 0u32;
    while Instant::now() < until {
        if !process.running() {
            eprintln!("the worker ended: {}", process.session().status());
            break;
        }
        if process.session().poll() {
            let frame = process.session().frame().expect("a polled frame is mapped").to_vec();
            let sequence = process.session().sequence();
            frames += 1;
            let uniform = frame.iter().all(|&b| b == frame[0]);
            eprintln!("frame {sequence}: uniform={uniform} first={:02x?}", &frame[..4]);
            if !uniform {
                let mut ppm = Vec::with_capacity(frame.len() / 4 * 3 + 32);
                ppm.extend_from_slice(b"P6\n1080 2100\n255\n");
                for pixel in frame.chunks_exact(4) {
                    ppm.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
                }
                std::fs::write(&out, ppm).expect("write the frame");
                eprintln!("saved {} after {frames} frames", out.display());
                wrote = true;
            }
            process.session().acknowledge();
            if wrote {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !wrote {
        eprintln!("no frame: {}", process.session().status());
        std::process::exit(1);
    }
    // The worker is persistent; the probe leaves it running only until it
    // is dropped, which stops the jail's process group.
    let _ = Kind::Close;
}
