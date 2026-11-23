#!/bin/bash
set -e

while true; do
    # Comprobar si el archivo .env existe y contiene TELEGRAM_TOKEN no vacío
    if [ ! -f /app/.env ] || ! grep -q "TELEGRAM_TOKEN=" /app/.env || [ -z "$(grep TELEGRAM_TOKEN /app/.env | cut -d'=' -f2)" ]; then
        echo "=========================================================="
        echo "⚠️ BOT NO CONFIGURADO"
        echo "🌐 Iniciando servidor web de configuración en http://localhost:8080"
        echo "=========================================================="
        /app/telegram-downloader-bot setup || true
    fi

    echo "=========================================================="
    echo "🚀 Arrancando Bot Multidescargas de Telegram (Motor Rust)..."
    echo "=========================================================="
    /app/telegram-downloader-bot || true

    echo "🔄 Reiniciando ciclo del contenedor..."
    sleep 1
done
