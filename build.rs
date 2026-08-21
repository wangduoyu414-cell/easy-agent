fn main() {
    println!("cargo:rerun-if-changed=assets/branding/easy-agent.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/branding/easy-agent.ico");
    resource.set("ProductName", "easy agent");
    resource.set("FileDescription", "easy agent");
    resource.set("InternalName", "easy-agent");
    resource.set("OriginalFilename", "easy-agent.exe");
    resource.set("LegalCopyright", "Copyright (c) easy agent contributors");
    resource
        .compile()
        .expect("failed to embed the easy agent Windows icon and version metadata");
}
