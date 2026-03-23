@echo off
setlocal
cd /d "%~dp0"

set "LOG=%~dp0vite-live.log"
set "ERR=%~dp0vite-live.err.log"

del /q "%LOG%" "%ERR%" 2>nul

start "frontend-modern-vite" /min cmd /c "npm run dev 1> \"%LOG%\" 2> \"%ERR%\""

echo started
