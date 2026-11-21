// Embeds the app icon (assets/Sordino.ico) into sordino.exe so Explorer, the taskbar and
// shortcuts show it. Only runs when building for Windows.
fn main() {
    println!("cargo:rerun-if-changed=../assets/Sordino.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../assets/Sordino.ico");
        res.compile().expect("failed to embed Windows icon resource");
    }
}
