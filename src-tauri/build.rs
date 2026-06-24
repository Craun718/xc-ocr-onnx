fn main() {
    // Check bundled tools exist for the target platform
    let target = std::env::var("TARGET").unwrap_or_default();
    let triple = target_to_triple_dir(&target);

    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let tools_dir = manifest_dir.join("tools").join(&triple);
    let has_pandoc = tools_dir.join("pandoc").exists() || tools_dir.join("pandoc.exe").exists();
    let has_wkhtml = tools_dir.join("wkhtmltoimage").exists() || tools_dir.join("wkhtmltoimage.exe").exists();

    if !has_pandoc || !has_wkhtml {
        println!("cargo:warning=");
        println!("cargo:warning=⚠  Bundled tools not found for target: {target}");
        println!("cargo:warning=   Expected directory: src-tauri/{:?}", tools_dir);
        println!("cargo:warning=   Needed: pandoc{} and wkhtmltoimage{}",
            if cfg!(windows) { ".exe" } else { "" },
            if cfg!(windows) { ".exe" } else { "" });
        println!("cargo:warning=");
        println!("cargo:warning=   Run the download script to fetch them:");
        println!("cargo:warning=     scripts/download-tools.ps1");
        println!("cargo:warning=");
        println!("cargo:warning=   Or manually place binaries in: src-tauri/{}", tools_dir.display());
        println!("cargo:warning=");
        println!("cargo:warning=   NOTE: The app will still compile, but DOCX conversion");
        println!("cargo:warning=   will fail at runtime without these tools.");
        println!("cargo:warning=");
    }

    tauri_build::build()
}

/// Convert a Rust target triple like `x86_64-pc-windows-msvc` to
/// our tool directory name like `windows-x86_64`.
fn target_to_triple_dir(target: &str) -> String {
    let parts: Vec<&str> = target.split('-').collect();
    let os = match parts.get(2) {
        Some(&"windows") => "windows",
        Some(&"linux") => "linux",
        Some(&"darwin") => "macos",
        _ => return "unknown".into(),
    };
    let arch = match parts.first() {
        Some(&"x86_64") | Some(&"amd64") => "x86_64",
        Some(&"aarch64") | Some(&"arm64") => "arm64",
        Some(&"i686") | Some(&"i386") => "x86",
        _ => "unknown",
    };
    format!("{os}-{arch}")
}
