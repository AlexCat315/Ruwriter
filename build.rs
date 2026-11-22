fn main() {
    #[cfg(windows)]
    {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
            return;
        }
        const ICON: &str = "Platforms/Windows/Resources/Ruwriter.ico";
        println!("cargo:rerun-if-changed={ICON}");
        winresource::WindowsResource::new()
            .set_icon(ICON)
            .compile()
            .expect("embed the Ruwriter executable icon");
    }
}
