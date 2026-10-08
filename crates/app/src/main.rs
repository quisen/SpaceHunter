#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    use spacehunter::{Args, SpaceHunter};
    // Never fail silently (release builds have no console on Windows): log + dialog.
    std::panic::set_hook(Box::new(|info| {
        let msg = format!(
            "SpaceHunter crashed:\n{info}\n{}",
            std::backtrace::Backtrace::force_capture()
        );
        let _ = std::fs::write(std::env::temp_dir().join("spacehunter-crash.log"), &msg);
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("SpaceHunter crashed")
            .set_description(format!(
                "{}\n\nDetails: {}",
                info,
                std::env::temp_dir().join("spacehunter-crash.log").display()
            ))
            .show();
    }));
    let mut args = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--demo" => args.demo = true,
            "--3d" => args.mode3d = true,
            "--zoom" => args.zoom_first = true,
            "--screenshot" => args.screenshot = it.next().map(Into::into),
            "-h" | "--help" => {
                println!("SpaceHunter [PATH] [--demo] [--3d]\n  PATH      folder or drive to scan at start-up");
                return Ok(());
            }
            p => args.path = Some(p.into()),
        }
    }
    let run = |msaa: u16, args: Args| {
        let options = eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_inner_size([1280.0, 800.0])
                .with_min_inner_size([640.0, 420.0])
                .with_title("Space Hunter")
                .with_icon(
                    eframe::icon_data::from_png_bytes(include_bytes!(
                        "../../../web/icons/app-256.png"
                    ))
                    .expect("valid embedded app icon"),
                ),
            multisampling: msaa,
            depth_buffer: 24,
            ..Default::default()
        };
        eframe::run_native(
            "SpaceHunter",
            options,
            Box::new(move |cc| Ok(Box::new(SpaceHunter::new(cc, args)))),
        )
    };
    let a2 = Args {
        path: args.path.clone(),
        demo: args.demo,
        mode3d: args.mode3d,
        screenshot: args.screenshot.clone(),
        zoom_first: args.zoom_first,
    };
    // Some drivers (e.g. WSLg) expose no multisampled config: retry without MSAA.
    run(4, args).or_else(|_| run(0, a2))
}

#[cfg(target_arch = "wasm32")]
fn main() {}
