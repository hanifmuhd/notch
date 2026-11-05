# Installs or updates Claude Notch for the current user (no admin rights). Run in PowerShell:
#   irm https://raw.githubusercontent.com/hanifmuhd/notch/main/install.ps1 | iex
$ErrorActionPreference = 'Stop'
$setup = Join-Path $env:TEMP 'ClaudeNotch-setup.exe'
Write-Host 'Downloading Claude Notch...'
curl.exe -fL -o $setup 'https://github.com/hanifmuhd/notch/releases/latest/download/ClaudeNotch-setup.exe'
if ($LASTEXITCODE -ne 0) { throw 'Download failed. Check your internet connection and try again.' }
Write-Host 'Installing...'
Start-Process $setup -ArgumentList '/S' -Wait
Remove-Item $setup
Start-Process "$env:LOCALAPPDATA\Claude Notch\claude-notch.exe"
Write-Host 'Claude Notch is installed and running at the top of your screen.'
