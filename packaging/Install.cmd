@echo off
rem Installs Glint for the current user. No admin rights are needed, apart from one optional UAC prompt that makes Glint your default app.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\install.ps1" %*
echo.
pause
