@echo off
rem Removes Glint, its file associations and its PATH entry.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\uninstall.ps1"
echo.
pause
