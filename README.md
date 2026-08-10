# 🚀 Telegram Multi-Downloader & Utility Bot

Un bot de Telegram universal, completo y desplegable en Docker para descargar audio y vídeo de más de 15 plataformas (YouTube, Instagram, Spotify, TikTok, Twitter/X, Twitch, Reddit, etc.), consultar metadatos y administrar el servidor.

![Python](https://img.shields.io/badge/Python-3.10+-blue.svg)
![Docker](https://img.shields.io/badge/Docker-Windows%20%7C%20Mac%20%7C%20Linux-blue.svg)
![yt-dlp](https://img.shields.io/badge/yt--dlp-Latest-red.svg)
![License](https://img.shields.io/badge/License-MIT-green.svg)

---

## ⚡ Instalación en 1 Solo Clic (Universal)

El proyecto incluye ejecutables automatizados para que cualquier persona en **Windows, Mac o Linux** lo ponga en marcha sin escribir comandos.

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

- 📊 **Barras de Progreso en Tiempo Real**: Visualización de descarga tipo `[██████░░░░] 60%`.
- 📹 **Descarga Multimedios**: YouTube (MP4 / MP3), TikTok, Twitter/X, Facebook, Reddit, Pinterest, Twitch, Vimeo...
- 🎵 **Integración con Spotify**: Descarga canciones, álbumes y listas completas con portadas e información ID3 integradas mediante `spotDL`.
- 📸 **Instagram Downloader**: Descarga de posts, reels e historias activas (soporta `cookies.txt` y logins privados).
- 🌐 **Asistente Web de Configuración**: Interfaz gráfica en el puerto `8080` de un solo uso que se auto-apaga tras guardar para no consumir recursos del sistema.
- 📱 **Menú Interactivo `/help`**: Centro de ayuda visual basado en botones inline navegables sin saturar el chat.
- 🛠️ **Herramientas del Servidor**:
  - `/status` o `/stats`: Monitorización de CPU, Memoria RAM, uso de disco y tiempo activo (*uptime*).
  - `/clean`: Consulta el tamaño del almacenamiento de descargas y permite vaciarlo con `/clean confirm`.
  - `/info`: Obtención de metadatos (título, duración, canal, visitas) sin descargar el archivo.
  - `/ping`: Medición de latencia de respuesta del bot.

---

## ⚙️ Configuración Manual (Alternativa)

Si prefieres configurar mediante comandos tradicionales de Docker:

```bash
docker-compose up -d --build
```
Y luego abre en tu navegador `http://localhost:8080`.

---

## 📋 Lista de Comandos

| Comando | Descripción |
| :--- | :--- |
| `/start` | Mensaje de bienvenida e información inicial |
| `/help` / `/ayuda` | Menú visual e interactivo de ayuda |
| `/mp3 <URL>` / `/audio <URL>` | Descarga directa de audio en MP3 |
| `/insta <usuario>` | Descarga de historias activas de Instagram |
| `/spotify <URL>` | Descarga de música de Spotify (pistas, listas, álbumes) |
| `/info <URL>` | Consulta metadatos de un enlace sin descargarlo |
| `/status` / `/stats` | Estado del servidor (CPU, RAM, Disco, Uptime) |
| `/clean` | Consultar espacio ocupado en el disco de descargas |
| `/clean confirm` | Vaciar la carpeta de descargas |
| `/ping` | Medir la latencia de respuesta del bot |

---

## 📜 Licencia

Este proyecto está bajo la Licencia MIT. ¡Siéntete libre de colaborar o hacer fork!
