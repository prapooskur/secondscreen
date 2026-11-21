fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    println!("cargo:rerun-if-changed=src/viewer/webview.c");
    let webkit = pkg_config::Config::new().probe("webkit2gtk-4.1").unwrap();
    cc::Build::new()
        .file("src/viewer/webview.c")
        .includes(webkit.include_paths)
        .compile("secondscreen_webview");
}
