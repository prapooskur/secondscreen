fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    println!("cargo:rerun-if-changed=src/viewer/webview.c");
    let webkit = pkg_config::Config::new()
        .cargo_metadata(false)
        .probe("webkit2gtk-4.1")
        .unwrap();
    pkg_config::probe_library("gio-2.0").unwrap();
    pkg_config::probe_library("cairo").unwrap();
    println!("cargo:rustc-link-lib=dl");
    cc::Build::new()
        .file("src/viewer/webview.c")
        .includes(webkit.include_paths)
        .compile("secondscreen_webview");
}
