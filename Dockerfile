FROM python:3.10-slim

# Instalar ffmpeg, aria2 y dependencias necesarias del sistema
RUN apt-get update && \
    apt-get install -y --no-install-recommends ffmpeg aria2 ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Establecer directorio de trabajo
WORKDIR /app

# Copiar e instalar requerimientos de Python
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Crear usuario no raíz por seguridad
RUN useradd -m -u 1000 botuser && \
    mkdir -p /downloads && \
    chown -R botuser:botuser /app /downloads

# Copiar el código fuente y scripts
COPY main.py setup_web.py entrypoint.sh /app/
RUN chmod +x /app/entrypoint.sh && \
    chown -R botuser:botuser /app

# Cambiar a usuario no raíz
USER botuser

# Puerto para el asistente de configuración web inicial
EXPOSE 8080

# Declarar volumen para descargas
VOLUME ["/downloads"]

ENTRYPOINT ["/app/entrypoint.sh"]

