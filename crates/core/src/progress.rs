use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Mutex;

/// Shared live counters, readable from the UI thread while a scan/load runs.
#[derive(Default)]
pub struct Progress {
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub bytes: AtomicU64,
    pub cancel: AtomicBool,
    pub done: AtomicBool,
    pub error: Mutex<Option<String>>,
}
