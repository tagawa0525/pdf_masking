<#
.SYNOPSIS
    pdf_masking の Windows 開発環境セットアップスクリプト。

.DESCRIPTION
    ネイティブ依存ライブラリ (leptonica, jbig2enc, pdfium) をダウンロード・ビルドし、
    環境変数設定スクリプト (env-windows.ps1) を生成する。

    初回のみ実行が必要。以降は各ターミナルセッションで env-windows.ps1 を読み込む。

.PARAMETER Triplet
    vcpkg トリプレット (既定: x64-windows-static-md)。
    静的ライブラリ + 動的CRT で Rust の既定リンク方式と一致する。

.PARAMETER PdfiumRepo
    pdfium プリビルドバイナリの GitHub リポジトリ (owner/repo 形式)。

.EXAMPLE
    .\scripts\setup-windows.ps1
    . .\scripts\env-windows.ps1
    cargo build
#>

param(
    [string]$Triplet = "x64-windows-static-md",
    [string]$PdfiumRepo = "nicehash/nicehash-pdfium-binaries"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"  # Invoke-WebRequest の進捗表示を抑制

$ProjectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$DepsDir = Join-Path $ProjectRoot "deps"

function Write-Step([string]$Message) {
    Write-Host "`n=== $Message ===" -ForegroundColor Cyan
}

function Assert-Command([string]$Name, [string]$InstallHint) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        Write-Host "  [MISSING] $Name - $InstallHint" -ForegroundColor Red
        return $false
    }
    Write-Host "  [OK] $Name" -ForegroundColor Green
    return $true
}

# ============================================================
# 1. Prerequisites
# ============================================================
Write-Step "Prerequisites check"

$ok = $true
$ok = (Assert-Command "git"   "https://git-scm.com/") -and $ok
$ok = (Assert-Command "cmake" "https://cmake.org/ or winget install Kitware.CMake") -and $ok
$ok = (Assert-Command "cargo" "https://rustup.rs/") -and $ok

# MSVC (cl.exe) の存在確認
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
if (Test-Path $vswhere) {
    $vsPath = & $vswhere -latest -products * `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
        -property installationPath 2>$null
    if ($vsPath) {
        Write-Host "  [OK] MSVC ($vsPath)" -ForegroundColor Green
    } else {
        Write-Host "  [MISSING] Visual Studio C++ Build Tools" -ForegroundColor Red
        Write-Host "           winget install Microsoft.VisualStudio.2022.BuildTools" -ForegroundColor Yellow
        $ok = $false
    }
} else {
    Write-Host "  [MISSING] Visual Studio / Build Tools" -ForegroundColor Red
    $ok = $false
}

if (-not $ok) {
    Write-Host "`nMissing prerequisites. Install them and re-run." -ForegroundColor Red
    exit 1
}

# ============================================================
# 2. deps/ ディレクトリ作成
# ============================================================
New-Item -ItemType Directory -Path $DepsDir -Force | Out-Null

# ============================================================
# 3. vcpkg のセットアップ
# ============================================================
Write-Step "vcpkg"

$VcpkgDir = Join-Path $DepsDir "vcpkg"
$VcpkgExe = Join-Path $VcpkgDir "vcpkg.exe"

if (-not (Test-Path $VcpkgExe)) {
    if (-not (Test-Path $VcpkgDir)) {
        Write-Host "Cloning vcpkg..."
        git clone https://github.com/microsoft/vcpkg.git $VcpkgDir
    }
    Write-Host "Bootstrapping vcpkg..."
    & (Join-Path $VcpkgDir "bootstrap-vcpkg.bat") -disableMetrics
    if ($LASTEXITCODE -ne 0) { throw "vcpkg bootstrap failed" }
}

$VcpkgInstalled = Join-Path $VcpkgDir "installed" $Triplet
Write-Host "vcpkg ready: $VcpkgDir" -ForegroundColor Green

# ============================================================
# 4. leptonica (vcpkg)
# ============================================================
Write-Step "leptonica (via vcpkg, triplet=$Triplet)"

& $VcpkgExe install "leptonica:$Triplet"
if ($LASTEXITCODE -ne 0) { throw "vcpkg install leptonica failed" }

$LeptonicaInclude = Join-Path $VcpkgInstalled "include"
$LeptonicaLib = Join-Path $VcpkgInstalled "lib"
Write-Host "leptonica installed: $VcpkgInstalled" -ForegroundColor Green

# ============================================================
# 5. jbig2enc (ソースからビルド)
# ============================================================
Write-Step "jbig2enc (build from source)"

$Jbig2SrcDir     = Join-Path $DepsDir "jbig2enc-src"
$Jbig2BuildDir   = Join-Path $DepsDir "jbig2enc-build"
$Jbig2InstallDir = Join-Path $DepsDir "jbig2enc"

# ソース取得
if (-not (Test-Path (Join-Path $Jbig2SrcDir ".git"))) {
    Write-Host "Cloning jbig2enc..."
    git clone https://github.com/agl/jbig2enc.git $Jbig2SrcDir
}

# CMakeLists.txt をコピー (autotools は Windows 非対応)
Copy-Item (Join-Path $PSScriptRoot "jbig2enc-CMakeLists.txt") `
          (Join-Path $Jbig2SrcDir "CMakeLists.txt") -Force

# ビルド
Write-Host "Configuring jbig2enc..."
cmake -S $Jbig2SrcDir -B $Jbig2BuildDir `
    -DCMAKE_PREFIX_PATH="$VcpkgInstalled" `
    -DLEPTONICA_INCLUDE_DIR="$LeptonicaInclude"
if ($LASTEXITCODE -ne 0) { throw "jbig2enc cmake configure failed" }

Write-Host "Building jbig2enc..."
cmake --build $Jbig2BuildDir --config Release
if ($LASTEXITCODE -ne 0) { throw "jbig2enc build failed" }

Write-Host "Installing jbig2enc..."
cmake --install $Jbig2BuildDir --config Release --prefix $Jbig2InstallDir
if ($LASTEXITCODE -ne 0) { throw "jbig2enc install failed" }

Write-Host "jbig2enc installed: $Jbig2InstallDir" -ForegroundColor Green

# ============================================================
# 6. pdfium (プリビルドバイナリ)
# ============================================================
Write-Step "pdfium (prebuilt binaries)"

$PdfiumDir = Join-Path $DepsDir "pdfium"

if (-not (Test-Path $PdfiumDir) -or -not (Get-ChildItem $PdfiumDir -Filter "*.dll" -Recurse -ErrorAction SilentlyContinue)) {
    New-Item -ItemType Directory -Path $PdfiumDir -Force | Out-Null

    Write-Host "Fetching latest release from $PdfiumRepo..."
    $headers = @{}
    if ($env:GITHUB_TOKEN) {
        $headers["Authorization"] = "token $env:GITHUB_TOKEN"
    }
    try {
        $releaseInfo = Invoke-RestMethod `
            -Uri "https://api.github.com/repos/$PdfiumRepo/releases/latest" `
            -Headers $headers
    } catch {
        Write-Host "Failed to fetch pdfium release info: $_" -ForegroundColor Red
        Write-Host "Set PdfiumRepo parameter or download pdfium manually to $PdfiumDir" -ForegroundColor Yellow
        Write-Host "Required: pdfium.dll in $PdfiumDir or a subdirectory" -ForegroundColor Yellow
        $releaseInfo = $null
    }

    if ($releaseInfo) {
        $asset = $releaseInfo.assets |
            Where-Object { $_.name -match "win.*x64" -or $_.name -match "windows.*x64" } |
            Select-Object -First 1

        if (-not $asset) {
            Write-Host "No Windows x64 asset found in release. Available assets:" -ForegroundColor Red
            $releaseInfo.assets | ForEach-Object { Write-Host "  - $($_.name)" }
            throw "Could not find Windows x64 pdfium binary"
        }

        $archivePath = Join-Path $DepsDir $asset.name
        $sizeMB = [math]::Round($asset.size / 1MB, 1)
        Write-Host "Downloading $($asset.name) (${sizeMB}MB)..."
        Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $archivePath -Headers $headers

        Write-Host "Extracting..."
        tar -xzf $archivePath -C $PdfiumDir
        Remove-Item $archivePath -Force
    }
}

# pdfium.dll の場所を検出
$pdfiumDll = Get-ChildItem $PdfiumDir -Filter "pdfium.dll" -Recurse -ErrorAction SilentlyContinue |
    Select-Object -First 1
if ($pdfiumDll) {
    $PdfiumLibDir = $pdfiumDll.DirectoryName
    Write-Host "pdfium found: $($pdfiumDll.FullName)" -ForegroundColor Green
} else {
    $PdfiumLibDir = $PdfiumDir
    Write-Host "WARNING: pdfium.dll not found in $PdfiumDir" -ForegroundColor Yellow
    Write-Host "Download pdfium manually and place pdfium.dll in $PdfiumDir" -ForegroundColor Yellow
}

# ============================================================
# 7. qpdf
# ============================================================
Write-Step "qpdf"

if (Get-Command qpdf -ErrorAction SilentlyContinue) {
    $qpdfVersion = (qpdf --version 2>&1 | Select-Object -First 1)
    Write-Host "qpdf found: $qpdfVersion" -ForegroundColor Green
} else {
    Write-Host "qpdf not found. Install with:" -ForegroundColor Yellow
    Write-Host "  winget install qpdf.qpdf" -ForegroundColor Yellow
    Write-Host "  scoop install qpdf" -ForegroundColor Yellow
    Write-Host "Note: qpdf is only needed if linearize=true in job config." -ForegroundColor Yellow
}

# ============================================================
# 8. env-windows.ps1 生成
# ============================================================
Write-Step "Generating env-windows.ps1"

$envContent = @"
# Auto-generated by setup-windows.ps1 at $(Get-Date -Format "yyyy-MM-dd HH:mm:ss")
# Usage: . .\scripts\env-windows.ps1

# --- build.rs: C++ shim compilation ---
`$env:JBIG2ENC_INCLUDE_PATH = "$Jbig2InstallDir\include"
`$env:JBIG2ENC_LIB_PATH     = "$Jbig2InstallDir\lib"
`$env:LEPTONICA_INCLUDE_PATH = "$LeptonicaInclude"

# --- pdfium-render: runtime dynamic loading ---
`$env:PDFIUM_DYNAMIC_LIB_PATH = "$PdfiumLibDir"

# --- leptonica-sys: vcpkg integration ---
`$env:VCPKG_ROOT       = "$VcpkgDir"
`$env:VCPKGRS_TRIPLET  = "$Triplet"

# --- DLLs and tools ---
`$env:PATH = "$PdfiumLibDir;`$env:PATH"

Write-Host "pdf_masking development environment loaded." -ForegroundColor Green
"@

$envScriptPath = Join-Path $PSScriptRoot "env-windows.ps1"
Set-Content -Path $envScriptPath -Value $envContent -Encoding UTF8
Write-Host "Generated: $envScriptPath" -ForegroundColor Green

# ============================================================
# Done
# ============================================================
Write-Step "Setup complete"

Write-Host @"

Next steps:
  1. Open a Developer Command Prompt or run vcvarsall.bat
  2. Load environment:  . .\scripts\env-windows.ps1
  3. Build:             cargo build
  4. Test:              cargo test

Tip: Add '. $envScriptPath' to your PowerShell profile.
"@
