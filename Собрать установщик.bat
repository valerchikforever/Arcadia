@echo off
chcp 65001 >nul
cd /d "%~dp0"
where cargo >nul 2>nul || (echo Установите Rust с https://rustup.rs и запустите снова & pause & exit /b 1)
where node >nul 2>nul || (echo Установите Node.js с https://nodejs.org и запустите снова & pause & exit /b 1)
if not exist node_modules (echo Устанавливаю зависимости... & call npm install)
call npm run build
echo.
echo Готово: src-tauri\target\release\bundle\nsis
pause
