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
if docker compose version &> /dev/null; then
    docker compose up -d --build
else
    docker-compose up -d --build
fi

# Detectar IP LAN del equipo/servidor sin depender de Python en el host
SERVER_IP=""
if command -v ipconfig &> /dev/null; then
    IFACE=$(route -n get 8.8.8.8 2>/dev/null | awk '/interface:/{print $2}')
    if [ -n "$IFACE" ]; then
        SERVER_IP=$(ipconfig getifaddr "$IFACE" 2>/dev/null || true)
    fi
    [ -z "$SERVER_IP" ] && SERVER_IP=$(ipconfig getifaddr en0 2>/dev/null || true)
elif command -v ip &> /dev/null; then
    SERVER_IP=$(ip route get 8.8.8.8 2>/dev/null | awk '{for(i=1;i<=NF;i++) if($i=="src") print $(i+1)}' | head -n1 || true)
elif command -v hostname &> /dev/null; then
    SERVER_IP=$(hostname -I 2>/dev/null | awk '{print $1}' || true)
fi

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
