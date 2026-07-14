# Repository Guidelines

## Project Structure & Module Organization

This repository combines a React frontend with a Rust/Tauri desktop backend.

- `src/`: React 19 UI (`App.tsx`, `main.tsx`, `App.css`), static assets in `src/assets/`
- `src-tauri/src/`: Tauri command layer and desktop entrypoints
- `src-tauri/models/ocr/`: bundled OCR model variants (`v4/mobile`, `v4/server`, `v6/...`)
- `crates/ocr/`: ONNX-based OCR engine
- `crates/docx-to-image/`: DOCX/PDF rendering helpers
- `test/`: sample DOCX/PDF files for manual verification
- `scripts/download-tools.ps1` and `tools/windows-x86_64/`: Windows-side helper tooling

## Build, Test, and Development Commands

- `pnpm install`: install frontend and Tauri JS dependencies
- `pnpm dev`: run the rsbuild web UI only
- `pnpm tauri dev`: launch the full desktop app for local development
- `pnpm build`: build the frontend bundle
- `pnpm tauri build`: produce a desktop package
- `cargo check --workspace`: fast Rust workspace validation
- `cargo test --workspace`: run Rust tests across `src-tauri` and `crates/*`

LibreOffice is a runtime dependency for DOCX/PDF rendering; verify it is available before testing document flows.

## Coding Style & Naming Conventions

Use 4-space indentation in both TypeScript and Rust, matching the existing codebase. Prefer `PascalCase` for React components and Rust types, `camelCase` for variables/functions, and `snake_case` for Rust modules, Tauri commands, and file names where already established. Keep frontend logic in `src/App.tsx` cohesive, and move reusable Rust logic into `crates/` instead of growing `src-tauri/src/lib.rs`.

No dedicated lint script is configured today. Before opening a PR, run `cargo fmt` and keep TypeScript imports, spacing, and semicolon usage consistent with `src/App.tsx`.

## Testing Guidelines

Automated coverage is currently Rust-first. Add unit tests beside Rust modules or under crate-level `tests/` directories, and run them with `cargo test --workspace`. For UI and OCR regressions, manually exercise image, DOCX, and PDF flows using the files in `test/`, especially when changing model loading, page rendering, or block ordering.

## Commit & Pull Request Guidelines

Recent history uses Conventional Commit prefixes such as `feat:`, `fix:`, and `docs:`. Keep commit subjects short and scoped to one change. PRs should describe the affected area, list verification steps, and include screenshots when `src/` UI behavior changes. For OCR or rendering changes, include a sample input and the observed output difference.
