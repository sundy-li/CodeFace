use serde_json::Value;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn rust_string(path: &Path) -> String {
    format!("{:?}", path.to_string_lossy())
}

fn generate_bundled_themes() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let themes_root = manifest_dir.join("../resources/theme-packs");
    println!("cargo:rerun-if-changed={}", themes_root.display());

    let mut directories = fs::read_dir(&themes_root)
        .expect("failed to read resources/theme-packs")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    directories.sort();

    let mut entries = Vec::new();
    for directory in directories {
        let folder_id = directory
            .file_name()
            .and_then(|name| name.to_str())
            .expect("theme folder name must be valid UTF-8");
        let json_path = directory.join("theme.json");
        let css_path = directory.join("codeface.css");
        let background_path = directory.join("background.png");
        for path in [&json_path, &css_path, &background_path] {
            assert!(
                path.is_file(),
                "bundled theme is missing {}",
                path.display()
            );
            println!("cargo:rerun-if-changed={}", path.display());
        }

        let manifest: Value = serde_json::from_str(
            &fs::read_to_string(&json_path).expect("failed to read bundled theme manifest"),
        )
        .expect("bundled theme manifest is not valid JSON");
        let id = manifest["id"]
            .as_str()
            .expect("bundled theme manifest must contain id");
        let name = manifest["name"]
            .as_str()
            .expect("bundled theme manifest must contain name");
        assert_eq!(
            id, folder_id,
            "bundled theme folder and manifest id must match"
        );
        if name.to_ascii_lowercase().starts_with("todo") {
            continue;
        }

        let avatar_path = directory.join("avatar.png");
        let avatar = if avatar_path.is_file() {
            println!("cargo:rerun-if-changed={}", avatar_path.display());
            format!("Some(include_bytes!({}))", rust_string(&avatar_path))
        } else {
            "None".into()
        };
        entries.push(format!(
            "BundledTheme {{ id: {id:?}, json: include_str!({json}), css: include_str!({css}), background: include_bytes!({background}), avatar: {avatar} }}",
            json = rust_string(&json_path),
            css = rust_string(&css_path),
            background = rust_string(&background_path),
        ));
    }

    assert!(!entries.is_empty(), "no bundled themes were discovered");
    let generated = format!(
        "const BUNDLED_THEMES: &[BundledTheme] = &[\n    {}\n];\n",
        entries.join(",\n    ")
    );
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("out dir")).join("bundled_themes.rs");
    fs::write(output, generated).expect("failed to write bundled theme registry");
}

fn main() {
    generate_bundled_themes();

    let icon = "../resources/app-icon/CodeFace.ico";
    println!("cargo:rerun-if-changed={icon}");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon(icon)
            .compile()
            .expect("failed to embed the CodeFace Windows icon");
    }
}
