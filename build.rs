fn main() {
    println!("cargo:rerun-if-changed=assets/scopework.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/scopework.ico")
            .set("ProductName", "Scopework")
            .set("FileDescription", "Scopework Research Workbench")
            .set("CompanyName", "Scopework")
            .compile()
            .expect("compile Windows icon and version resources");
    }
}
