fn main() {
    // Check bundled tools exist for the target platform
    let target = std::env::var("TARGET").unwrap_or_default();
    let triple = target_to_triple_dir(&target);

    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let tools_dir = manifest_dir.join("tools").join(&triple);
    let has_pandoc = tools_dir.join("pandoc").exists() || tools_dir.join("pandoc.exe").exists();
    let has_wkhtml = tools_dir.join("wkhtmltoimage").exists() || tools_dir.join("wkhtmltoimage.exe").exists();

    if !has_pandoc || !has_wkhtml {
        // Try auto-download
        let repo_root = manifest_dir.parent().unwrap_or(manifest_dir);
        let download_script = repo_root.join("scripts").join("download-tools.ps1");

        if download_script.exists() {
            println!("cargo:warning=");
            println!("cargo:warning=Bundled tools not found, attempting auto-download...");

            let result = std::process::Command::new("powershell")
                .arg("-ExecutionPolicy")
                .arg("Bypass")
                .arg("-File")
                .arg(&download_script)
                .arg("-TargetDir")
                .arg(&tools_dir)
                .arg("-Platform")
                .arg(&triple)
                .status();

            match result {
                Ok(status) if status.success() => {
                    println!("cargo:warning=Tools downloaded successfully to {}", tools_dir.display());
                    println!("cargo:warning=");
                }
                Ok(status) => {
                    println!("cargo:warning=Download script exited with code {}", status.code().unwrap_or(-1));
                    print_manual_instructions(&target, &tools_dir);
                }
                Err(e) => {
                    println!("cargo:warning=Failed to run download script: {e}");
                    print_manual_instructions(&target, &tools_dir);
                }
            }
        } else {
            print_manual_instructions(&target, &tools_dir);
        }
    }

    tauri_build::build()
}

fn print_manual_instructions(target: &str, tools_dir: &std::path::Path) {
    println!("cargo:warning=");
    println!("cargo:warning=Auto-download failed. Please place tools manually:");
    println!("cargo:warning=  Target: {target}");
    println!("cargo:warning=  Directory: {}", tools_dir.display());
    println!("cargo:warning=  Needed: pandoc{} and wkhtmltoimage{}",
        if cfg!(windows) { ".exe" } else { "" },
        if cfg!(windows) { ".exe" } else { "" });
    println!("cargo:warning=");
    println!("cargo:warning=  Windows: Run scripts/download-tools.ps1");
    println!("cargo:warning=  Linux:   sudo apt install pandoc wkhtmltopdf ghostscript");
    println!("cargo:warning=  macOS:   brew install pandoc wkhtmltopdf ghostscript");
    println!("cargo:warning=");
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
