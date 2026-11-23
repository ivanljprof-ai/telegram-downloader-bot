# 🚀 Telegram Multi-Downloader & Utility Bot

Un bot de Telegram universal, completo, ultrarrápido y desplegable en Docker, escrito íntegramente en **Rust** para ofrecer un consumo mínimo de memoria y CPU, capaz de descargar audio y vídeo de más de 15 plataformas (YouTube, Instagram, Spotify, TikTok, Twitter/X, Twitch, Reddit, etc.), consultar metadatos y administrar el servidor.

![Rust](https://img.shields.io/badge/Rust-2024%20Edition-orange.svg)
![Tokio](https://img.shields.io/badge/Async-Tokio-blueviolet.svg)
![Docker](https://img.shields.io/badge/Docker-Windows%20%7C%20Mac%20%7C%20Linux-blue.svg)
![yt-dlp](https://img.shields.io/badge/yt--dlp-Latest-red.svg)
![License](https://img.shields.io/badge/License-MIT-green.svg)

---

## 🦀 Migración a Rust: Consumo Mínimo de Recursos

El núcleo del bot y el panel web han sido reescritos por completo de Python a **Rust** (Edición 2024) con el objetivo de minimizar al máximo la huella de recursos en servidores domésticos, Raspberry Pi, Jetson o VPS de bajos recursos:

- **Consumo ultrabajo de RAM y CPU**: binario nativo compilado con `opt-level = "z"`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"` y `strip = true`, reduciendo el consumo en reposo a unos pocos megabytes de RSS.
- **Sin recolector de basura (Zero GC overhead)**: ejecución determinista sin pausas de Garbage Collector ni sobrecarga de intérprete.
- **Runtime asíncrono nativo (`Tokio` + `Axum` + `Reqwest/Rustls`)**: manejo concurrente de long-polling de Telegram, descargas externas en streaming, asistente web SSE y panel de estado con mínima huella de hilos y sin dependencias de OpenSSL.
- **Imagen Docker multi-stage ligera**: compilación aislada en `rust:1-slim-bookworm` y ejecución final en `debian:bookworm-slim` con únicamente el binario nativo compilado y las herramientas CLI externas (`ffmpeg`, `aria2`, `yt-dlp`, `instaloader`, `spotdl`).

---

## ⚡ Instalación en 1 Solo Clic (Universal)

El proyecto incluye ejecutables automatizados para que cualquier persona en **Windows, Mac o Linux** lo ponga en marcha sin escribir comandos ni necesitar Python instalado en el equipo anfitrión.

### 🌐 En Windows:
1. Asegúrate de tener [Docker Desktop](https://www.docker.com/products/docker-desktop/) instalado y abierto.
2. Haz **doble clic** en `start.bat`.
3. Se abrirá automáticamente la web de configuración en tu navegador (`http://localhost:8080`).
4. Rellena tu Token de Telegram e ID de Usuario y haz clic en **Guardar**. ¡Listo!

### 🍏 En Mac / Linux:
1. Abre tu terminal en la carpeta del proyecto.
2. Ejecuta:
   ```bash
   ./start.sh
   ```
3. Se abrirá automáticamente la web de configuración (`http://localhost:8080`) para que introduzcas tus credenciales.

---

## ✨ Características Principales

- 📊 **Barras de Progreso en Tiempo Real**: Visualización de descarga tipo `[██████░░░░] 60.0%` con detección automática de `aria2c` para descargas aceleradas.
- 📹 **Descarga Multimedios**: YouTube (MP4 / MP3), TikTok, Twitter/X, Facebook, Reddit, Pinterest, Twitch, SoundCloud, Vimeo...
- 🎵 **Integración con Spotify**: Descarga canciones, álbumes y listas completas con portadas e información ID3 integradas mediante `spotDL` (con fallback automático a `yt-dlp`).
- 📸 **Instagram Downloader**: Descarga de posts, reels e historias activas mediante `instaloader` / `yt-dlp` (soporta `cookies.txt` y credenciales opcionales).
- 🌐 **Asistente Web de Configuración y Panel de Estado**: Interfaz gráfica en el puerto `8080` con progreso SSE en tiempo real durante la configuración inicial y panel operativo con CPU, RAM del sistema, RAM del bot (RSS), Uptime y botón de reconfiguración (`/reconfig`).
- 📱 **Menú Interactivo `/help`**: Centro de ayuda visual basado en botones inline navegables sin saturar el chat.
- 🛠️ **Herramientas del Servidor**:
  - `/status` o `/stats`: Monitorización de CPU, Memoria RAM del sistema y del proceso, uso de disco y tiempo activo (*uptime*).
  - `/clean` o `/limpiar`: Consulta el tamaño del almacenamiento de descargas y permite vaciarlo con `/clean confirm`.
  - `/info`: Obtención de metadatos (título, duración, canal, visitas) sin descargar el archivo.
  - `/ping`: Medición de latencia de respuesta del bot en milisegundos.

---

## ⚙️ Configuración Manual (Alternativa)

Si prefieres configurar mediante comandos tradicionales de Docker:

```bash
docker compose up -d --build
```
Y luego abre en tu navegador `http://localhost:8080`.

También puedes compilar y ejecutar el binario nativo en local:

```bash
cargo build --release
./target/release/telegram-downloader-bot
```

---

## 📋 Lista de Comandos

| Comando | Descripción |
| :--- | :--- |
| `/start` | Mensaje de bienvenida e información inicial |
| `/help` / `/ayuda` | Menú visual e interactivo de ayuda |
| `/mp3 <URL>` / `/audio <URL>` | Descarga directa de audio en MP3 |
| `/insta <usuario>` / `/historias <usuario>` | Descarga de historias activas de Instagram |
| `/spotify <URL>` / `/sp <URL>` | Descarga de música de Spotify (pistas, listas, álbumes) |
| `/info <URL>` | Consulta metadatos de un enlace sin descargarlo |
| `/status` / `/stats` | Estado del servidor (CPU, RAM Sistema, RAM Bot, Disco, Uptime) |
| `/clean` / `/limpiar` | Consultar espacio ocupado en el disco de descargas |
| `/clean confirm` | Vaciar la carpeta de descargas |
| `/ping` | Medir la latencia de respuesta del bot |

---

## 📜 Licencia

Este proyecto está bajo la Licencia MIT. ¡Siéntete libre de colaborar o hacer fork!
