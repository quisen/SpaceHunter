//! Parallel native disk scanner (not available on wasm).

use crate::progress::Progress;
use crate::tree::{NodeId, Tree, TreeBuilder};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct ScanOptions {
    /// Report allocated size (clusters/blocks) instead of logical length – what really frees up on delete.
    pub size_on_disk: bool,
    /// Do not descend into other mount points / volumes.
    pub one_filesystem: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            size_on_disk: true,
            one_filesystem: true,
        }
    }
}

struct Shared {
    builder: Mutex<TreeBuilder>,
    opts: ScanOptions,
    progress: Arc<Progress>,
    #[cfg(unix)]
    root_dev: u64,
    #[cfg(unix)]
    seen: Mutex<HashSet<(u64, u64)>>,
    #[cfg(windows)]
    cluster: u64,
}

pub fn scan(root: &Path, opts: ScanOptions, progress: Arc<Progress>) -> Tree {
    let name = root
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string_lossy().into_owned());

    let shared = Shared {
        builder: Mutex::new(TreeBuilder::new(&name)),
        #[cfg(unix)]
        root_dev: {
            use std::os::unix::fs::MetadataExt;
            std::fs::metadata(root).map(|m| m.dev()).unwrap_or(0)
        },
        #[cfg(unix)]
        seen: Mutex::new(HashSet::new()),
        #[cfg(windows)]
        cluster: cluster_size(root),
        opts,
        progress: progress.clone(),
    };
    rayon::scope(|s| walk(s, &shared, root.to_path_buf(), 0));
    // `done` is set by the caller only after the result has been handed over (otherwise the UI can see
    // "done" with an empty inbox and silently drop the scan).
    shared.builder.into_inner().unwrap().finish()
}

fn walk<'s>(scope: &rayon::Scope<'s>, sh: &'s Shared, dir: PathBuf, id: NodeId) {
    if sh.progress.cancel.load(Relaxed) {
        return;
    }
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return;
    };
    // Collect first, so the builder lock is held only briefly.
    let mut files: Vec<(String, u64, u32)> = Vec::new();
    let mut subdirs: Vec<(String, PathBuf)> = Vec::new();
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        let name = e.file_name().to_string_lossy().into_owned();
        if ft.is_symlink() {
            continue;
        }
        let Ok(md) = e.metadata() else { continue };
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if md.file_attributes() & 0x400 != 0 {
                continue; // reparse point (junction, mount point, OneDrive placeholder…): avoid loops
            }
        }
        if ft.is_dir() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if sh.opts.one_filesystem && md.dev() != sh.root_dev {
                    continue;
                }
            }
            subdirs.push((name, e.path()));
        } else if ft.is_file() {
            let Some(size) = file_size(sh, &md) else {
                continue;
            };
            let mtime = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs().min(u32::MAX as u64) as u32);
            files.push((name, size, mtime));
        }
    }
    let ids: Vec<NodeId> = {
        let mut b = sh.builder.lock().unwrap();
        for (n, s, m) in &files {
            b.add_file(id, n, *s, *m);
        }
        subdirs.iter().map(|(n, _)| b.add_dir(id, n)).collect()
    };
    let p = &sh.progress;
    p.files.fetch_add(files.len() as u64, Relaxed);
    p.dirs.fetch_add(subdirs.len() as u64, Relaxed);
    p.bytes.fetch_add(files.iter().map(|f| f.1).sum(), Relaxed);
    for ((_, path), cid) in subdirs.into_iter().zip(ids) {
        scope.spawn(move |s| walk(s, sh, path, cid));
    }
}

#[cfg(unix)]
fn file_size(sh: &Shared, md: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    if md.nlink() > 1 && !sh.seen.lock().unwrap().insert((md.dev(), md.ino())) {
        return None; // hard link already counted
    }
    Some(if sh.opts.size_on_disk {
        md.blocks() * 512
    } else {
        md.len()
    })
}

#[cfg(windows)]
fn file_size(sh: &Shared, md: &std::fs::Metadata) -> Option<u64> {
    let len = md.len();
    Some(if sh.opts.size_on_disk && sh.cluster > 1 {
        len.div_ceil(sh.cluster) * sh.cluster
    } else {
        len
    })
}

#[cfg(not(any(unix, windows)))]
fn file_size(_: &Shared, md: &std::fs::Metadata) -> Option<u64> {
    Some(md.len())
}

#[cfg(windows)]
fn cluster_size(root: &Path) -> u64 {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceW;
    let mut s: Vec<u16> = root.as_os_str().encode_wide().take(3).collect();
    s.push(0);
    let (mut spc, mut bps, mut a, mut b) = (0u32, 0u32, 0u32, 0u32);
    // SAFETY: valid NUL-terminated buffer and out pointers.
    let ok = unsafe { GetDiskFreeSpaceW(s.as_ptr(), &mut spc, &mut bps, &mut a, &mut b) };
    if ok != 0 {
        (spc as u64) * (bps as u64)
    } else {
        4096
    }
}

/// Total / free bytes of the volume holding `path`, if known.
pub fn disk_space(path: &Path) -> Option<(u64, u64)> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let mut s: Vec<u16> = path.as_os_str().encode_wide().collect();
        s.push(0);
        let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
        // SAFETY: valid NUL-terminated buffer and out pointers.
        let ok = unsafe { GetDiskFreeSpaceExW(s.as_ptr(), &mut avail, &mut total, &mut free) };
        return if ok != 0 { Some((total, avail)) } else { None };
    }
    #[cfg(unix)]
    {
        // statvfs through libc-free path: parse via `df`-less approach is not possible in std; use a tiny extern.
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        #[repr(C)]
        struct StatVfs {
            f_bsize: u64,
            f_frsize: u64,
            f_blocks: u64,
            f_bfree: u64,
            f_bavail: u64,
            _rest: [u64; 12],
        }
        extern "C" {
            fn statvfs(path: *const std::ffi::c_char, buf: *mut StatVfs) -> i32;
        }
        let c = CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut buf = std::mem::MaybeUninit::<StatVfs>::zeroed();
        // SAFETY: buffer is larger than any platform's struct statvfs prefix we read (x86_64/aarch64 glibc & musl layouts agree on first fields).
        if unsafe { statvfs(c.as_ptr(), buf.as_mut_ptr()) } == 0 {
            let b = unsafe { buf.assume_init() };
            return Some((b.f_blocks * b.f_frsize, b.f_bavail * b.f_frsize));
        }
        return None;
    }
    #[allow(unreachable_code)]
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scans_cwd() {
        let p = Arc::new(Progress::default());
        let t = scan(Path::new("."), ScanOptions::default(), p.clone());
        assert!(t.len() > 1);
        assert_eq!(t.total_size(), p.bytes.load(Relaxed));
    }
}
