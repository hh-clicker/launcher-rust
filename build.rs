// Build-only pin: Python packs the exact Noto font and retains its upstream notices.
use epaint_default_fonts as _;
fn main() {
    for path in [
        "payload",
        "assets",
        "Cargo.lock",
        "scripts/embedded-resources.py",
        "scripts/font_assets.py",
        "scripts/build_inputs.py",
        "scripts/legal_collection.py",
        "scripts/font_notices.py",
        "legal",
        "README.md",
        "LICENSE",
        "THIRD_PARTY_NOTICES.md",
        "vendor/epaint/PATCHES.md",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    println!("cargo:rerun-if-env-changed=CARGO_HOME");
    println!("cargo:rerun-if-env-changed=HH_RUST_DOCS_DIR");
    let archive = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("app.zip");
    let status = std::process::Command::new("python")
        .arg("scripts/embedded-resources.py")
        .arg(&archive)
        .status()
        .expect("Python 3 is required to package application resources");
    assert!(status.success(), "Embedded application packaging failed");
    let legal_inputs = std::fs::read_to_string(archive.with_file_name("legal-inputs.txt"))
        .expect("Legal input watch list");
    for path in legal_inputs.lines() {
        println!("cargo:rerun-if-changed={path}");
    }
    println!("cargo:rerun-if-changed=assets/hh-clicker.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/hh-clicker.ico");
        res.set("FileDescription", "HH Clicker Launcher — Rust portable");
        res.set("ProductName", "HH Clicker Launcher");
        let version = std::env::var("CARGO_PKG_VERSION").unwrap();
        res.set("FileVersion", &version);
        res.set("ProductVersion", &version);
        let parts: Vec<u16> = version
            .split('.')
            .map(|part| part.parse().unwrap())
            .collect();
        assert_eq!(parts.len(), 3, "Expected numeric X.Y.Z version");
        let packed =
            (u64::from(parts[0]) << 48) | (u64::from(parts[1]) << 32) | (u64::from(parts[2]) << 16);
        res.set_version_info(winresource::VersionInfo::FILEVERSION, packed);
        res.set_version_info(winresource::VersionInfo::PRODUCTVERSION, packed);
        res.compile().expect("Windows resources");
    }
}
