<#
.SYNOPSIS
    pdf_masking の Windows 開発環境セットアップスクリプト。

.DESCRIPTION
    ネイティブ依存ライブラリ (pdfium) をダウンロードし、
    環境変数設定スクリプト (env-windows.ps1) を生成する。

    leptonica と jbig2enc は pure Rust crate のため、C ライブラリの
    インストールは不要。

    初回のみ実行が必要。以降は各ターミナルセッションで env-windows.ps1 を読み込む。

.PARAMETER PdfiumRepo
    pdfium プリビルドバイナリの GitHub リポジトリ (owner/repo 形式)。

.EXAMPLE
    .\scripts\setup-windows.ps1
    . .\scripts\env-windows.ps1
    cargo build
#>

param(
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
$ok = (Assert-Command "cargo" "https://rustup.rs/") -and $ok

if (-not $ok) {
    Write-Host "`nMissing prerequisites. Install them and re-run." -ForegroundColor Red
    exit 1
}

# ============================================================
# 2. deps/ ディレクトリ作成
# ============================================================
New-Item -ItemType Directory -Path $DepsDir -Force | Out-Null

# ============================================================
# 3. pdfium (プリビルドバイナリ)
# ============================================================
Write-Step "pdfium (prebuilt binaries)"

$PdfiumDir = Join-Path $DepsDir "pdfium"

if (-not (Test-Path $PdfiumDir) -or -not (Get-ChildItem $PdfiumDir -Filter "pdfium.dll" -Recurse -ErrorAction SilentlyContinue)) {
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

        $expectedSha256 = $env:PDFIUM_EXPECTED_SHA256
        if ($expectedSha256) {
            Write-Host "Verifying SHA-256 hash of downloaded pdfium archive..."
            $fileHash = (Get-FileHash -Path $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
            $normalizedExpected = $expectedSha256.Trim().ToLowerInvariant()
            if ($fileHash -ne $normalizedExpected) {
                Write-Host "SHA-256 mismatch for downloaded pdfium archive!" -ForegroundColor Red
                Write-Host "Expected: $normalizedExpected" -ForegroundColor Red
                Write-Host "Actual  : $fileHash" -ForegroundColor Red
                Remove-Item $archivePath -Force -ErrorAction SilentlyContinue
                throw "Aborting pdfium setup due to failed integrity check."
            }
            Write-Host "SHA-256 verification succeeded." -ForegroundColor Green
        } else {
            Write-Host "WARNING: PDFIUM_EXPECTED_SHA256 is not set; skipping integrity verification." -ForegroundColor Yellow
        }

        Write-Host "Extracting..."
        $assetNameLower = $asset.name.ToLowerInvariant()
        try {
            if ($assetNameLower -like "*.zip") {
                Expand-Archive -Path $archivePath -DestinationPath $PdfiumDir -Force
            } elseif ($assetNameLower -like "*.tar.gz" -or $assetNameLower -like "*.tgz") {
                tar -xzf $archivePath -C $PdfiumDir
                if ($LASTEXITCODE -ne 0) {
                    throw "tar extraction failed with exit code $LASTEXITCODE"
                }
            } else {
                throw "Unsupported pdfium archive format: $($asset.name)"
            }
        } catch {
            Write-Host "Failed to extract pdfium archive: $_" -ForegroundColor Red
            throw
        }
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
# 4. qpdf
# ============================================================
Write-Step "qpdf"

if (Get-Command qpdf -ErrorAction SilentlyContinue) {
    $qpdfVersion = (qpdf --version 2>&1 | Select-Object -First 1)
    Write-Host "qpdf found: $qpdfVersion" -ForegroundColor Green
} else {
    Write-Host "qpdf not found. Install with:" -ForegroundColor Yellow
    Write-Host "  winget install qpdf.qpdf" -ForegroundColor Yellow
    Write-Host "  scoop install qpdf" -ForegroundColor Yellow
    Write-Host "Note: qpdf is required for the default settings (linearize: true). You can omit it only if you set linearize=false in your job config." -ForegroundColor Yellow
}

# ============================================================
# 5. env-windows.ps1 生成
# ============================================================
Write-Step "Generating env-windows.ps1"

$envContent = @"
# Auto-generated by setup-windows.ps1 at $(Get-Date -Format "yyyy-MM-dd HH:mm:ss")
# Usage: . .\scripts\env-windows.ps1

# --- pdfium-render: runtime dynamic loading ---
`$env:PDFIUM_DYNAMIC_LIB_PATH = "$PdfiumLibDir"

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
  1. Load environment:  . .\scripts\env-windows.ps1
  2. Build:             cargo build
  3. Test:              cargo test

Note: leptonica and jbig2enc are pure Rust crates; no C/C++ compiler is required.
Tip: Add '. $envScriptPath' to your PowerShell profile.
"@
