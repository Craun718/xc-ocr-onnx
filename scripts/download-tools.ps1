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
    $TargetDir = Join-Path $PSScriptRoot "..\tools\$Platform"
}
$null = New-Item -ItemType Directory -Path $TargetDir -Force

# Use system temp dir for all downloads/extraction to avoid triggering file watchers
$WORK = Join-Path $env:TEMP "xc-ocr-tools-$PID"
$null = New-Item -ItemType Directory -Path $WORK -Force

Write-Host ">> Downloading tools for $Platform -> $TargetDir" -ForegroundColor Cyan
Write-Host "   (working in $WORK)" -ForegroundColor DarkGray

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

Write-Host "`n[1/3] Pandoc" -ForegroundColor Yellow
$PANDOC_VER = "3.10"
switch -Wildcard ($Platform) {
    "windows-*" {
        $url = "https://github.com/jgm/pandoc/releases/download/$PANDOC_VER/pandoc-$PANDOC_VER-windows-x86_64.zip"
        $zip = Join-Path $WORK "pandoc.zip"
        Download-File $url $zip
        Expand-Zip $zip $WORK
        $inner = Join-Path $WORK "pandoc-$PANDOC_VER"
        Copy-Item (Join-Path $inner "pandoc.exe") (Join-Path $TargetDir "pandoc.exe") -Force
        Write-Host "  [OK] pandoc.exe" -ForegroundColor Green
    }
    "linux-arm64" {
        $url = "https://github.com/jgm/pandoc/releases/download/$PANDOC_VER/pandoc-$PANDOC_VER-linux-arm64.tar.gz"
        $tarball = Join-Path $WORK "pandoc.tar.gz"
        Download-File $url $tarball
        tar -xzf $tarball -C $WORK
        $inner = Join-Path $WORK "pandoc-$PANDOC_VER"
        Copy-Item (Join-Path $inner "bin/pandoc") (Join-Path $TargetDir "pandoc") -Force
        chmod +x (Join-Path $TargetDir "pandoc")
        Write-Host "  [OK] pandoc (ARM64)" -ForegroundColor Green
    }
    "linux-x86_64" {
        $url = "https://github.com/jgm/pandoc/releases/download/$PANDOC_VER/pandoc-$PANDOC_VER-linux-amd64.tar.gz"
        $tarball = Join-Path $WORK "pandoc.tar.gz"
        Download-File $url $tarball
        tar -xzf $tarball -C $WORK
        $inner = Join-Path $WORK "pandoc-$PANDOC_VER"
        Copy-Item (Join-Path $inner "bin/pandoc") (Join-Path $TargetDir "pandoc") -Force
        chmod +x (Join-Path $TargetDir "pandoc")
        Write-Host "  [OK] pandoc (x64)" -ForegroundColor Green
    }
    "macos-*" {
        $url = "https://github.com/jgm/pandoc/releases/download/$PANDOC_VER/pandoc-$PANDOC_VER-arm64-macOS.zip"
        $zip = Join-Path $WORK "pandoc.zip"
        Download-File $url $zip
        Expand-Zip $zip $WORK
        $inner = Join-Path $WORK "pandoc-$PANDOC_VER"
        Copy-Item (Join-Path $inner "bin/pandoc") (Join-Path $TargetDir "pandoc") -Force
        chmod +x (Join-Path $TargetDir "pandoc")
        Write-Host "  [OK] pandoc (macOS)" -ForegroundColor Green
    }
}

# ---- wkhtmltopdf ----

Write-Host "`n[2/3] wkhtmltopdf" -ForegroundColor Yellow
$WK_VER = "0.12.6.1-3"
switch -Wildcard ($Platform) {
    "windows-x86_64" {
        $url = "https://github.com/wkhtmltopdf/packaging/releases/download/0.12.6-1/wkhtmltox-0.12.6-1.mxe-cross-win64.7z"
        $archive = Join-Path $WORK "wkhtmltox.7z"
        Download-File $url $archive

        if (Get-Command 7z -ErrorAction SilentlyContinue) {
            Write-Host "  extracting with 7z ..." -NoNewline
            & 7z x $archive -o"$WORK\_wk" -y *> $null
            $bin = Get-ChildItem -Path $WORK\_wk -Recurse -Filter "wkhtmltopdf.exe" | Select-Object -First 1
            $img = Get-ChildItem -Path $WORK\_wk -Recurse -Filter "wkhtmltoimage.exe" | Select-Object -First 1
            if ($bin) {
                Copy-Item $bin.FullName (Join-Path $TargetDir "wkhtmltopdf.exe") -Force
            }
            if ($img) {
                Copy-Item $img.FullName (Join-Path $TargetDir "wkhtmltoimage.exe") -Force
            }
            if ($bin -and $img) {
                Write-Host " OK (wkhtmltopdf + wkhtmltoimage)" -ForegroundColor Green
            } else {
                Write-Host " FAILED - exe not found in archive" -ForegroundColor Red
            }
            Remove-Item $WORK\_wk -Recurse -Force -ErrorAction SilentlyContinue
        } else {
            Write-Host "  [SKIP] 7z not found. Install wkhtmltopdf manually." -ForegroundColor DarkYellow
        }
    }
    "linux-arm64" {
        $url = "https://github.com/wkhtmltopdf/packaging/releases/download/$WK_VER/wkhtmltox_$WK_VER.bookworm_arm64.deb"
        $deb = Join-Path $WORK "wkhtmltox.deb"
        Download-File $url $deb
        $tmpExtract = Join-Path $WORK "_deb"
        $null = New-Item -ItemType Directory -Path $tmpExtract -Force
        & ar x $deb --output=$tmpExtract 2>$null
        if ($?) {
            tar -xzf (Join-Path $tmpExtract "data.tar.gz") -C $tmpExtract 2>$null
            if (-not $?) { tar -xJf (Join-Path $tmpExtract "data.tar.xz") -C $tmpExtract 2>$null }
        }
        $bin = Get-ChildItem -Path $tmpExtract -Recurse -Filter "wkhtmltopdf" | Select-Object -First 1
        $img = Get-ChildItem -Path $tmpExtract -Recurse -Filter "wkhtmltoimage" | Select-Object -First 1
        if ($bin -and $img) {
            Copy-Item $bin.FullName (Join-Path $TargetDir "wkhtmltopdf") -Force
            Copy-Item $img.FullName (Join-Path $TargetDir "wkhtmltoimage") -Force
            chmod +x (Join-Path $TargetDir "wkhtmltopdf")
            chmod +x (Join-Path $TargetDir "wkhtmltoimage")
            Write-Host "  [OK] wkhtmltopdf + wkhtmltoimage (ARM64)" -ForegroundColor Green
        } else {
            Write-Host "  [FAILED] binaries not found in deb" -ForegroundColor Red
        }
        Remove-Item $tmpExtract -Recurse -Force -ErrorAction SilentlyContinue
    }
    "linux-x86_64" {
        $url = "https://github.com/wkhtmltopdf/packaging/releases/download/$WK_VER/wkhtmltox_$WK_VER.bookworm_amd64.deb"
        $deb = Join-Path $WORK "wkhtmltox.deb"
        Download-File $url $deb
        $tmpExtract = Join-Path $WORK "_deb"
        $null = New-Item -ItemType Directory -Path $tmpExtract -Force
        & ar x $deb --output=$tmpExtract 2>$null
        if ($?) {
            tar -xzf (Join-Path $tmpExtract "data.tar.gz") -C $tmpExtract 2>$null
            if (-not $?) { tar -xJf (Join-Path $tmpExtract "data.tar.xz") -C $tmpExtract 2>$null }
        }
        $bin = Get-ChildItem -Path $tmpExtract -Recurse -Filter "wkhtmltopdf" | Select-Object -First 1
        $img = Get-ChildItem -Path $tmpExtract -Recurse -Filter "wkhtmltoimage" | Select-Object -First 1
        if ($bin -and $img) {
            Copy-Item $bin.FullName (Join-Path $TargetDir "wkhtmltopdf") -Force
            Copy-Item $img.FullName (Join-Path $TargetDir "wkhtmltoimage") -Force
            chmod +x (Join-Path $TargetDir "wkhtmltopdf")
            chmod +x (Join-Path $TargetDir "wkhtmltoimage")
            Write-Host "  [OK] wkhtmltopdf + wkhtmltoimage (x64)" -ForegroundColor Green
        } else {
            Write-Host "  [FAILED] binaries not found in deb" -ForegroundColor Red
        }
        Remove-Item $tmpExtract -Recurse -Force -ErrorAction SilentlyContinue
    }
    default {
        Write-Host "  >> Manual download: https://wkhtmltopdf.org/downloads.html" -ForegroundColor DarkYellow
    }
}

# ---- ghostscript ----

Write-Host "`n[3/3] Ghostscript" -ForegroundColor Yellow
$GS_VER = "10.04.0"
switch -Wildcard ($Platform) {
    "windows-x86_64" {
        $url = "https://github.com/ArtifexSoftware/ghostpdl-downloads/releases/download/gs10040/gs10040w64.exe"
        $exe = Join-Path $WORK "gs-setup.exe"
        Download-File $url $exe

        if (Get-Command 7z -ErrorAction SilentlyContinue) {
            Write-Host "  extracting with 7z ..." -NoNewline
            & 7z x $exe -o"$WORK\_gs" -y *> $null
            $found = Get-ChildItem -Path $WORK\_gs -Recurse -Filter "gswin64c.exe" | Select-Object -First 1
            if ($found) {
                Copy-Item $found.FullName (Join-Path $TargetDir "gswin64c.exe") -Force
                Write-Host " OK" -ForegroundColor Green
            } else {
                Write-Host " FAILED - gswin64c.exe not found in installer" -ForegroundColor Red
            }
            Remove-Item $WORK\_gs -Recurse -Force -ErrorAction SilentlyContinue
        } else {
            Write-Host "  [SKIP] 7z not found. Install Ghostscript manually." -ForegroundColor DarkYellow
        }
    }
    "windows-x86" {
        $url = "https://github.com/ArtifexSoftware/ghostpdl-downloads/releases/download/gs10040/gs10040w32.exe"
        $exe = Join-Path $WORK "gs-setup.exe"
        Download-File $url $exe

        if (Get-Command 7z -ErrorAction SilentlyContinue) {
            Write-Host "  extracting with 7z ..." -NoNewline
            & 7z x $exe -o"$WORK\_gs" -y *> $null
            $found = Get-ChildItem -Path $WORK\_gs -Recurse -Filter "gswin32c.exe" | Select-Object -First 1
            if ($found) {
                Copy-Item $found.FullName (Join-Path $TargetDir "gswin32c.exe") -Force
                Write-Host " OK" -ForegroundColor Green
            } else {
                Write-Host " FAILED - gswin32c.exe not found in installer" -ForegroundColor Red
            }
            Remove-Item $WORK\_gs -Recurse -Force -ErrorAction SilentlyContinue
        } else {
            Write-Host "  [SKIP] 7z not found. Install Ghostscript manually." -ForegroundColor DarkYellow
        }
    }
    "linux-*" {
        $url = "https://github.com/ArtifexSoftware/ghostpdl-downloads/releases/download/gs10040/ghostscript-10.04.0-linux-x86_64.tgz"
        $tarball = Join-Path $WORK "gs.tar.gz"
        Download-File $url $tarball
        tar -xzf $tarball -C $WORK
        $inner = Join-Path $WORK "ghostscript-10.04.0-linux-x86_64"
        $gsBin = Join-Path $inner "gs-10040-linux-x86_64"
        if (Test-Path $gsBin) {
            Copy-Item $gsBin (Join-Path $TargetDir "gs") -Force
            chmod +x (Join-Path $TargetDir "gs")
            Write-Host "  [OK] gs" -ForegroundColor Green
        } else {
            Write-Host "  [FAILED] gs binary not found in archive" -ForegroundColor Red
        }
        Remove-Item $inner -Recurse -Force -ErrorAction SilentlyContinue
    }
    "macos-*" {
        Write-Host "  >> Install: brew install ghostscript" -ForegroundColor DarkYellow
    }
}

# Cleanup temp dir
Remove-Item $WORK -Recurse -Force -ErrorAction SilentlyContinue

# verify

Write-Host "`n-- Verification --" -ForegroundColor Cyan
$pandocExe = if ($Platform -like "windows-*") { "pandoc.exe" } else { "pandoc" }
$wkExe = if ($Platform -like "windows-*") { "wkhtmltopdf.exe" } else { "wkhtmltopdf" }
$gsExe = if ($Platform -eq "windows-x86_64") { "gswin64c.exe" } elseif ($Platform -eq "windows-x86") { "gswin32c.exe" } else { "gs" }
$pandocOk = Test-Path (Join-Path $TargetDir $pandocExe)
$wkOk = Test-Path (Join-Path $TargetDir $wkExe)
$gsOk = Test-Path (Join-Path $TargetDir $gsExe)
if ($pandocOk) { Write-Host "  [OK] $pandocExe" -ForegroundColor Green }
else { Write-Host "  [MISSING] $pandocExe" -ForegroundColor Red }
if ($wkOk) { Write-Host "  [OK] $wkExe" -ForegroundColor Green }
else { Write-Host "  [MISSING] $wkExe" -ForegroundColor Red }
if ($gsOk) { Write-Host "  [OK] $gsExe" -ForegroundColor Green }
else { Write-Host "  [MISSING] $gsExe" -ForegroundColor Red }

if ($pandocOk -and $wkOk -and $gsOk) {
    Write-Host "`nAll tools ready!" -ForegroundColor Green
} else {
    Write-Host "`nSome tools are missing. See README.md for manual setup." -ForegroundColor Yellow
}
