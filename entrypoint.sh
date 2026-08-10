#!/bin/bash
set -e

while true; do
    # Comprobar si el archivo .env existe y contiene TELEGRAM_TOKEN no vacío
    if [ ! -f /app/.env ] || ! grep -q "TELEGRAM_TOKEN=" /app/.env || [ -z "$(grep TELEGRAM_TOKEN /app/.env | cut -d'=' -f2)" ]; then
        echo "=========================================================="
        echo "⚠️ BOT NO CONFIGURADO"
        echo "🌐 Iniciando servidor web de configuración en http://localhost:8080"
        echo "=========================================================="
        python /app/setup_web.py || true
    fi

    echo "=========================================================="
    echo "🚀 Arrancando Bot Multidescargas de Telegram..."
    echo "=========================================================="
    python /app/main.py || true

    echo "🔄 Reiniciando ciclo del contenedor..."
    sleep 1
done
