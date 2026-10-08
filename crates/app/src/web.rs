//! Browser back-end: File System Access API (Chromium), `<input webkitdirectory>` fallback and drag & drop.

use crate::platform::{Job, Loaded};
use eframe::egui;
use js_sys::{Array, Function, Promise, Reflect};
use spacehunter_core::{Progress, TreeBuilder};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{future_to_promise, spawn_local, JsFuture};

pub const CAN_SCAN_PATHS: bool = false;

fn get(o: &JsValue, k: &str) -> JsValue {
    Reflect::get(o, &JsValue::from_str(k)).unwrap_or(JsValue::UNDEFINED)
}
fn call0(o: &JsValue, k: &str) -> Result<JsValue, JsValue> {
    let f: Function = get(o, k).dyn_into()?;
    f.call0(o)
}

struct Ctx {
    b: RefCell<TreeBuilder>,
    progress: Arc<Progress>,
    egui: egui::Context,
}

/// Recursively read a directory handle. Subdirectories are processed concurrently.
fn walk(cx: Rc<Ctx>, handle: JsValue, parent: u32) -> Promise {
    future_to_promise(async move {
        let iter = call0(&handle, "values")?;
        let mut file_promises: Vec<(String, Promise)> = Vec::new();
        let mut sub: Vec<Promise> = Vec::new();
        loop {
            if cx.progress.cancel.load(Relaxed) {
                break;
            }
            let next: Promise = call0(&iter, "next")?.dyn_into()?;
            let r = JsFuture::from(next).await?;
            if get(&r, "done").as_bool().unwrap_or(true) {
                break;
            }
            let entry = get(&r, "value");
            let name = get(&entry, "name").as_string().unwrap_or_default();
            match get(&entry, "kind").as_string().as_deref() {
                Some("file") => {
                    if let Ok(p) = call0(&entry, "getFile").and_then(|p| p.dyn_into::<Promise>()) {
                        file_promises.push((name, p));
                    }
                }
                Some("directory") => {
                    let id = cx.b.borrow_mut().add_dir(parent, &name);
                    cx.progress.dirs.fetch_add(1, Relaxed);
                    sub.push(walk(cx.clone(), entry, id));
                }
                _ => {}
            }
        }
        let swallow = Closure::<dyn FnMut(JsValue)>::new(|_| {});
        let files = Promise::all(&file_promises.iter().map(|(_, p)| p.catch(&swallow)).collect::<Array>());
        swallow.forget();
        let arr: Array = JsFuture::from(files).await?.into();
        let mut bytes = 0u64;
        {
            let mut b = cx.b.borrow_mut();
            for (i, (name, _)) in file_promises.iter().enumerate() {
                let f = arr.get(i as u32);
                if f.is_null() || f.is_undefined() {
                    continue;
                }
                let size = get(&f, "size").as_f64().unwrap_or(0.0) as u64;
                let m = (get(&f, "lastModified").as_f64().unwrap_or(0.0) / 1000.0) as u32;
                b.add_file(parent, name, size, m);
                bytes += size;
            }
        }
        cx.progress.files.fetch_add(file_promises.len() as u64, Relaxed);
        cx.progress.bytes.fetch_add(bytes, Relaxed);
        cx.egui.request_repaint();
        let _ = JsFuture::from(Promise::all(&sub.into_iter().collect::<Array>())).await?;
        Ok(JsValue::UNDEFINED)
    })
}

fn finish(cx: Rc<Ctx>, inbox: Arc<std::sync::Mutex<Option<Loaded>>>) {
    // All clones are dropped once the walk promises resolve; take the builder out.
    let b = std::mem::replace(&mut *cx.b.borrow_mut(), TreeBuilder::new(""));
    *inbox.lock().unwrap() = Some(Loaded { tree: b.finish(), path: None, disk: None });
    cx.progress.done.store(true, Relaxed);
    cx.egui.request_repaint();
}

fn fail(job: &Arc<Progress>, msg: &str, ctx: &egui::Context) {
    *job.error.lock().unwrap() = Some(msg.to_owned());
    job.done.store(true, Relaxed);
    ctx.request_repaint();
}

fn start_handle(ctx: &egui::Context, handle: JsValue, job: &Job) {
    let name = get(&handle, "name").as_string().unwrap_or_else(|| "folder".into());
    let cx = Rc::new(Ctx { b: RefCell::new(TreeBuilder::new(&name)), progress: job.progress.clone(), egui: ctx.clone() });
    let (inbox, prog, ctx) = (job.inbox.clone(), job.progress.clone(), ctx.clone());
    spawn_local(async move {
        match JsFuture::from(walk(cx.clone(), handle, 0)).await {
            Ok(_) => finish(cx, inbox),
            Err(e) => fail(&prog, &format!("{e:?}"), &ctx),
        }
    });
}

/// Ask the user for a folder. Uses the File System Access API when present.
pub fn start_open(ctx: &egui::Context) -> Option<Job> {
    let job = Job::new("folder");
    let win = web_sys::window()?;
    if Reflect::has(&win, &JsValue::from_str("showDirectoryPicker")).unwrap_or(false) {
        let (ctx2, j2) = (ctx.clone(), Job { progress: job.progress.clone(), inbox: job.inbox.clone(), label: String::new() });
        spawn_local(async move {
            let f: Function = get(&web_sys::window().unwrap(), "showDirectoryPicker").unchecked_into();
            let p = f.call0(&web_sys::window().unwrap()).and_then(|p| p.dyn_into::<Promise>());
            match p {
                Ok(p) => match JsFuture::from(p).await {
                    Ok(h) => start_handle(&ctx2, h, &j2),
                    Err(_) => fail(&j2.progress, "cancelled", &ctx2),
                },
                Err(_) => fail(&j2.progress, "picker unavailable", &ctx2),
            }
        });
    } else {
        input_fallback(ctx, &job);
    }
    Some(job)
}

/// `<input type=file webkitdirectory>`: works in Firefox/Safari, gives sizes synchronously.
fn input_fallback(ctx: &egui::Context, job: &Job) {
    let doc = web_sys::window().unwrap().document().unwrap();
    let input: web_sys::HtmlInputElement = doc.create_element("input").unwrap().unchecked_into();
    input.set_type("file");
    let _ = input.set_attribute("webkitdirectory", "");
    let _ = input.set_attribute("multiple", "");
    let (inbox, prog, ctx) = (job.inbox.clone(), job.progress.clone(), ctx.clone());
    let inp2 = input.clone();
    let cb = Closure::once_into_js(move || {
        let Some(files) = inp2.files() else { return fail(&prog, "no files", &ctx) };
        if files.length() == 0 {
            return fail(&prog, "cancelled", &ctx);
        }
        let first = files.get(0).unwrap();
        let rel = get(&first, "webkitRelativePath").as_string().unwrap_or_default();
        let root = rel.split('/').next().unwrap_or("folder").to_owned();
        let mut b = TreeBuilder::new(&root);
        let mut dirs: HashMap<String, u32> = HashMap::new();
        for i in 0..files.length() {
            let f = files.get(i).unwrap();
            let rel = get(&f, "webkitRelativePath").as_string().unwrap_or_default();
            let rest = rel.split_once('/').map_or(rel.as_str(), |x| x.1);
            let size = f.size() as u64;
            b.add_path(&mut dirs, rest, size, (f.last_modified() / 1000.0) as u32);
            prog.bytes.fetch_add(size, Relaxed);
        }
        prog.files.store(files.length() as u64, Relaxed);
        *inbox.lock().unwrap() = Some(Loaded { tree: b.finish(), path: None, disk: None });
        prog.done.store(true, Relaxed);
        ctx.request_repaint();
    });
    let _ = input.add_event_listener_with_callback("change", cb.unchecked_ref());
    input.click();
}

/// Accept a folder dropped onto the page (Chromium: `getAsFileSystemHandle`).
pub fn install_drop(ctx: &egui::Context, slot: Rc<RefCell<Option<Job>>>) {
    let win = web_sys::window().unwrap();
    let over = Closure::<dyn FnMut(web_sys::DragEvent)>::new(|e: web_sys::DragEvent| e.prevent_default());
    let _ = win.add_event_listener_with_callback("dragover", over.as_ref().unchecked_ref());
    over.forget();
    let ctx = ctx.clone();
    let drop = Closure::<dyn FnMut(web_sys::DragEvent)>::new(move |e: web_sys::DragEvent| {
        e.prevent_default();
        let Some(items) = e.data_transfer().map(|d| d.items()) else { return };
        if items.length() == 0 {
            return;
        }
        let item: JsValue = items.get(0).unwrap().into();
        let Ok(f) = get(&item, "getAsFileSystemHandle").dyn_into::<Function>() else { return };
        let Ok(p) = f.call0(&item).and_then(|p| p.dyn_into::<Promise>()) else { return };
        let job = Job::new("dropped folder");
        let (ctx2, j2) = (ctx.clone(), Job { progress: job.progress.clone(), inbox: job.inbox.clone(), label: String::new() });
        *slot.borrow_mut() = Some(job);
        spawn_local(async move {
            match JsFuture::from(p).await {
                Ok(h) if get(&h, "kind").as_string().as_deref() == Some("directory") => start_handle(&ctx2, h, &j2),
                _ => fail(&j2.progress, "please drop a folder", &ctx2),
            }
        });
    });
    let _ = win.add_event_listener_with_callback("drop", drop.as_ref().unchecked_ref());
    drop.forget();
}
