//! Platform glue. Both back-ends fill a `Job` whose `inbox` receives the finished tree.

use eframe::egui;
use spacehunter_core::{Progress, Tree};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub struct Loaded {
    pub tree: Tree,
    pub path: Option<PathBuf>,
    /// (total, free) bytes of the volume, if known.
    pub disk: Option<(u64, u64)>,
}

pub struct Job {
    pub progress: Arc<Progress>,
    pub inbox: Arc<Mutex<Option<Loaded>>>,
    pub label: String,
}

impl Job {
    pub fn new(label: impl Into<String>) -> Self {
        Job {
            progress: Arc::new(Progress::default()),
            inbox: Arc::new(Mutex::new(None)),
            label: label.into(),
        }
    }
}

pub fn demo(ctx: &egui::Context) -> Job {
    let job = Job::new("demo data");
    let (inbox, progress, ctx) = (job.inbox.clone(), job.progress.clone(), ctx.clone());
    let work = move || {
        let tree = spacehunter_core::sample::generate(7, 60_000);
        progress.files.store(
            tree.node(0).files as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
        *inbox.lock().unwrap() = Some(Loaded {
            tree,
            path: None,
            disk: None,
        });
        progress
            .done
            .store(true, std::sync::atomic::Ordering::Relaxed);
        ctx.request_repaint();
    };
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(work);
    #[cfg(target_arch = "wasm32")]
    work();
    job
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use spacehunter_core::scan::{self, ScanOptions};
    use std::sync::atomic::Ordering::Relaxed;

    pub const CAN_SCAN_PATHS: bool = true;

    pub fn pick_folder(title: &str) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title(title).pick_folder()
    }

    pub fn start_scan(ctx: &egui::Context, path: PathBuf, opts: ScanOptions) -> Job {
        let job = Job::new(path.display().to_string());
        let (inbox, progress, ctx2) = (job.inbox.clone(), job.progress.clone(), ctx.clone());
        std::thread::spawn(move || {
            // Repaint ticker so the counters animate while scanning.
            let (p2, c3) = (progress.clone(), ctx2.clone());
            std::thread::spawn(move || {
                while !p2.done.load(Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(80));
                    c3.request_repaint();
                }
            });
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                scan::scan(&path, opts, progress.clone())
            }));
            match r {
                Ok(tree) => {
                    let disk = scan::disk_space(&path);
                    if !progress.cancel.load(Relaxed) {
                        *inbox.lock().unwrap() = Some(Loaded {
                            tree,
                            path: Some(path),
                            disk,
                        });
                    }
                }
                Err(e) => {
                    let msg = e
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_else(|| "scanner panicked".into());
                    *progress.error.lock().unwrap() = Some(format!("scan failed: {msg}"));
                }
            }
            progress.done.store(true, Relaxed);
            ctx2.request_repaint();
        });
        job
    }

    /// Drives / mount points offered in the "Open drive" menu.
    pub fn roots() -> Vec<(String, PathBuf)> {
        let mut v = Vec::new();
        #[cfg(windows)]
        for c in b'A'..=b'Z' {
            let p = PathBuf::from(format!("{}:\\", c as char));
            if std::fs::metadata(&p).is_ok() {
                v.push((format!("{}:\\", c as char), p));
            }
        }
        #[cfg(not(windows))]
        {
            v.push(("/ (root)".into(), PathBuf::from("/")));
            if let Some(h) = std::env::var_os("HOME") {
                v.push(("Home".into(), PathBuf::from(h)));
            }
            if let Ok(rd) = std::fs::read_dir("/mnt") {
                for e in rd.flatten() {
                    if e.path().is_dir() {
                        v.push((e.path().display().to_string(), e.path()));
                    }
                }
            }
        }
        v
    }

    pub fn reveal(path: &std::path::Path, is_dir: bool) {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let mut c = std::process::Command::new("explorer");
            c.creation_flags(0x0800_0000);
            if is_dir {
                c.arg(path);
            } else {
                c.arg(format!("/select,{}", path.display()));
            }
            let _ = c.spawn();
        }
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open")
                .arg(if is_dir {
                    path.to_path_buf()
                } else {
                    path.parent().unwrap_or(path).to_path_buf()
                })
                .spawn();
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let _ = std::process::Command::new("xdg-open")
                .arg(if is_dir {
                    path.to_path_buf()
                } else {
                    path.parent().unwrap_or(path).to_path_buf()
                })
                .spawn();
        }
    }

    /// Open the file with its default application.
    pub fn run(path: &std::path::Path) {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("cmd")
                .args(["/C", "start", ""])
                .arg(path)
                .creation_flags(0x0800_0000)
                .spawn();
        }
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(path).spawn();
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let _ = std::process::Command::new("xdg-open").arg(path).spawn();
        }
    }

    pub fn delete(path: &std::path::Path, is_dir: bool) -> Result<(), String> {
        let r = if is_dir {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        };
        r.map_err(|e| e.to_string())
    }

    pub fn default_opts(size_on_disk: bool) -> ScanOptions {
        ScanOptions {
            size_on_disk,
            one_filesystem: true,
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(target_arch = "wasm32")]
pub use crate::web::*;
