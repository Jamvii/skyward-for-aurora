//! The logs copied continuously into one file, so a crash and the
//! recovery's reload keep what led to them.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The basic browser's log, as the browser worker writes it.
pub const BASIC: &str = "/skyward/run/browser.log";
/// The full browser's Chromium log, inside its plugin root.
pub const FULL: &str = "/data/media/0/Skyward/plugins/chromium/tmp/chromium.log";

const EVERY: Duration = Duration::from_millis(500);
const STORAGE: &str = "/sdcard/Skyward/logs";
const FALLBACK: &str = "/skyward/run/logs";

/// The file the continuous log appends to, and the marker that starts it
/// again after the recovery restarts.
#[derive(Debug)]
pub struct Continuous {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    path: PathBuf,
    marker: PathBuf,
}

impl Continuous {
    /// Start appending to the continuous log, creating it if it is not
    /// there and leaving what an earlier run wrote.
    pub fn start() -> Result<Self> {
        let folder = folder();
        fs::create_dir_all(&folder).with_context(|| format!("make {}", folder.display()))?;
        let path = folder.join("continuous.log");
        let marker = folder.join("continuous.enabled");
        File::create(&marker).with_context(|| format!("write {}", marker.display()))?;
        let mut out = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("open {}", path.display()))?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        writeln!(out, "\n===== Skyward continuous log, {now} =====").context("write the continuous log")?;
        for (name, path) in sources() {
            writeln!(out, "===== {name}: {}", path.display()).context("write the continuous log")?;
        }
        out.flush().context("flush the continuous log")?;
        let _ = out.sync_all();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let written = path.clone();
        let thread = thread::Builder::new()
            .name("continuous-log".into())
            .spawn(move || follow(&written, &stopping))
            .context("start the continuous log thread")?;
        Ok(Self { stop, thread: Some(thread), path, marker })
    }

    /// Start it again when the marker says it was on before the recovery
    /// restarted, and internal storage is open.
    pub fn resume() -> Option<Self> {
        let marked = [STORAGE, FALLBACK]
            .iter()
            .map(|folder| Path::new(folder).join("continuous.enabled"))
            .any(|marker| marker.is_file());
        if marked { Self::start().ok() } else { None }
    }

    /// Where it appends, for the page to say.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Stop copying, forget the marker, and keep the file as it stands.
    pub fn stop(mut self) {
        fs::remove_file(&self.marker).ok();
        self.halt();
    }

    fn halt(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Continuous {
    fn drop(&mut self) {
        self.halt();
    }
}

/// The folder to write in: internal storage when it is open, and the
/// recovery's own memory until it is.
fn folder() -> PathBuf {
    if Path::new("/sdcard").is_dir() {
        PathBuf::from(STORAGE)
    } else {
        PathBuf::from(FALLBACK)
    }
}

fn sources() -> [(&'static str, PathBuf); 3] {
    [
        ("recovery", PathBuf::from(skyward_sys::LOG)),
        ("browser-basic", PathBuf::from(BASIC)),
        ("browser-full", PathBuf::from(FULL)),
    ]
}

#[cfg(target_os = "android")]
fn kernel() -> Option<String> {
    skyward_sys::kernel_log().ok()
}

#[cfg(not(target_os = "android"))]
fn kernel() -> Option<String> {
    None
}

fn follow(path: &Path, stop: &AtomicBool) {
    let Ok(mut out) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let mut offsets: HashMap<PathBuf, u64> = HashMap::new();
    let mut kernel_seen = 0usize;
    while !stop.load(Ordering::Relaxed) {
        thread::sleep(EVERY);
        if stop.load(Ordering::Relaxed) {
            break;
        }
        for (name, source) in sources() {
            let mut file = match File::open(&source) {
                Ok(file) => file,
                Err(_) => continue,
            };
            let len = file.metadata().map(|m| m.len()).unwrap_or(0);
            let offset = offsets.get(&source).copied().unwrap_or(0);
            if len < offset {
                offsets.insert(source.clone(), 0);
            }
            let from = offsets.get(&source).copied().unwrap_or(0);
            if len > from {
                if from == 0 {
                    let _ = writeln!(out, "\n===== {name} =====");
                }
                if file.seek(SeekFrom::Start(from)).is_ok() {
                    let mut bytes = Vec::new();
                    if file.read_to_end(&mut bytes).is_ok() {
                        let _ = out.write_all(&bytes);
                        offsets.insert(source.clone(), from + bytes.len() as u64);
                    }
                }
            }
        }
        if let Some(log) = kernel() {
            let bytes = log.as_bytes();
            if bytes.len() > kernel_seen {
                let _ = out.write_all(&bytes[kernel_seen..]);
                kernel_seen = bytes.len();
            } else if bytes.len() < kernel_seen {
                kernel_seen = 0;
            }
        }
        let _ = out.flush();
        let _ = out.sync_all();
    }
}
