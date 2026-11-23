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
docker compose version >nul 2>&1
if %errorlevel% equ 0 (
    docker compose up -d --build
) else (
    docker-compose up -d --build
)

for /f "usebackq delims=" %%i in (`powershell -NoProfile -Command "(Get-NetIPAddress -AddressFamily IPv4 | Where-Object { $_.InterfaceAlias -notmatch 'Loopback|vEthernet|Docker|WSL' -and $_.IPAddress -notlike '169.254.*' } | Select-Object -First 1 -ExpandProperty IPAddress)" 2^>nul`) do set SERVER_IP=%%i

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
