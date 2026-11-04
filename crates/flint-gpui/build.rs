use std::env;
use std::fs;
use std::path::Path;

const LIBDIR: &str = "/usr/lib/x86_64-linux-gnu";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let dev_link = format!("{LIBDIR}/libxkbcommon-x11.so");
    if Path::new(&dev_link).exists() {
        return;
    }
    let runtime = format!("{LIBDIR}/libxkbcommon-x11.so.0");
    if !Path::new(&runtime).exists() {
        return;
    }
    let out = env::var("OUT_DIR").expect("OUT_DIR is set by cargo");
    let link = Path::new(&out).join("libxkbcommon-x11.so");
    let _ = fs::remove_file(&link);
    if std::os::unix::fs::symlink(&runtime, &link).is_ok() {
        println!("cargo:rustc-link-search=native={out}");
    }
}
