# Stage 1: Builder (Compilación nativa en Rust)
FROM rust:1-slim-bookworm AS builder

WORKDIR /build

# Copiar manifiestos y crear archivos dummy para cachear dependencias
COPY Cargo.toml Cargo.lock* ./
RUN mkdir -p src && \
    echo "fn main() {}" > src/main.rs && \
    touch src/lib.rs && \
    cargo build --release && \
    rm -rf src

# Copiar código fuente y compilar el binario optimizado
COPY src ./src
RUN touch src/lib.rs src/main.rs && \
    cargo build --release

# Stage 2: Runtime mínimo (Debian Bookworm Slim)
FROM debian:bookworm-slim

# Instalar ffmpeg, aria2, certificados y herramientas CLI externas (yt-dlp, instaloader, spotdl)
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ffmpeg \
        aria2 \
        ca-certificates \
        curl \
        python3 \
        python3-pip && \
    pip3 install --no-cache-dir --break-system-packages yt-dlp instaloader spotdl && \
    rm -rf /var/lib/apt/lists/* /root/.cache/pip

WORKDIR /app

# Crear usuario no raíz por seguridad
RUN useradd -m -u 1000 botuser && \
    mkdir -p /downloads && \
    chown -R botuser:botuser /app /downloads

# Copiar binario compilado y entrypoint
COPY --from=builder /build/target/release/telegram-downloader-bot /app/telegram-downloader-bot
COPY entrypoint.sh /app/entrypoint.sh
RUN chmod +x /app/telegram-downloader-bot /app/entrypoint.sh && \
    chown -R botuser:botuser /app

# Cambiar a usuario no raíz
USER botuser

# Puerto para el asistente web de configuración y panel de estado
EXPOSE 8080

# Declarar volumen para descargas
VOLUME ["/downloads"]

ENTRYPOINT ["/app/entrypoint.sh"]
