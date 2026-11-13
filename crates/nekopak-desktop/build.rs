fn main() {
    let config = slint_build::CompilerConfiguration::new().with_style("fluent".into());
    slint_build::compile_with_config("ui/app.slint", config).expect("Slint UI failed to compile");
    #[cfg(windows)]
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/icon.ico");
        resource.set("ProductName", "NekoPak");
        resource.set("FileDescription", "NekoPak");
        resource.set("LegalCopyright", "MIT License");
        if let Err(error) = resource.compile() {
            println!("cargo:warning=Windows resources were not embedded: {error}");
        }
    }
}
