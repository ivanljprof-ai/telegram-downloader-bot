@echo off
echo ============================================================
echo 🚀 Iniciando Bot Multidescargas de Telegram (Windows)
echo ============================================================
echo.

docker --version >nul 2>&1
if %errorlevel% neq 0 (
    echo ❌ Docker Desktop no esta instalado o no esta en ejecucion.
    echo ℹ️ Por favor, instala y abre Docker Desktop primero: https://www.docker.com/products/docker-desktop/
    pause
    exit /b 1
)

echo 🐳 Construyendo e iniciando contenedor Docker...
docker-compose up -d --build

for /f "delims=" %%i in ('python -c "import socket; s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.connect(('8.8.8.8', 80)); print(s.getsockname()[0]); s.close()" 2^>nul') do set SERVER_IP=%%i

if "%SERVER_IP%"=="" (
    for /f "tokens=2 delims=:" %%a in ('ipconfig ^| findstr /c:"IPv4"') do (
        set SERVER_IP=%%a
        goto :ip_found
    )
    :ip_found
    set SERVER_IP=%SERVER_IP: =%
)

if "%SERVER_IP%"=="" set SERVER_IP=127.0.0.1

echo.
echo ============================================================
echo 🌐 ASISTENTE WEB LISTO - ENLACE DE ACCESO DIRECTO:
echo ============================================================
echo 👉 Conectate desde cualquier navegador en:
echo    http://%SERVER_IP%:8080
echo ============================================================
echo.

start http://%SERVER_IP%:8080

echo.
echo 🎉 ¡Listo! Accede a la direccion web mostrada arriba.
pause
