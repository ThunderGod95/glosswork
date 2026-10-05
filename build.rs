fn main() {
    println!("cargo:rerun-if-changed=assets/windows/glosswork.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut res = winresource::WindowsResource::new();

    res.set_icon("assets/windows/glosswork.ico");

    res.compile().expect("failed to compile Windows resources");
}