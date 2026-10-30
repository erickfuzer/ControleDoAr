# Compila o app em modo release e gera o instalador em dist\.
# Uso: powershell -ExecutionPolicy Bypass -File installer\build.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = (Select-String -Path "Cargo.toml" -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches.Groups[1].Value
Write-Host "Controle do Ar $version"

cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build falhou" }

# makensis: no PATH, na instalação padrão do NSIS ou na cópia que o Tauri baixava.
$makensis = (Get-Command makensis -ErrorAction SilentlyContinue).Source
foreach ($candidate in @("${env:ProgramFiles(x86)}\NSIS\makensis.exe", "$env:ProgramFiles\NSIS\makensis.exe", "$env:LOCALAPPDATA\tauri\NSIS\makensis.exe")) {
    if (-not $makensis -and (Test-Path $candidate)) { $makensis = $candidate }
}
if (-not $makensis) { throw "NSIS não encontrado. Instale com: winget install NSIS.NSIS" }

New-Item -ItemType Directory -Force "dist" | Out-Null
& $makensis "/DVERSION=$version" "installer\ControleDoAr.nsi"
if ($LASTEXITCODE -ne 0) { throw "makensis falhou" }
Get-ChildItem "dist\*$version*setup.exe" | Select-Object Name, @{n = "MB"; e = { [math]::Round($_.Length / 1MB, 2) } }
