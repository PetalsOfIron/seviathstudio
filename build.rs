// SPDX-License-Identifier: GPL-3.0-or-later
// Puts the snake icon into the Windows .exe (Explorer, desktop shortcuts, taskbar).
// The icon file is made from the logo by `cargo test --release write_app_icon`.

fn main() {
    println!("cargo:rerun-if-changed=assets/seviath.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") && std::path::Path::new("assets/seviath.ico").exists() {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/seviath.ico");
        res.set("ProductName", "SeviathStudio");
        res.set("FileDescription", "SeviathStudio — draw, rig, animate and stream");
        if let Err(e) = res.compile() {
            // never block a build over the icon
            println!("cargo:warning=App icon not embedded: {e}");
        }
    }
}
