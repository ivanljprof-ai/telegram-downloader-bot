#!/bin/bash
echo "============================================================"
echo "🚀 Iniciando Bot Multidescargas de Telegram (Mac / Linux)"
echo "============================================================"
echo ""

if ! command -v docker &> /dev/null; then
    echo "❌ Docker no está instalado o no se encuentra en ejecucion."
    echo "ℹ️ Por favor, instala Docker desde: https://www.docker.com/"
    exit 1
fi

echo "🐳 Construyendo e iniciando contenedores Docker..."
docker-compose up -d --build

# Obtener IP real del equipo/servidor mediante Python o fallback
SERVER_IP=$(python3 -c "import socket; s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.connect(('8.8.8.8', 80)); print(s.getsockname()[0]); s.close()" 2>/dev/null)

if [ -z "$SERVER_IP" ]; then
    SERVER_IP="127.0.0.1"
fi

URL="http://${SERVER_IP}:8080"

echo ""
echo "============================================================"
echo "🌐 ASISTENTE WEB LISTO - ENLACE DE ACCESO DIRECTO:"
echo "============================================================"
echo "👉 Conéctate desde cualquier navegador en:"
echo "   $URL"
echo "============================================================"
echo ""

echo "🌐 Abriendo $URL en tu navegador..."

if command -v open &> /dev/null; then
    open "$URL"
elif command -v xdg-open &> /dev/null; then
    xdg-open "$URL"
else
    echo "ℹ️ Abre manualmente en tu navegador: $URL"
fi

echo ""
echo "🎉 ¡Listo! Accede a la dirección $URL para configurar el bot."
