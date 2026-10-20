fn main() {
    println!("cargo:rerun-if-changed=assets/glint.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if !std::path::Path::new("assets/glint.ico").exists() {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/glint.ico");
    res.set("FileDescription", "Glint image viewer");
    res.set("ProductName", "Glint");
    // A missing resource compiler shouldn't block a build; the exe just lacks an icon.
    if let Err(e) = res.compile() {
        println!("cargo:warning=could not embed icon: {e}");
    }
}
