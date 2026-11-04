use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-check-cfg=cfg(have_avdevice)");

    // The runtime package ships libavdevice.so.<soname> without the development
    // libavdevice.so symlink, so link the versioned name directly when present.
    let Ok(output) = Command::new("ldconfig").arg("-p").output() else {
        return;
    };
    let listing = String::from_utf8_lossy(&output.stdout);
    for line in listing.lines() {
        let Some(name) = line.split_whitespace().next() else {
            continue;
        };
        if name.starts_with("libavdevice.so.") {
            println!("cargo:rustc-link-lib=dylib:+verbatim={name}");
            println!("cargo:rustc-cfg=have_avdevice");
            return;
        }
    }
}
