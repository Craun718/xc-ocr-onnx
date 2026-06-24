import { useState, useRef, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

interface OcrBlock {
  text: string;
  confidence: number;
  x: number;
  y: number;
  width: number;
  height: number;
}

interface PageImage {
  page: number;
  width: number;
  height: number;
  image_data: string;
}


function App() {
  const [imageDataUrl, setImageDataUrl] = useState<string | null>(null);
  const [blocks, setBlocks] = useState<OcrBlock[]>([]);
  const [loading, setLoading] = useState(false);
  const [fileType, setFileType] = useState<"image" | "docx" | null>(null);
  const [pages, setPages] = useState<PageImage[]>([]);
  const [currentPage, setCurrentPage] = useState(0);
  const [fileName, setFileName] = useState("");

  // model switching
  const [models, setModels] = useState<string[]>([]);
  const [currentModel, setCurrentModel] = useState("");

  const fileInputRef = useRef<HTMLInputElement>(null);
  const imgRef = useRef<HTMLImageElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rawBase64Ref = useRef<string>("");

  useEffect(() => {
    invoke<string[]>("list_models").then((list) => {
      setModels(list);
      if (list.length > 0) setCurrentModel(list[0]);
    }).catch(console.error);
  }, []);

  const handleModelChange = async (variant: string) => {
    setCurrentModel(variant);
    setBlocks([]);
    try {
      await invoke("switch_model", { variant });
    } catch (err) {
      alert("切换模型失败: " + err);
    }
  };

  const handleFileSelect = () => {
    fileInputRef.current?.click();
  };

  const handleFileChange = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;

    setFileName(file.name);
    setBlocks([]);
    setPages([]);
    setCurrentPage(0);
    setImageDataUrl(null);

    const reader = new FileReader();
    reader.onload = async (evt) => {
      const dataUrl = evt.target?.result as string;
      rawBase64Ref.current = dataUrl;

      if (file.name.endsWith(".docx")) {
        setFileType("docx");
        setLoading(true);
        try {
          const result: PageImage[] = await invoke("render_docx", { data: dataUrl });
          setPages(result);
          if (result.length > 0) {
            setImageDataUrl(result[0].image_data);
            setCurrentPage(0);
          }
        } catch (err) {
          alert("渲染 DOCX 失败: " + err);
        } finally {
          setLoading(false);
        }
      } else {
        setFileType("image");
        setImageDataUrl(dataUrl);
      }
    };
    reader.readAsDataURL(file);
  };

  const handleRecognize = async () => {
    if (!rawBase64Ref.current) return;
    setLoading(true);
    setBlocks([]);
    try {
      const inputData = fileType === "docx" && pages[currentPage]
        ? pages[currentPage].image_data
        : rawBase64Ref.current;

      const result: OcrBlock[] = await invoke("recognize_image", {
        data: inputData,
      });
      setBlocks(result);
    } catch (err) {
      alert("OCR 识别失败: " + err);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    const img = imgRef.current;
    const canvas = canvasRef.current;
    if (!img || !canvas || blocks.length === 0) {
      if (canvas) {
        const ctx = canvas.getContext("2d");
        ctx?.clearRect(0, 0, canvas.width, canvas.height);
      }
      return;
    }

    canvas.width = img.naturalWidth;
    canvas.height = img.naturalHeight;
    const ctx = canvas.getContext("2d")!;
    ctx.clearRect(0, 0, canvas.width, canvas.height);

    ctx.strokeStyle = "#00ff00";
    ctx.lineWidth = 2;
    ctx.font = "16px sans-serif";
    ctx.fillStyle = "#00ff00";

    for (const block of blocks) {
      ctx.strokeRect(block.x, block.y, block.width, block.height);
      ctx.fillText(block.text, block.x, Math.max(block.y - 4, 16));
    }
  }, [blocks]);

  const switchPage = (pageIdx: number) => {
    const page = pages[pageIdx];
    if (page) {
      setImageDataUrl(page.image_data);
      setBlocks([]);
      setCurrentPage(pageIdx);
    }
  };

  return (
    <div className="app">
      <input
        ref={fileInputRef}
        type="file"
        accept="image/*,.docx"
        onChange={handleFileChange}
        style={{ display: "none" }}
      />

      <div className="toolbar">
        <button onClick={handleFileSelect}>
          {fileName ? "重新选择文件" : "选择文件"}
        </button>
        <span className="file-name">{fileName}</span>
        <button onClick={handleRecognize} disabled={!imageDataUrl || loading}>
          {loading ? "识别中..." : "识别"}
        </button>

        {models.length > 1 && (
          <div className="model-selector">
            <label>模型：</label>
            <select
              value={currentModel}
              onChange={(e) => handleModelChange(e.target.value)}
            >
              {models.map((m) => (
                <option key={m} value={m}>{m}</option>
              ))}
            </select>
          </div>
        )}

      </div>

      {loading && <div className="loading-hint">处理中，请稍候...</div>}

      {fileType === "docx" && pages.length > 0 && (
        <div className="page-nav">
          <button
            disabled={currentPage === 0}
            onClick={() => switchPage(currentPage - 1)}
          >
            上一页
          </button>
          <span>第 {currentPage + 1} / {pages.length} 页</span>
          <button
            disabled={currentPage === pages.length - 1}
            onClick={() => switchPage(currentPage + 1)}
          >
            下一页
          </button>
        </div>
      )}

      {imageDataUrl && (
        <div className="image-container">
          <img ref={imgRef} src={imageDataUrl} alt="OCR input" />
          <canvas ref={canvasRef} />
        </div>
      )}

      {blocks.length > 0 && (
        <div className="results">
          <h2>识别结果 ({blocks.length} 项)</h2>
          {blocks.map((b, i) => (
            <div key={i} className="result-item">
              <span className="result-text">{b.text}</span>
              <span className="result-conf">{(b.confidence * 100).toFixed(1)}%</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export default App;
