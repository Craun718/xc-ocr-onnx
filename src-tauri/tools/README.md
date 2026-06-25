# 转换工具

将对应平台的 CLI 工具放入此目录，软件启动时自动检测。

## 目录结构

```
tools/
├── windows-x86_64/       ← Windows x64
│   ├── pandoc.exe
│   ├── wkhtmltoimage.exe
│   └── gswin64c.exe
├── linux-x86_64/         ← Linux x64
│   ├── pandoc
│   ├── wkhtmltoimage
│   └── gs
├── linux-arm64/          ← Linux ARM64（如树莓派）
│   ├── pandoc
│   ├── wkhtmltoimage
│   └── gs
└── README.md
```


## 使用工具

下载以下工具放入对应目录：

| 工具 | 下载地址 | 作用 |
|---|---|---|
| Pandoc | https://pandoc.org/installing.html | DOCX → HTML |
| wkhtmltopdf | https://wkhtmltopdf.org/downloads.html | HTML → PDF/PNG |
| Ghostscript | https://ghostscript.com/releases/gsdnld.html | PDF → PNG |

运行 `scripts/download-tools.ps1` 可自动下载 Windows 版工具。
