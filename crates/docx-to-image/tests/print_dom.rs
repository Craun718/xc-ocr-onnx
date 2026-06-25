fn read_test_docx() -> (std::path::PathBuf, Vec<u8>) {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let test_file = manifest_dir
        .join("..").join("..").join("test").join("仓库数据调整.docx");
    let bytes = std::fs::read(&test_file).expect("读取测试文件失败");
    (test_file, bytes)
}

#[test]
fn print_dom() {
    let (test_file, docx_bytes) = read_test_docx();

    println!("文件: {}", test_file.canonicalize().unwrap().display());
    println!("大小: {} bytes", docx_bytes.len());

    let html = docx_to_image::render_docx_html(&docx_bytes);
    match &html {
        Some(h) => {
            println!("--- HTML ---");
            println!("{}", h);
        }
        None => {
            println!("纯 Rust HTML 生成返回 None（文档含表格/绘图等），尝试 pandoc 回退...");
        }
    }

    let renderer = docx_to_image::DocxRenderer::new();
    let page_info = renderer.page_info(&docx_bytes);
    println!("--- 页面尺寸 ---");
    println!("{:#?}", page_info);
}

#[test]
fn print_soffice_html() {
    let has_soffice = std::process::Command::new("soffice")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !has_soffice {
        println!("[soffice] LibreOffice (soffice) 未安装，跳过测试");
        return;
    }

    let (test_file, _docx_bytes) = read_test_docx();

    let tmp = tempfile::TempDir::new().expect("创建临时目录失败");
    let docx_path = tmp.path().join("仓库数据调整.docx");
    std::fs::copy(&test_file, &docx_path).expect("复制测试文件失败");

    let output = std::process::Command::new("soffice")
        .arg("--headless")
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
    let html = std::fs::read_to_string(&html_path)
        .expect("读取生成的 HTML 文件失败");

    println!("--- soffice HTML ---");
    println!("{}", html);
}
