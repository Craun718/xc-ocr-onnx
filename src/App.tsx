import { useState, useRef, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { info, error } from "@tauri-apps/plugin-log";
import "./App.css";

interface OcrBlock {
	text: string;
	confidence: number;
	x: number;
	y: number;
	width: number;
	height: number;
}

interface RecognizeResult {
	blocks: OcrBlock[];
	corrected_image: string | null;
	rotation_angle: number;
}

interface PageImage {
	page: number;
	width: number;
	height: number;
	orientation: string;
	image_data: string;
}

function App() {
	const [imageDataUrl, setImageDataUrl] = useState<string | null>(null);
	const [blocks, setBlocks] = useState<OcrBlock[]>([]);
	const [loading, setLoading] = useState(false);
	const [fileType, setFileType] = useState<"image" | "docx" | "pdf" | null>(null);
	const [pages, setPages] = useState<(PageImage | null)[]>([]);
	const [pageCount, setPageCount] = useState(0);
	const [currentPage, setCurrentPage] = useState(0);
	const [pdfData, setPdfData] = useState<string | null>(null); // 缓存 PDF base64 数据
	const [fileName, setFileName] = useState("");
	const [filePath, setFilePath] = useState("");
	const [selectedIndex, setSelectedIndex] = useState<number | null>(null);
	const [searchQuery, setSearchQuery] = useState("");

	// Zoom: absolute scale (1 = native pixels, 0.5 = half, 2 = double)
	const [zoomLevel, setZoomLevel] = useState(1);
	// Display size in CSS px — drives img/canvas/container layout
	const [displaySize, setDisplaySize] = useState({ w: 0, h: 0 });

	const ZOOM_STEP = 0.1;
	const ZOOM_MIN = 0.1;
	const ZOOM_MAX = 10;

	// model switching
	const [models, setModels] = useState<string[]>([]);
	const [currentModel, setCurrentModel] = useState("");

	const imgRef = useRef<HTMLImageElement>(null);
	const canvasRef = useRef<HTMLCanvasElement>(null);
	const containerRef = useRef<HTMLDivElement>(null);
	const rawBase64Ref = useRef<string>("");

	useEffect(() => {
		invoke<string[]>("list_models")
			.then((list) => {
				setModels(list);
				if (list.length > 0) setCurrentModel(list[0]);
				info(`[前端] 可用模型: ${list.join(", ")}`);
			})
			.catch((err) => {
				error(`[前端] 获取模型列表失败: ${err}`);
			});
	}, []);

	const handleModelChange = async (variant: string) => {
		setCurrentModel(variant);
		setBlocks([]);
		try {
			await invoke("switch_model", { variant });
			info(`[前端] 切换模型: ${variant}`);
		} catch (err) {
			error(`[前端] 切换模型失败: ${err}`);
			alert("切换模型失败: " + err);
		}
	};

	const handleFileSelect = async () => {
		const selected = await open({
			multiple: false,
			filters: [
				{
					name: "图片与文档",
					extensions: [
						"png",
						"jpg",
						"jpeg",
						"bmp",
						"tiff",
						"tif",
						"webp",
						"docx",
						"pdf",
					],
				},
			],
		});
		if (!selected) return;

		const path = selected as string;
		const name = path.split("\\").pop()?.split("/").pop() || path;

		setFilePath(path);
		setFileName(name);
		setBlocks([]);
		setPages([]);
		setCurrentPage(0);
		setImageDataUrl(null);
		setPdfData(null);
		setPageCount(0);
		setLoading(true);

		try {
			const dataUrl = await invoke<string>("read_file_as_data_url", { path });
			rawBase64Ref.current = dataUrl;
			info(`[前端] 读取文件: ${name}`);

			if (name.endsWith(".pdf")) {
				setFileType("pdf");
				setPdfData(dataUrl);
				// 获取 PDF 页数
				const count = await invoke<number>("pdf_page_count", { data: dataUrl });
				setPageCount(count);
				// 初始化页面数组（全部为 null，等待懒加载）
				setPages(new Array(count).fill(null));
				// 渲染第一页
				if (count > 0) {
					const firstPage: PageImage = await invoke("render_pdf_page", {
						data: dataUrl,
						page: 0,
					});
					setPages(prev => {
						const newPages = [...prev];
						newPages[0] = firstPage;
						return newPages;
					});
					setImageDataUrl(firstPage.image_data);
					setCurrentPage(0);
				}
			} else if (name.endsWith(".docx")) {
				setFileType("docx");
				const result: PageImage[] = await invoke("render_docx", {
					filename: path,
					data: dataUrl,
				});
				setPages(result);
				if (result.length > 0) {
					setImageDataUrl(result[0].image_data);
					setCurrentPage(0);
				}
			} else {
				setFileType("image");
				setImageDataUrl(dataUrl);
			}
		} catch (err) {
			error(`[前端] 读取文件失败: ${err}`);
			alert("读取文件失败: " + err);
		} finally {
			setLoading(false);
		}
	};

	const handleRecognize = async () => {
		if (!rawBase64Ref.current) return;
		setLoading(true);
		setBlocks([]);
		try {
			const inputData =
				(fileType === "docx" || fileType === "pdf") && pages[currentPage]
					? pages[currentPage]!.image_data
					: rawBase64Ref.current;

			const label =
				fileType === "docx" || fileType === "pdf"
					? `${filePath} - 第 ${currentPage + 1} 页`
					: filePath;

			const result: RecognizeResult = await invoke("recognize_image", {
				filename: label,
				data: inputData,
			});

			// 如果检测到旋转并矫正了图像，更新显示的图像
			if (result.corrected_image && result.rotation_angle > 0) {
				setImageDataUrl(result.corrected_image);
				if (fileType === "image") {
					rawBase64Ref.current = result.corrected_image;
				}
			}

			setBlocks(result.blocks);
		} catch (err) {
			error(`[前端] OCR 识别失败: ${err}`);
			alert("OCR 识别失败: " + err);
		} finally {
			setLoading(false);
		}
	};

	function getConfidenceColor(conf: number): string {
		if (conf >= 0.9) return "#4ade80"; // green
		if (conf >= 0.7) return "#facc15"; // yellow
		if (conf >= 0.5) return "#fb923c"; // orange
		return "#ef4444"; // red
	}

	function drawOcrBlock(
		ctx: CanvasRenderingContext2D,
		block: OcrBlock,
		index: number,
		isHighlighted: boolean
	) {
		const { x, y, width, height, confidence, text } = block;
		const color = getConfidenceColor(confidence);
		const confPct = (confidence * 100).toFixed(1) + "%";
		const FONT_SIZE = 20;
		const PAD = 3;

		// Draw rectangle
		ctx.strokeStyle = isHighlighted ? "#ffffff" : color;
		ctx.lineWidth = isHighlighted ? 3 : 2;
		ctx.strokeRect(x, y, width, height);

		ctx.font = `${FONT_SIZE}px sans-serif`;
		ctx.textBaseline = "top";
		const textH = FONT_SIZE;

		// --- Top-right outside: index number ---
		const idxText = `${index + 1}`;
		const idxW = ctx.measureText(idxText).width;
		ctx.fillStyle = "rgba(0,0,0,0.65)";
		ctx.fillRect(x + width + PAD, y + PAD, idxW + PAD * 2, textH + PAD * 2);
		ctx.fillStyle = color;
		ctx.fillText(idxText, x + width + PAD * 2, y + PAD * 2);

		// --- Top-left outside: confidence (right-aligned) ---
		const confW = ctx.measureText(confPct).width;
		const confLabelX = x - PAD - confW - PAD * 2;
		ctx.fillStyle = "rgba(0,0,0,0.65)";
		ctx.fillRect(confLabelX - PAD, y + PAD, confW + PAD * 2 + 2, textH + PAD * 2);
		ctx.fillStyle = color;
		ctx.fillText(confPct, confLabelX, y + PAD * 2);

		// --- Bottom-right outside: recognized text ---
		ctx.textBaseline = "bottom";
		const textY = y + height + PAD;
		const maxTextW = Math.min(600, width * 2);
		let displayText = text;
		if (ctx.measureText(displayText).width > maxTextW) {
			while (displayText.length > 1 && ctx.measureText(displayText + "...").width > maxTextW) {
				displayText = displayText.slice(0, -1);
			}
			displayText += "...";
		}
		const textW = ctx.measureText(displayText).width;
		ctx.fillStyle = "rgba(0,0,0,0.65)";
		ctx.fillRect(x + PAD, textY + PAD, textW + PAD * 2 + 2, textH + PAD * 2);
		ctx.fillStyle = "#ffffff";
		ctx.fillText(displayText, x + PAD * 2, textY + PAD * 2 + textH);
	}

	// Draw canvas overlay when blocks or selection changes
	useEffect(() => {
		const img = imgRef.current;
		const canvas = canvasRef.current;
		if (!img || !canvas) return;

		canvas.width = img.naturalWidth;
		canvas.height = img.naturalHeight;
		const ctx = canvas.getContext("2d")!;
		ctx.clearRect(0, 0, canvas.width, canvas.height);

		if (blocks.length === 0) return;

		for (let i = 0; i < blocks.length; i++) {
			drawOcrBlock(ctx, blocks[i], i, selectedIndex === i);
		}
	}, [blocks, selectedIndex]);

	// On image load: compute initial fit-zoom
	useEffect(() => {
		const img = imgRef.current;
		const container = containerRef.current;
		if (!img || !container) return;

		const onLoad = () => {
			const nw = img.naturalWidth;
			const nh = img.naturalHeight;
			if (nw === 0) return;
			// Fit to container width with small padding, cap height to 80vh
			const maxW = container.clientWidth - 8;
			const maxH = window.innerHeight * 0.75;
			const fit = Math.min(1, maxW / nw, maxH / nh);
			setZoomLevel(fit);
			setDisplaySize({ w: nw * fit, h: nh * fit });
		};

		if (img.complete && img.naturalWidth > 0) {
			onLoad();
		} else {
			img.addEventListener("load", onLoad);
			return () => img.removeEventListener("load", onLoad);
		}
	// We intentionally only run this when imageDataUrl changes
	// eslint-disable-next-line react-hooks/exhaustive-deps
	}, [imageDataUrl]);

	// When zoomLevel changes, update displaySize
	useEffect(() => {
		const img = imgRef.current;
		if (!img || img.naturalWidth === 0) return;
		setDisplaySize({ w: img.naturalWidth * zoomLevel, h: img.naturalHeight * zoomLevel });
	}, [zoomLevel]);

	const handleZoomDelta = useCallback((delta: number) => {
		const img = imgRef.current;
		if (!img) return;
		const nw = img.naturalWidth;
		const nh = img.naturalHeight;
		if (nw === 0) return;
		setZoomLevel(z => {
			const next = z + delta;
			const clamped = Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, Math.round(next / ZOOM_STEP) * ZOOM_STEP));
			setDisplaySize({ w: nw * clamped, h: nh * clamped });
			return clamped;
		});
	}, []);

	const zoomIn = useCallback(() => handleZoomDelta(ZOOM_STEP), [handleZoomDelta]);
	const zoomOut = useCallback(() => handleZoomDelta(-ZOOM_STEP), [handleZoomDelta]);

	const resetZoom = useCallback(() => {
		const img = imgRef.current;
		const container = containerRef.current;
		if (!img || !container) return;
		const nw = img.naturalWidth;
		const nh = img.naturalHeight;
		if (nw === 0) return;
		const maxW = container.clientWidth - 8;
		const maxH = window.innerHeight * 0.75;
		const fit = Math.min(1, maxW / nw, maxH / nh);
		setZoomLevel(fit);
		setDisplaySize({ w: nw * fit, h: nh * fit });
	}, []);

	const switchPage = async (pageIdx: number) => {
		// 对于 PDF，检查页面是否已缓存
		if (fileType === "pdf") {
			const cachedPage = pages[pageIdx];
			if (cachedPage) {
				// 已缓存，直接显示
				setImageDataUrl(cachedPage.image_data);
				setBlocks([]);
				setCurrentPage(pageIdx);
			} else if (pdfData) {
				// 未缓存，渲染页面
				setLoading(true);
				try {
					const renderedPage: PageImage = await invoke("render_pdf_page", {
						data: pdfData,
						page: pageIdx,
					});
					setPages(prev => {
						const newPages = [...prev];
						newPages[pageIdx] = renderedPage;
						return newPages;
					});
					setImageDataUrl(renderedPage.image_data);
					setBlocks([]);
					setCurrentPage(pageIdx);
				} catch (err) {
					alert("渲染页面失败: " + err);
				} finally {
					setLoading(false);
				}
			}
		} else {
			// DOCX：所有页面已预渲染
			const page = pages[pageIdx];
			if (page) {
				setImageDataUrl(page.image_data);
				setBlocks([]);
				setCurrentPage(pageIdx);
			}
		}
	};

	// 计算实际页数（用于分页导航显示）
	const totalPages = fileType === "pdf" ? pageCount : pages.length;

	return (
		<div className="app">
			<div className="toolbar">
				<button onClick={handleFileSelect}>
					{fileName ? "重新选择文件" : "选择文件"}
				</button>
				<span className="file-name">{fileName}</span>
				<button onClick={handleRecognize} disabled={!imageDataUrl || loading}>
					{loading ? "识别中..." : "识别"}
				</button>

				{imageDataUrl && (
					<div className="zoom-controls">
						<button onClick={zoomOut} disabled={zoomLevel <= ZOOM_MIN} title="缩小">-</button>
						<span className="zoom-label">{(zoomLevel * 100).toFixed(0)}%</span>
						<button onClick={zoomIn} disabled={zoomLevel >= ZOOM_MAX} title="放大">+</button>
						<button onClick={resetZoom} className="zoom-reset-btn">还原</button>
					</div>
				)}

				{models.length > 1 && (
					<div className="model-selector">
						<label>模型：</label>
						<select
							value={currentModel}
							onChange={(e) => handleModelChange(e.target.value)}
						>
							{models.map((m) => (
								<option key={m} value={m}>
									{m}
								</option>
							))}
						</select>
					</div>
				)}
			</div>

			{loading && <div className="loading-hint">处理中，请稍候...</div>}

			{(fileType === "docx" || fileType === "pdf") && totalPages > 0 && (
				<div className="page-nav">
					<button
						disabled={currentPage === 0}
						onClick={() => switchPage(currentPage - 1)}
					>
						上一页
					</button>
					<span>
						第 {currentPage + 1} / {totalPages} 页
					</span>
					<button
						disabled={currentPage === totalPages - 1}
						onClick={() => switchPage(currentPage + 1)}
					>
						下一页
					</button>
				</div>
			)}

			{imageDataUrl && (
				<div className="image-container" ref={containerRef}>
					<div className="image-viewport" style={{ width: displaySize.w || undefined, height: displaySize.h || undefined }}>
						<img
							ref={imgRef}
							src={imageDataUrl}
							alt="OCR input"
							draggable={false}
							style={{ width: displaySize.w || undefined, height: displaySize.h || undefined }}
						/>
						<canvas
							ref={canvasRef}
							style={{ width: displaySize.w || undefined, height: displaySize.h || undefined }}
						/>
					</div>
				</div>
			)}

			{blocks.length > 0 && (
				<div className="results">
					<div className="results-header">
						<h2>识别结果 ({blocks.length} 项)</h2>
						<input
							type="text"
							className="search-input"
							placeholder="搜索文字..."
							value={searchQuery}
							onChange={(e) => setSearchQuery(e.target.value)}
						/>
					</div>
					{(() => {
						const filtered = searchQuery
							? blocks.filter((b) =>
									b.text.toLowerCase().includes(searchQuery.toLowerCase())
							  )
							: blocks;
						if (filtered.length === 0) {
							return <div className="no-results">未找到匹配项</div>;
						}
						return filtered.map((b) => {
							const origIndex = blocks.indexOf(b);
							return (
								<div
									key={origIndex}
									className={`result-item${selectedIndex === origIndex ? " selected" : ""}`}
									onClick={() =>
										setSelectedIndex(selectedIndex === origIndex ? null : origIndex)
									}
								>
									<span className="result-idx">{origIndex + 1}</span>
									<span className="result-text">{b.text}</span>
									<span
										className="result-conf"
										style={{ color: getConfidenceColor(b.confidence) }}
									>
										{(b.confidence * 100).toFixed(1)}%
									</span>
								</div>
							);
						});
					})()}
				</div>
			)}
		</div>
	);
}

export default App;