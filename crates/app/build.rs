fn main() {
    println!("cargo:rerun-if-changed=../../assets/icon/formatcraft.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icon/formatcraft.ico");
        if let Err(e) = res.compile() {
            // A missing resource compiler only costs the exe icon.
            println!("cargo:warning=could not embed the Windows icon: {e}");
        }
    }
}
