# xc-ocr-onnx

基于 Rust + ONNX Runtime 的本地 OCR 引擎，使用 PaddleOCR 模型进行文字检测与识别。以 Tauri 2 构建桌面 GUI，支持图片、DOCX、PDF 文档。

## Rust 后端架构

项目由三个 Rust crate 组成：

### `PaddleOCR-rs` — OCR 引擎核心（独立仓库）

OCR 引擎已拆分为独立仓库 [Craun718/PaddleOCR-rs](https://github.com/Craun718/PaddleOCR-rs)，通过 git submodule 引入。

包含文本检测（DBNet）、文本识别（CRNN）、CTC 解码、文档方向分类（PP-LCNet）模块。

### `crates/docx-to-image` — 文档渲染

```
crates/docx-to-image/src/
├── lib.rs
├── renderer.rs  # 调用 LibreOffice 将 DOCX/PDF 渲染为 PNG
└── error.rs
```

通过系统 LibreOffice（或捆绑的便携版）将 DOCX/PDF 的每一页转为 RGBA 图像，供 OCR 引擎处理。

### `src-tauri/src/lib.rs` — Tauri 命令层

```
src-tauri/src/
├── lib.rs   # 7 条 Tauri 命令 + 模型/状态管理
└── main.rs  # 桌面入口
```

- `recognize_image`：加载图片 → 方向分类自动矫正 → det → rec → 排序 → 返回结果
- `list_models` / `switch_model`：扫描 `models/ocr/` 目录，运行时动态切换检测+识别模型
- `render_docx` / `render_pdf_page` / `pdf_page_count`：文档渲染接口
- `read_file_as_data_url`：文件读取

模型通过 `tauri::State` 管理，启动时默认加载 `v4` 变体。

## PaddleOCR ONNX 模型

模型来自 [PaddleOCR](https://github.com/PaddlePaddle/PaddleOCR) 预训练模型，经 [paddle2onnx](https://github.com/PaddlePaddle/paddle2onnx) 导出，由 [ort](https://github.com/pykeio/ort) crate 在内存中加载并执行 ONNX Runtime 推理。

### 管线

| 阶段 | 模型 | 输入 | 输出 |
|------|------|------|------|
| 方向分类 | PP-LCNet_x1_0_doc_ori | 3×224×224 RGB | 4 类 logits |
| 文本检测 | DBNet (`det.onnx`) | 3×H×W RGB (归一化: mean=0.485/0.456/0.406, std=0.229/0.224/0.225) | 概率图 + 阈值图 |
| 文本识别 | CRNN (`rec.onnx`) | 3×48×W RGB | T×C logits (T 时序步, C = 字符数+1 blank) |
| 字符集 | `keys.txt` | — | 每行一个字符，索引与模型输出对齐 |

### 模型变体

```
src-tauri/models/ocr/
├── v4/
│   ├── mobile/     # PP-OCRv4 轻量版 (日常使用)
│   └── server/     # PP-OCRv4 高精度版
└── v6/
    ├── medium/     # PP-OCRv6 均衡版
    └── small/      # PP-OCRv6 轻量版
```

启动默认 `v4`，可在界面下拉菜单动态切换。

### 获取模型

预置模型随仓库提供。自定义模型流程：

1. 从 [PaddleOCR 模型库](https://github.com/PaddlePaddle/PaddleOCR/blob/release/2.8/doc/doc_ch/models_list.md) 下载
2. 导出 ONNX：`paddle2onnx --model_dir ./model --model_filename inference.pdmodel --params_filename inference.pdiparams --save_file model.onnx --opset_version 11`
3. 将 `det.onnx`、`rec.onnx`、`keys.txt` 放入对应版本目录

## 快速开始

```bash
# 前置依赖: Rust, Node.js >= 18, pnpm, LibreOffice
pnpm install
pnpm tauri dev    # 开发
pnpm tauri build  # 构建
```

## 鸣谢

- [Tauri](https://tauri.app/) — 提供跨平台桌面框架
- [PaddleOCR](https://github.com/PaddlePaddle/PaddleOCR) — 提供文本检测与识别模型
- [MaaFramework](https://github.com/MaaAssistantArknights/MaaFramework) — 参考了基于ORT的OCR实现

## 许可

MIT
