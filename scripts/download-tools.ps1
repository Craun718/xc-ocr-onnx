param(
    [string]$TargetDir,
    [string]$Platform = ""
)

$ErrorActionPreference = "Stop"

# detect platform
if (-not $Platform) {
    $isWin = $IsWindows -or (-not $IsLinux -and -not $IsMacOs)
    if ($isWin) {
        $arch = if ([Environment]::Is64BitOperatingSystem) { "x86_64" } else { "x86" }
        $Platform = "windows-$arch"
    } elseif ($IsLinux) {
        $arch = if ((uname -m) -match "aarch64|arm64") { "arm64" } else { "x86_64" }
        $Platform = "linux-$arch"
    } elseif ($IsMacOs) {
        $arch = if ((uname -m) -match "arm64") { "arm64" } else { "x86_64" }
        $Platform = "macos-$arch"
    }
}

if (-not $TargetDir) {
    $TargetDir = Join-Path $PSScriptRoot "..\src-tauri\tools\$Platform"
}
$null = New-Item -ItemType Directory -Path $TargetDir -Force

Write-Host ">> Downloading tools for $Platform -> $TargetDir" -ForegroundColor Cyan

# helpers

function Download-File($url, $dest) {
    $name = Split-Path $dest -Leaf
    Write-Host "  downloading $name ..." -NoNewline
    try {
        Invoke-WebRequest -Uri $url -OutFile $dest -ErrorAction Stop
        Write-Host " OK" -ForegroundColor Green
    } catch {
        Write-Host " FAILED: $($_.Exception.Message)" -ForegroundColor Red
        throw
    }
}

function Expand-Zip($zip, $dest) {
    $name = Split-Path $zip -Leaf
    Write-Host "  extracting $name ..." -NoNewline
    try {
        Expand-Archive -Path $zip -DestinationPath $dest -Force -ErrorAction Stop
        Write-Host " OK" -ForegroundColor Green
    } catch {
        Write-Host " FAILED: $($_.Exception.Message)" -ForegroundColor Red
        throw
    }
}

# ---- pandoc ----

Write-Host "`n[1/2] Pandoc" -ForegroundColor Yellow
$PANDOC_VER = "3.10"
switch -Wildcard ($Platform) {
    "windows-*" {
        $url = "https://github.com/jgm/pandoc/releases/download/$PANDOC_VER/pandoc-$PANDOC_VER-windows-x86_64.zip"
        $zip = Join-Path $TargetDir "pandoc.zip"
        Download-File $url $zip
        Expand-Zip $zip $TargetDir
        $inner = Join-Path $TargetDir "pandoc-$PANDOC_VER"
        Move-Item (Join-Path $inner "pandoc.exe") (Join-Path $TargetDir "pandoc.exe") -Force
        Remove-Item $inner -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item $zip -Force
        Write-Host "  [OK] pandoc.exe" -ForegroundColor Green
    }
    "linux-arm64" {
        $url = "https://github.com/jgm/pandoc/releases/download/$PANDOC_VER/pandoc-$PANDOC_VER-linux-arm64.tar.gz"
        $tarball = Join-Path $TargetDir "pandoc.tar.gz"
        Download-File $url $tarball
        tar -xzf $tarball -C $TargetDir
        $inner = Join-Path $TargetDir "pandoc-$PANDOC_VER"
        Copy-Item (Join-Path $inner "bin/pandoc") (Join-Path $TargetDir "pandoc") -Force
        Remove-Item $inner -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item $tarball -Force
        chmod +x (Join-Path $TargetDir "pandoc")
        Write-Host "  [OK] pandoc (ARM64)" -ForegroundColor Green
    }
    "linux-x86_64" {
        $url = "https://github.com/jgm/pandoc/releases/download/$PANDOC_VER/pandoc-$PANDOC_VER-linux-amd64.tar.gz"
        $tarball = Join-Path $TargetDir "pandoc.tar.gz"
        Download-File $url $tarball
        tar -xzf $tarball -C $TargetDir
        $inner = Join-Path $TargetDir "pandoc-$PANDOC_VER"
        Copy-Item (Join-Path $inner "bin/pandoc") (Join-Path $TargetDir "pandoc") -Force
        Remove-Item $inner -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item $tarball -Force
        chmod +x (Join-Path $TargetDir "pandoc")
        Write-Host "  [OK] pandoc (x64)" -ForegroundColor Green
    }
    "macos-*" {
        $url = "https://github.com/jgm/pandoc/releases/download/$PANDOC_VER/pandoc-$PANDOC_VER-arm64-macOS.zip"
        $zip = Join-Path $TargetDir "pandoc.zip"
        Download-File $url $zip
        Expand-Zip $zip $TargetDir
        $inner = Join-Path $TargetDir "pandoc-$PANDOC_VER"
        Copy-Item (Join-Path $inner "bin/pandoc") (Join-Path $TargetDir "pandoc") -Force
        Remove-Item $inner -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item $zip -Force
        chmod +x (Join-Path $TargetDir "pandoc")
        Write-Host "  [OK] pandoc (macOS)" -ForegroundColor Green
    }
}

# ---- wkhtmltoimage ----

Write-Host "`n[2/2] wkhtmltoimage" -ForegroundColor Yellow
$WK_VER = "0.12.6.1-3"
switch -Wildcard ($Platform) {
    "windows-x86_64" {
        # Use the portable 7z archive from 0.12.6-1 (latest with Windows builds)
        $url = "https://github.com/wkhtmltopdf/packaging/releases/download/0.12.6-1/wkhtmltox-0.12.6-1.mxe-cross-win64.7z"
        $archive = Join-Path $TargetDir "wkhtmltox.7z"
        Download-File $url $archive

        if (Get-Command 7z -ErrorAction SilentlyContinue) {
            Write-Host "  extracting with 7z ..." -NoNewline
            & 7z x $archive -o"$TargetDir" -y *> $null
            # 7z extracts into wkhtmltox\bin\ - find and move the binary
            $found = Get-ChildItem -Path $TargetDir -Recurse -Filter "wkhtmltoimage.exe" | Select-Object -First 1
            if ($found) {
                Copy-Item $found.FullName (Join-Path $TargetDir "wkhtmltoimage.exe") -Force
                Write-Host " OK" -ForegroundColor Green
            } else {
                Write-Host " FAILED - wkhtmltoimage.exe not found in archive" -ForegroundColor Red
            }
        } else {
            Write-Host "  7z not found. Install 7-Zip or manually extract:" -ForegroundColor DarkYellow
            Write-Host "  $archive" -ForegroundColor DarkYellow
            Write-Host "  Copy wkhtmltoimage.exe to $TargetDir" -ForegroundColor DarkYellow
        }

        Remove-Item $archive -Force -ErrorAction SilentlyContinue
        # clean up extracted subdirs
        Get-ChildItem -Path $TargetDir -Directory | Where-Object { $_.Name -ne "pandoc.exe" -and $_.Name -ne "wkhtmltoimage.exe" } | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
    }
    "linux-arm64" {
        $url = "https://github.com/wkhtmltopdf/packaging/releases/download/$WK_VER/wkhtmltox_$WK_VER.bookworm_arm64.deb"
        $deb = Join-Path $TargetDir "wkhtmltox.deb"
        Download-File $url $deb
        $tmpExtract = Join-Path $TargetDir "_deb"
        $null = New-Item -ItemType Directory -Path $tmpExtract -Force
        & ar x $deb --output=$tmpExtract 2>$null
        if ($?) {
            tar -xzf (Join-Path $tmpExtract "data.tar.gz") -C $tmpExtract 2>$null
            if (-not $?) { tar -xJf (Join-Path $tmpExtract "data.tar.xz") -C $tmpExtract 2>$null }
        }
        $bin = Get-ChildItem -Path $tmpExtract -Recurse -Filter "wkhtmltoimage" | Select-Object -First 1
        if ($bin) {
            Copy-Item $bin.FullName (Join-Path $TargetDir "wkhtmltoimage") -Force
            chmod +x (Join-Path $TargetDir "wkhtmltoimage")
            Write-Host "  [OK] wkhtmltoimage (ARM64)" -ForegroundColor Green
        } else {
            Write-Host "  [FAILED] wkhtmltoimage not found in deb" -ForegroundColor Red
        }
        Remove-Item $tmpExtract -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item $deb -Force -ErrorAction SilentlyContinue
    }
    "linux-x86_64" {
        $url = "https://github.com/wkhtmltopdf/packaging/releases/download/$WK_VER/wkhtmltox_$WK_VER.bookworm_amd64.deb"
        $deb = Join-Path $TargetDir "wkhtmltox.deb"
        Download-File $url $deb
        $tmpExtract = Join-Path $TargetDir "_deb"
        $null = New-Item -ItemType Directory -Path $tmpExtract -Force
        & ar x $deb --output=$tmpExtract 2>$null
        if ($?) {
            tar -xzf (Join-Path $tmpExtract "data.tar.gz") -C $tmpExtract 2>$null
            if (-not $?) { tar -xJf (Join-Path $tmpExtract "data.tar.xz") -C $tmpExtract 2>$null }
        }
        $bin = Get-ChildItem -Path $tmpExtract -Recurse -Filter "wkhtmltoimage" | Select-Object -First 1
        if ($bin) {
            Copy-Item $bin.FullName (Join-Path $TargetDir "wkhtmltoimage") -Force
            chmod +x (Join-Path $TargetDir "wkhtmltoimage")
            Write-Host "  [OK] wkhtmltoimage (x64)" -ForegroundColor Green
        } else {
            Write-Host "  [FAILED] wkhtmltoimage not found in deb" -ForegroundColor Red
        }
        Remove-Item $tmpExtract -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item $deb -Force -ErrorAction SilentlyContinue
    }
    default {
        Write-Host "  >> Manual download required: https://wkhtmltopdf.org/downloads.html" -ForegroundColor DarkYellow
        Write-Host "     Copy wkhtmltoimage to: $TargetDir" -ForegroundColor DarkYellow
    }
}

# verify

Write-Host "`n-- Verification --" -ForegroundColor Cyan
$pandocExe = if ($Platform -like "windows-*") { "pandoc.exe" } else { "pandoc" }
$wkExe = if ($Platform -like "windows-*") { "wkhtmltoimage.exe" } else { "wkhtmltoimage" }
$pandocOk = Test-Path (Join-Path $TargetDir $pandocExe)
$wkOk = Test-Path (Join-Path $TargetDir $wkExe)
if ($pandocOk) { Write-Host "  [OK] $pandocExe" -ForegroundColor Green }
else { Write-Host "  [MISSING] $pandocExe" -ForegroundColor Red }
if ($wkOk) { Write-Host "  [OK] $wkExe" -ForegroundColor Green }
else { Write-Host "  [MISSING] $wkExe" -ForegroundColor Red }

if ($pandocOk -and $wkOk) {
    Write-Host "`nAll tools ready!" -ForegroundColor Green
} else {
    Write-Host "`nSome tools are missing. See README.md for manual setup." -ForegroundColor Yellow
}
