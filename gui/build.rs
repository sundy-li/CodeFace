use std::env;

fn main() {
    let icon = "../resources/app-icon/CodeFace.ico";
    println!("cargo:rerun-if-changed={icon}");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon(icon)
            .compile()
            .expect("failed to embed the CodeFace Windows icon");
    }
}
