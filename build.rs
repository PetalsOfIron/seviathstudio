// SPDX-License-Identifier: GPL-3.0-or-later
// Puts the snake icon into the Windows .exe (Explorer, desktop shortcuts, taskbar).
// The icon file is made from the logo by `cargo test --release write_app_icon`.

/// Today's date (UTC) as YYYY-MM-DD, without extra build dependencies.
fn build_date() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    // days since 1970-01-01 → civil date (Howard Hinnant's algorithm)
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

fn main() {
    println!("cargo:rustc-env=SEVIATH_BUILD_DATE={}", build_date());
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
