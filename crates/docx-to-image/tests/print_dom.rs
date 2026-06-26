fn read_test_docx() -> (std::path::PathBuf, Vec<u8>) {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let test_file = manifest_dir
        .join("..")
        .join("..")
        .join("test")
        .join("仓库数据调整.docx");
    let bytes = std::fs::read(&test_file).expect("读取测试文件失败");
    (test_file, bytes)
}

fn read_test_docx2() -> (std::path::PathBuf, Vec<u8>) {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let test_file = manifest_dir
        .join("..")
        .join("..")
        .join("test")
        .join("测试样板.docx");
    let bytes = std::fs::read(&test_file).expect("读取测试文件失败");
    (test_file, bytes)
}

/// 创建一个预配置 soffice Command，保证在 Windows 上完全无窗口运行。
/// 必须使用 `.output()` 或 `.status()` 执行。
fn soffice_cmd() -> std::process::Command {
    let mut cmd = std::process::Command::new("soffice");
    cmd.arg("--headless")
        .arg("--norestore")
        .env("SAL_USE_VCLPLUGIN", "svp");
    cmd
}

/// 检测 soffice 是否可用 —— 用 `where` 查找，完全不启动 soffice 进程。
fn has_soffice() -> bool {
    find_soffice_path().is_some()
}

/// 查找 soffice 的路径，不启动 soffice 进程。
fn find_soffice_path() -> Option<std::path::PathBuf> {
    // 1. 用 Windows `where` 命令 / Unix `which` 查找
    #[cfg(windows)]
    let output = std::process::Command::new("where")
        .arg("soffice")
        .output()
        .ok();
    #[cfg(not(windows))]
    let output = std::process::Command::new("which")
        .arg("soffice")
        .output()
        .ok();

    if let Some(out) = output {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let first_line = stdout.lines().next().unwrap_or("").trim();
            if !first_line.is_empty() {
                let p = std::path::PathBuf::from(first_line);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }

    // 2. 检查常见安装路径
    #[cfg(windows)]
    {
        let candidates = [
            r"C:\Program Files\LibreOffice\program\soffice.exe",
            r"C:\Program Files (x86)\LibreOffice\program\soffice.exe",
        ];
        for c in candidates {
            let p = std::path::PathBuf::from(c);
            if p.is_file() {
                return Some(p);
            }
        }
    }

    None
}

#[test]
fn print_dom() {
    let (test_file, docx_bytes) = read_test_docx();

    println!("文件: {}", test_file.canonicalize().unwrap().display());
    println!("大小: {} bytes", docx_bytes.len());
    println!("自定义渲染器已移除，现在只走 pandoc 转换路径");

    let renderer = docx_to_image::DocxRenderer::new();
    let page_info = renderer.page_info(&docx_bytes);
    println!("--- 页面尺寸 ---");
    println!("{:#?}", page_info);
}

#[test]
fn print_soffice_html() {
    if !has_soffice() {
        println!("[soffice] LibreOffice (soffice) 未安装，跳过测试");
        return;
    }

    let (test_file, _docx_bytes) = read_test_docx();

    let tmp = tempfile::TempDir::new().expect("创建临时目录失败");
    let docx_path = tmp.path().join("仓库数据调整.docx");
    std::fs::copy(&test_file, &docx_path).expect("复制测试文件失败");

    let output = soffice_cmd()
        .arg("--convert-to")
        .arg("html:HTML")
        .arg("--outdir")
        .arg(tmp.path())
        .arg(&docx_path)
        .output()
        .expect("soffice 执行失败");

    assert!(
        output.status.success(),
        "soffice 转换失败: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let html_path = tmp.path().join("仓库数据调整.html");
    let html = std::fs::read_to_string(&html_path).expect("读取生成的 HTML 文件失败");

    println!("--- soffice HTML ---");
    println!("{}", html);
}

// ─── 测试样板.docx 纸张朝向提取 ───────────────────────────────

/// 方法1: 从 DOCX 内部 XML 提取纸张朝向（通过 docx_rs 解析 section_property）
#[test]
fn extract_orientation_from_xml() {
    let (test_file, docx_bytes) = read_test_docx2();

    println!("文件: {}", test_file.canonicalize().unwrap().display());
    println!("大小: {} bytes", docx_bytes.len());

    let renderer = docx_to_image::DocxRenderer::new();
    let page_info = renderer.page_info(&docx_bytes);

    println!("--- 方法1: DOCX XML 解析 ---");
    println!("纸张朝向: {:?}", page_info.orientation);
    println!(
        "页面尺寸: {}x{} TWIP ({}x{} px)",
        page_info.width_twip, page_info.height_twip, page_info.width_px, page_info.height_px,
    );
}

/// 方法2: 通过 soffice 转 HTML，从 @page CSS 中提取纸张朝向
#[test]
fn extract_orientation_from_soffice_html() {
    let (test_file, _docx_bytes) = read_test_docx2();

    println!("文件: {}", test_file.canonicalize().unwrap().display());

    if !has_soffice() {
        println!("[soffice] LibreOffice (soffice) 未安装，跳过");
        return;
    }

    let tmp = tempfile::TempDir::new().expect("创建临时目录失败");
    let docx_path = tmp.path().join("测试样板.docx");
    std::fs::copy(&test_file, &docx_path).expect("复制测试文件失败");

    let output = soffice_cmd()
        .arg("--convert-to")
        .arg("html:HTML")
        .arg("--outdir")
        .arg(tmp.path())
        .arg(&docx_path)
        .output()
        .expect("soffice 执行失败");

    assert!(
        output.status.success(),
        "soffice 转换失败: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let html_path = tmp.path().join("测试样板.html");
    let html = std::fs::read_to_string(&html_path).expect("读取生成的 HTML 文件失败");

    println!("--- 方法2: soffice HTML 解析 ---");

    // 从 HTML 的 @page CSS 中提取朝向和尺寸
    if let Some(orientation) = extract_orientation_from_html(&html) {
        println!("纸张朝向: {:?}", orientation);
    } else {
        println!("纸张朝向: 无法从 HTML 中提取（可能不含 @page 规则）");
    }
}

/// 从 soffice 生成的 HTML 中解析 @page { size: ... } 获取朝向
fn extract_orientation_from_html(html: &str) -> Option<docx_to_image::PageOrientation> {
    // soffice 生成的 HTML 中常有: @page { size: 21cm 29.7cm; margin: 2cm }
    // 或: @page { size: landscape; }
    // 或通过 width/height 判断

    // 先尝试匹配 @page { ... size: landscape ... }
    for line in html.lines() {
        let trimmed = line.trim();
        if trimmed.contains("@page") {
            println!("  [CSS] {}", trimmed);
        }
    }

    // 尝试从 @page 规则中提取 size
    let page_re = regex::Regex::new(r"@page\s*\{[^}]*size\s*:\s*([^;}{]+)").ok()?;
    if let Some(caps) = page_re.captures(html) {
        let size_value = caps.get(1)?.as_str().trim().to_lowercase();
        println!("  @page size: {}", size_value);
        if size_value.contains("landscape") {
            return Some(docx_to_image::PageOrientation::Landscape);
        }
        if size_value.contains("portrait") {
            return Some(docx_to_image::PageOrientation::Portrait);
        }
        // 比如 "21cm 29.7cm" → 宽 < 高 = portrait
        let dims: Vec<f64> = size_value
            .split_whitespace()
            .filter_map(|s| {
                let s = s.trim_end_matches(';').trim();
                if s.ends_with("cm") {
                    s.trim_end_matches("cm").parse::<f64>().ok()
                } else if s.ends_with("mm") {
                    s.trim_end_matches("mm")
                        .parse::<f64>()
                        .ok()
                        .map(|v| v / 10.0)
                } else if s.ends_with("in") {
                    s.trim_end_matches("in")
                        .parse::<f64>()
                        .ok()
                        .map(|v| v * 2.54)
                } else {
                    s.parse::<f64>().ok()
                }
            })
            .collect();
        if dims.len() >= 2 {
            return Some(if dims[0] > dims[1] {
                docx_to_image::PageOrientation::Landscape
            } else {
                docx_to_image::PageOrientation::Portrait
            });
        }
    }

    None
}

/// 创建一个预配置的 DocxRenderer，自动搜索项目 tools/ 目录
fn create_renderer_with_tools() -> docx_to_image::DocxRenderer {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let tools_dir = manifest_dir.join("..").join("..").join("tools").join("windows-x86_64");
    let mut renderer = docx_to_image::DocxRenderer::new();
    if tools_dir.is_dir() {
        renderer = renderer.add_tool_dir(&tools_dir);
    }
    renderer
}

/// 通用渲染方向验证：渲染 docx，验证 page_info 方向与输出图片方向一致
fn assert_render_orientation(
    label: &str,
    docx_bytes: &[u8],
    expected: docx_to_image::PageOrientation,
) {
    let renderer = create_renderer_with_tools();
    let page_info = renderer.page_info(docx_bytes);

    println!(
        "[{}] DOCX 元数据: {}x{} TWIP, 方向: {:?}",
        label, page_info.width_twip, page_info.height_twip, page_info.orientation,
    );

    // 1. 验证元数据方向
    assert_eq!(
        page_info.orientation, expected,
        "[{}] DOCX 元数据方向应为 {:?}，实际为 {:?}",
        label, expected, page_info.orientation,
    );

    let is_landscape = matches!(expected, docx_to_image::PageOrientation::Landscape);
    println!(
        "[{}] 期望输出为{}, CSS viewport: {}x{} px",
        label,
        if is_landscape { "横向" } else { "纵向" },
        page_info.width_twip / 15,
        page_info.height_twip / 15,
    );

    // 2. 渲染并验证输出图片方向
    match renderer.render(docx_bytes) {
        Ok(pages) => {
            assert!(!pages.is_empty(), "渲染结果不应为空");
            let img = &pages[0];
            let w = img.width();
            let h = img.height();
            let cmp = if w > h { ">" } else if w < h { "<" } else { "=" };
            println!("[{}] 渲染输出: {}x{} px, width {} height", label, w, h, cmp);

            if is_landscape {
                assert!(w > h,
                    "[{}] 横向 DOCX 输出图片应为 width > height，实际: {}x{} px", label, w, h);
            } else {
                assert!(h > w,
                    "[{}] 纵向 DOCX 输出图片应为 height > width，实际: {}x{} px", label, w, h);
            }
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("没有找到可用的转换工具") {
                println!("[工具] 渲染工具不可用，跳过实际渲染验证: {}", msg);
                return;
            }
            panic!("[{}] 渲染失败: {}", label, e);
        }
    }
}

/// 仓库数据调整.docx — 纯纵向 A4，无表格，走自定义渲染器
#[test]
fn render_output_portrait() {
    let (test_file, docx_bytes) = read_test_docx();
    println!("文件: {}", test_file.canonicalize().unwrap().display());
    println!("大小: {} bytes", docx_bytes.len());
    assert_render_orientation("仓库数据调整", &docx_bytes, docx_to_image::PageOrientation::Portrait);
}

/// 测试样板.docx — 第一页横向，含表格/图片，走 pandoc 回退路径
#[test]
fn render_output_landscape() {
    let (test_file, docx_bytes) = read_test_docx2();
    println!("文件: {}", test_file.canonicalize().unwrap().display());
    println!("大小: {} bytes", docx_bytes.len());
    assert_render_orientation("测试样板", &docx_bytes, docx_to_image::PageOrientation::Landscape);
}
