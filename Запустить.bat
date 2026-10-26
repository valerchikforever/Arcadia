@echo off
chcp 65001 >nul
cd /d "%~dp0"
where node >/dev/null 2>/dev/null || (echo Установите Node.js с https://nodejs.org и запустите снова & pause & exit /b 1)
if not exist node_modules (echo Устанавливаю зависимости... & call npm install)
call npm start
