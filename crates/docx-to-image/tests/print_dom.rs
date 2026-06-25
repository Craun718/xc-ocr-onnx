#[test]
fn print_dom() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let test_file = manifest_dir
        .join("..").join("..").join("test").join("仓库数据调整.docx");

    let docx_bytes = std::fs::read(&test_file)
        .expect("读取测试文件失败");

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
