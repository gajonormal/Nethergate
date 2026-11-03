@echo off
setlocal
set "NETHERGATE_HOME=%~dp0"
if not exist "%~dp0src-tauri\target\debug\nethergate-desktop.exe" (
  echo O executavel ainda nao foi compilado. Consulta o README.
  pause
  exit /b 1
)
start "" "%~dp0src-tauri\target\debug\nethergate-desktop.exe"
