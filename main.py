import os
import re
import sys
import logging
import asyncio
import time
from typing import Optional
from telegram import Update, InlineKeyboardButton, InlineKeyboardMarkup
from telegram.ext import (
    Application,
    CommandHandler,
    MessageHandler,
    CallbackQueryHandler,
    filters,
    ContextTypes,
)
import instaloader
import psutil

BOT_START_TIME = time.time()

# Configuración de logging
logging.basicConfig(
    format="%(asctime)s - %(name)s - %(levelname)s - %(message)s",
    level=logging.INFO,
)
logger = logging.getLogger(__name__)

def load_env():
    """Carga las variables del archivo .env guardado en el disco si existen."""
    for env_path in [
        os.path.join(os.path.dirname(__file__), ".env"),
        "/app/.env",
        ".env"
    ]:
        if os.path.exists(env_path):
            try:
                with open(env_path, "r", encoding="utf-8") as f:
                    for line in f:
                        line = line.strip()
                        if line and not line.startswith("#") and "=" in line:
                            k, v = line.split("=", 1)
                            os.environ[k.strip()] = v.strip()
                logger.info(f"Archivo .env cargado correctamente desde {env_path}")
                break
            except Exception as e:
                logger.warning(f"Error al leer .env en {env_path}: {e}")

load_env()

# Configuración de entorno
TELEGRAM_TOKEN = os.getenv("TELEGRAM_TOKEN")
ALLOWED_USER_ID = os.getenv("ALLOWED_USER_ID")
DOWNLOAD_DIR = os.getenv("DOWNLOAD_DIR", "/downloads")

INSTAGRAM_USER = os.getenv("INSTAGRAM_USER")
INSTAGRAM_PASSWORD = os.getenv("INSTAGRAM_PASSWORD")

# Expresiones regulares para detectar URLs
URL_REGEX = re.compile(
    r"https?://(?:www\.)?[-a-zA-Z0-9@:%._\+~#=]{1,256}\.[a-zA-Z0-9()]{1,6}\b(?:[-a-zA-Z0-9()@:%_\+.~#?&//=]*)"
)

# Detector de Spotify
SPOTIFY_REGEX = re.compile(
    r"https?://(?:open|play)\.spotify\.com/(?:track|album|playlist|artist)/[a-zA-Z0-9?=&_-]+"
)

# Detectores específicos de Instagram
INSTAGRAM_STORIES_REGEX = re.compile(
    r"https?://(?:www\.)?instagram\.com/stories/([a-zA-Z0-9_.]+)"
)
INSTAGRAM_POST_REGEX = re.compile(
    r"https?://(?:www\.)?instagram\.com/(?:p|reel|reels|tv)/([a-zA-Z0-9_-]+)"
)
INSTAGRAM_PROFILE_REGEX = re.compile(
    r"https?://(?:www\.)?instagram\.com/([a-zA-Z0-9_.]+)/?"
)

TIKTOK_REGEX = re.compile(r"https?://(?:www\.|vm\.|vt\.)?tiktok\.com/.*")
TWITTER_REGEX = re.compile(r"https?://(?:www\.)?(?:twitter|x)\.com/.*")
REDDIT_REGEX = re.compile(r"https?://(?:www\.|v\.)?reddit\.com/.*")
FACEBOOK_REGEX = re.compile(r"https?://(?:www\.|m\.|fb\.)?(?:facebook\.com|fb\.watch)/.*")
PINTEREST_REGEX = re.compile(r"https?://(?:www\.|pin\.)?pinterest\.[a-z.]+/.*")
TWITCH_REGEX = re.compile(r"https?://(?:www\.|clips\.)?twitch\.tv/.*")
SOUNDCLOUD_REGEX = re.compile(r"https?://(?:www\.|m\.)?soundcloud\.com/.*")
VIMEO_REGEX = re.compile(r"https?://(?:www\.)?vimeo\.com/.*")

# Almacenamiento en memoria para URLs pendientes de selección de formato
PENDING_URLS = {}


def get_instaloader_instance() -> instaloader.Instaloader:
    """Crea y configura una instancia de Instaloader."""
    L = instaloader.Instaloader(
        download_pictures=True,
        download_videos=True,
        download_video_thumbnails=False,
        download_geotags=False,
        download_comments=False,
        save_metadata=False,
        dirname_pattern=os.path.join(DOWNLOAD_DIR, "{target}"),
        filename_pattern="{date_utc:_%Y%m%d_%H%M%S}_{shortcode}",
        quiet=True,
    )
    
    # Buscar posibles archivos de cookies y probar cada uno hasta tener éxito
    candidate_cookies = [
        "/app/cookies.txt",
        "/downloads/www.instagram.com_cookies.txt",
        "/downloads/cookies.txt",
        os.path.join(os.path.dirname(__file__), "cookies.txt"),
    ]
    
    for cookies_path in candidate_cookies:
        if os.path.exists(cookies_path):
            try:
                import http.cookiejar
                cj = http.cookiejar.MozillaCookieJar(cookies_path)
                cj.load(ignore_discard=True, ignore_expires=True)
                user_id = None
                cookie_count = 0
                for cookie in cj:
                    if "instagram.com" in cookie.domain:
                        L.context._session.cookies.set(
                            cookie.name, cookie.value, domain=cookie.domain, path=cookie.path
                        )
                        cookie_count += 1
                        if cookie.name == "ds_user_id":
                            user_id = cookie.value
                
                if cookie_count > 0:
                    logger.info(f"Cookies de Instagram cargadas correctamente desde: {cookies_path}")
                    break
            except Exception as e:
                logger.warning(f"No se pudieron cargar cookies desde {cookies_path}: {e}")

    if INSTAGRAM_USER and INSTAGRAM_PASSWORD:
        try:
            L.login(INSTAGRAM_USER, INSTAGRAM_PASSWORD)
            logger.info(f"Sesión iniciada en Instagram como: {INSTAGRAM_USER}")
        except Exception as e:
            logger.warning(f"No se pudo iniciar sesión en Instagram ({INSTAGRAM_USER}): {e}")
    return L


def is_user_allowed(user_id: int) -> bool:
    """Verifica si el usuario está autorizado si se definió ALLOWED_USER_ID."""
    if not ALLOWED_USER_ID:
        return True
    try:
        return user_id == int(ALLOWED_USER_ID)
    except ValueError:
        return True


async def start_command(update: Update, context: ContextTypes.DEFAULT_TYPE) -> None:
    """Manejador para el comando /start."""
    user = update.effective_user
    logger.info(f"Usuario {user.id} ({user.username}) ejecutó /start")
    await update.message.reply_text(
        f"👋 **¡Hola, {user.first_name}! Bienvenido al Bot Multidescargas & Utilidades** 🚀\n\n"
        "Conmigo puedes descargar vídeo y audio de prácticamente cualquier plataforma web, gestionar almacenamiento y consultar información. ⚡\n\n"
        "📌 **Redes soportadas:**\n"
        "• 📹 **YouTube**, TikTok, Instagram (Posts/Reels/Historias), Spotify, Twitter/X, Facebook, Reddit, Pinterest, Twitch, SoundCloud, Vimeo...\n\n"
        "💡 **¿Cómo empezar?**\n"
        "• ¡Simplemente pega un enlace en el chat y elige el formato! 📥\n"
        "• Escribe `/help` o `/ayuda` para ver la lista completa de comandos disponibles.",
        parse_mode="Markdown",
    )


def get_help_keyboard() -> InlineKeyboardMarkup:
    """Devuelve el teclado Inline para la ayuda interactiva."""
    keyboard = [
        [
            InlineKeyboardButton("📥 Descargas", callback_data="help_downloads"),
            InlineKeyboardButton("🌐 Redes Soportadas", callback_data="help_networks"),
        ],
        [
            InlineKeyboardButton("🛠️ Sistema", callback_data="help_system"),
            InlineKeyboardButton("💡 Ejemplos", callback_data="help_examples"),
        ],
        [
            InlineKeyboardButton("🏠 Menú Principal", callback_data="help_main"),
        ],
    ]
    return InlineKeyboardMarkup(keyboard)


def get_help_text(section: str = "main") -> str:
    """Devuelve los textos formateados con diseño visual para cada sección de ayuda."""
    if section == "downloads":
        return (
            "━━━━━━━━━━━━━━━━━━━━━\n"
            "  📥 **DESCARGAS Y FORMATOS** 📹🎵\n"
            "━━━━━━━━━━━━━━━━━━━━━\n\n"
            "• **Enviar Enlace Directo**:\n"
            "  Envía cualquier URL en el chat y elige:\n"
            "  ├ 📹 **MP4**: Máxima resolución disponible\n"
            "  └ 🎵 **MP3**: Extracción de audio pura\n\n"
            "• `/mp3 <enlace>` o `/audio <enlace>`:\n"
            "  Fuerza la descarga directa en audio MP3 sin preguntar.\n\n"
            "• `/insta <usuario>` o `/historias <usuario>`:\n"
            "  Descarga las historias activas de un perfil público de Instagram. 📸\n\n"
            "• `/spotify <enlace>` o `/sp <enlace>`:\n"
            "  Descarga canciones, álbumes o playlists de Spotify con carátula y tags. 🎵\n\n"
            "• `/info <enlace>`:\n"
            "  Muestra título, duración, canal y vistas sin descargar el archivo. 🔍\n\n"
            "📊 *Todas las descargas incluyen barra de progreso visual en tiempo real.*"
        )
    elif section == "networks":
        return (
            "━━━━━━━━━━━━━━━━━━━━━\n"
            "  🌐 **REDES Y PLATAFORMAS SOPORTADAS** 📱\n"
            "━━━━━━━━━━━━━━━━━━━━━\n\n"
            "🔴 **Vídeo & Streaming:**\n"
            "• YouTube, Twitch, Vimeo, Dailymotion, Streamable, Kick, Rumble\n\n"
            "📸 **Redes Sociales:**\n"
            "• Instagram (Posts, Reels, Historias, IGTV)\n"
            "• TikTok, Twitter / X, Facebook, Reddit, Pinterest, Douyin\n\n"
            "🎵 **Música & Audio:**\n"
            "• Spotify (Pistas, Álbumes, Playlists), SoundCloud, Mixcloud\n\n"
            "🔗 *Simplemente copia cualquier enlace y pégalo en el chat.*"
        )
    elif section == "system":
        return (
            "━━━━━━━━━━━━━━━━━━━━━\n"
            "  🛠️ **COMANDOS DE SISTEMA Y UTILIDADES** 📊\n"
            "━━━━━━━━━━━━━━━━━━━━━\n\n"
            "• `/status` o `/stats`:\n"
            "  Muestra CPU, RAM, uso de Disco y tiempo activo (*uptime*) del bot. 💻\n\n"
            "• `/clean` o `/limpiar`:\n"
            "  Muestra el total de archivos guardados y el espacio ocupado en `/downloads`. 📁\n\n"
            "• `/clean confirm`:\n"
            "  Vacía por completo la carpeta de descargas para liberar disco duro. 🧹\n\n"
            "• `/ping`:\n"
            "  Muestra la latencia del bot en milisegundos. 🏓"
        )
    elif section == "examples":
        return (
            "━━━━━━━━━━━━━━━━━━━━━\n"
            "  💡 **EJEMPLOS RÁPIDOS DE USO** 🚀\n"
            "━━━━━━━━━━━━━━━━━━━━━\n\n"
            "1️⃣ **Descargar MP3 de YouTube directamente:**\n"
            "   `/mp3 https://www.youtube.com/watch?v=dQw4w9WgXcQ`\n\n"
            "2️⃣ **Descargar historias de Instagram:**\n"
            "   `/insta nombre_usuario`\n\n"
            "3️⃣ **Consultar información de un vídeo sin descargarlo:**\n"
            "   `/info https://youtu.be/dQw4w9WgXcQ`\n\n"
            "4️⃣ **Liberar espacio en el servidor:**\n"
            "   `/clean confirm`"
        )
    else:  # main
        return (
            "━━━━━━━━━━━━━━━━━━━━━\n"
            "  📖 **CENTRO DE AYUDA INTERACTIVO** 🤖\n"
            "━━━━━━━━━━━━━━━━━━━━━\n\n"
            "Bienvenido a la guía visual del **Bot Multidescargas**. Selecciona una categoría para ver los detalles:\n\n"
            "📥 **Descargas**: Formatos, MP3 directo, Instagram y Spotify\n"
            "🌐 **Redes Soportadas**: YouTube, TikTok, X, Twitch, etc.\n"
            "🛠️ **Sistema**: Estado del servidor, limpieza y latencia\n"
            "💡 **Ejemplos**: Guía rápida paso a paso\n\n"
            "👇 *Haz clic en los botones inferiores para navegar:* "
        )


async def help_command(update: Update, context: ContextTypes.DEFAULT_TYPE) -> None:
    """Manejador para el comando /help o /ayuda."""
    await update.message.reply_text(
        get_help_text("main"),
        reply_markup=get_help_keyboard(),
        parse_mode="Markdown",
    )


async def ping_command(update: Update, context: ContextTypes.DEFAULT_TYPE) -> None:
    """Comprobación rápida de respuesta del bot."""
    if not update.message or not is_user_allowed(update.effective_user.id):
        return
    start = time.time()
    msg = await update.message.reply_text("🏓 Calculando latencia...")
    latency = round((time.time() - start) * 1000, 2)
    await msg.edit_text(
        f"🏓 **¡Pong!**\n⚡ Latencia de respuesta: `{latency} ms`\n🟢 Bot en línea y funcionando.",
        parse_mode="Markdown",
    )


async def audio_command(update: Update, context: ContextTypes.DEFAULT_TYPE) -> None:
    """Fuerza la descarga en formato MP3 para un enlace dado."""
    if not update.message or not is_user_allowed(update.effective_user.id):
        return
    if not context.args:
        await update.message.reply_text(
            "⚠️ Debes especificar un enlace.\nEjemplo: `/mp3 https://youtube.com/...`",
            parse_mode="Markdown",
        )
        return
    url = context.args[0]
    asyncio.create_task(process_single_download(url, update, context, fmt="mp3"))


async def info_command(update: Update, context: ContextTypes.DEFAULT_TYPE) -> None:
    """Obtiene metadatos del contenido sin descargar el vídeo."""
    if not update.message or not is_user_allowed(update.effective_user.id):
        return
    if not context.args:
        await update.message.reply_text(
            "⚠️ Debes especificar un enlace.\nEjemplo: `/info https://youtube.com/...`",
            parse_mode="Markdown",
        )
        return
    url = context.args[0]
    status_msg = await update.message.reply_text("🔍 Obteniendo información del enlace...")

    cmd = ["yt-dlp", "--dump-json", "--no-playlist", url]
    try:
        process = await asyncio.create_subprocess_exec(
            *cmd, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE
        )
        stdout, stderr = await process.communicate()
        if process.returncode == 0:
            import json
            data = json.loads(stdout.decode("utf-8"))
            title = data.get("title", "Desconocido")
            duration = data.get("duration", 0)
            uploader = data.get("uploader", "Desconocido")
            view_count = data.get("view_count", 0)
            extractor = data.get("extractor_key", "General")

            mins, secs = divmod(duration or 0, 60)
            dur_str = f"{int(mins)}m {int(secs)}s" if duration else "Desconocida"
            views_str = f"{view_count:,}" if view_count else "No disponible"

            info_text = (
                f"ℹ️ **Metadatos de Enlace** ({extractor})\n\n"
                f"📌 **Título:** `{title}`\n"
                f"👤 **Canal/Autor:** {uploader}\n"
                f"⏱️ **Duración:** {dur_str}\n"
                f"👁️ **Visualizaciones:** {views_str}\n"
                f"🔗 **URL:** {url}"
            )
            await status_msg.edit_text(info_text, parse_mode="Markdown")
        else:
            await status_msg.edit_text("❌ No se pudieron obtener metadatos para este enlace.")
    except Exception as e:
        await status_msg.edit_text(f"❌ Error al consultar metadatos: {e}")


async def clean_command(update: Update, context: ContextTypes.DEFAULT_TYPE) -> None:
    """Limpia o muestra el uso del directorio de descargas."""
    if not update.message or not is_user_allowed(update.effective_user.id):
        return

    if context.args and context.args[0].lower() in ("confirm", "purge", "borrar"):
        count = 0
        size_cleared = 0
        for root, dirs, files in os.walk(DOWNLOAD_DIR):
            for f in files:
                filepath = os.path.join(root, f)
                try:
                    size_cleared += os.path.getsize(filepath)
                    os.remove(filepath)
                    count += 1
                except Exception:
                    pass
        size_mb = size_cleared / (1024 * 1024)
        await update.message.reply_text(
            f"🧹 **Limpieza Completada**\n\n"
            f"🗑️ Archivos eliminados: `{count}`\n"
            f"💾 Espacio liberado: `{size_mb:.2f} MB`",
            parse_mode="Markdown",
        )
    else:
        total_files = 0
        total_size = 0
        for root, dirs, files in os.walk(DOWNLOAD_DIR):
            for f in files:
                total_files += 1
                try:
                    total_size += os.path.getsize(os.path.join(root, f))
                except Exception:
                    pass
        size_mb = total_size / (1024 * 1024)
        size_gb = total_size / (1024 ** 3)
        size_str = f"{size_gb:.2f} GB" if size_gb >= 1 else f"{size_mb:.2f} MB"

        await update.message.reply_text(
            f"📁 **Estado del Almacenamiento** (`/downloads`)\n\n"
            f"📄 Archivos guardados: `{total_files}`\n"
            f"💾 Espacio ocupado: `{size_str}`\n\n"
            f"⚠️ Para vaciar la carpeta y liberar espacio en disco, usa:\n`/clean confirm`",
            parse_mode="Markdown",
        )


async def status_command(update: Update, context: ContextTypes.DEFAULT_TYPE) -> None:
    """Manejador para el comando /status o /stats."""
    if not update.message or not is_user_allowed(update.effective_user.id):
        return

    cpu_percent = psutil.cpu_percent(interval=1)
    ram = psutil.virtual_memory()
    ram_percent = ram.percent
    ram_used = ram.used / (1024 ** 3)
    ram_total = ram.total / (1024 ** 3)

    uptime_secs = int(time.time() - BOT_START_TIME)
    hours, remainder = divmod(uptime_secs, 3600)
    minutes, seconds = divmod(remainder, 60)
    uptime_str = f"{hours}h {minutes}m {seconds}s"

    status_text = (
        "📊 **Estado del Bot y Servidor** 🤖\n\n"
        f"⏱️ **Tiempo activo:** `{uptime_str}`\n"
        f"💻 **CPU:** `{cpu_percent}%`\n"
        f"🧠 **RAM:** `{ram_percent}%` ({ram_used:.2f} GB / {ram_total:.2f} GB)\n"
    )

    try:
        disk = psutil.disk_usage(DOWNLOAD_DIR if os.path.exists(DOWNLOAD_DIR) else "/")
        disk_percent = disk.percent
        disk_used = disk.used / (1024 ** 3)
        disk_total = disk.total / (1024 ** 3)
        status_text += f"💾 **Disco:** `{disk_percent}%` ({disk_used:.2f} GB / {disk_total:.2f} GB)\n"
    except Exception:
        pass

    await update.message.reply_text(status_text, parse_mode="Markdown")


def _sync_download_stories(username: str) -> tuple[int, str]:
    """Función síncrona para descargar historias usando Instaloader."""
    L = get_instaloader_instance()
    profile = instaloader.Profile.from_username(L.context, username)
    count = 0
    target_name = f"{profile.username}_stories"
    
    for story in L.get_stories(userids=[profile.userid]):
        for item in story.get_items():
            logger.info(f"Procesando historia ID: {item.mediaid} de {profile.username}")
            L.download_storyitem(item, target=target_name)
            count += 1
    return count, profile.username


async def process_instagram_stories(
    username: str,
    update: Update,
    context: ContextTypes.DEFAULT_TYPE,
    status_msg=None,
) -> None:
    """Maneja la descarga de historias de Instagram notificando al usuario."""
    username = username.strip().lstrip("@")
    initial_text = f"🚀 Buscando e iniciando descarga de historias para @{username}... 📸"
    
    if status_msg is None:
        status_msg = await update.message.reply_text(initial_text)
    else:
        try:
            await status_msg.edit_text(initial_text)
        except Exception:
            pass

    try:
        count, clean_username = await asyncio.to_thread(_sync_download_stories, username)
        
        if count > 0:
            finish_text = f"🎉 ¡Se han descargado {count} historias de @{clean_username}! 📸✨"
        else:
            finish_text = (
                f"ℹ️ @{clean_username} no tiene historias activas en este momento (o el perfil es privado) 🔒"
            )
        
        try:
            await status_msg.edit_text(finish_text)
        except Exception:
            await update.message.reply_text(finish_text)

    except instaloader.exceptions.ProfileNotExistsException:
        finish_text = f"❌ El perfil de Instagram @{username} no existe."
        logger.warning(f"Intento de descargar historias de perfil inexistente: @{username}")
        try:
            await status_msg.edit_text(finish_text)
        except Exception:
            await update.message.reply_text(finish_text)
    except instaloader.exceptions.LoginRequiredException:
        finish_text = f"🔒 Se requiere iniciar sesión en Instagram para ver las historias de @{username}."
        logger.warning(f"Se requiere inicio de sesión para historias de @{username}")
        try:
            await status_msg.edit_text(finish_text)
        except Exception:
            await update.message.reply_text(finish_text)
    except Exception as e:
        logger.exception(f"Excepción al descargar historias de @{username}: {e}")
        err_str = str(e)
        if "Profile" in err_str and "does not exist" in err_str:
            finish_text = f"❌ El perfil de Instagram @{username} no existe."
        elif "login" in err_str.lower() or "redirect" in err_str.lower() or "404" in err_str:
            finish_text = f"⚠️ No se pudo acceder a las historias de @{username} (Perfil privado, no existente o requiere autenticación) 🔒"
        else:
            finish_text = f"❌ Error al descargar historias de @{username}: {e} ⚠️"
        
        try:
            await status_msg.edit_text(finish_text)
        except Exception:
            await update.message.reply_text(finish_text)


def _sync_download_post(shortcode: str) -> bool:
    """Descarga publicación/reel de Instagram por shortcode usando Instaloader."""
    L = get_instaloader_instance()
    post = instaloader.Post.from_shortcode(L.context, shortcode)
    target_dir = os.path.join(DOWNLOAD_DIR, "instagram")
    os.makedirs(target_dir, exist_ok=True)
    L.download_post(post, target=target_dir)
    return True


async def process_instagram_post(
    url: str,
    shortcode: Optional[str],
    update: Update,
    context: ContextTypes.DEFAULT_TYPE,
    status_msg=None,
) -> None:
    """Procesa publicaciones/reels de Instagram."""
    initial_text = "🚀 Descargando publicación/reel de Instagram... 📸"
    if status_msg is None:
        status_msg = await update.message.reply_text(initial_text)
    else:
        try:
            await status_msg.edit_text(initial_text)
        except Exception:
            pass

    download_success = False

    if shortcode:
        try:
            download_success = await asyncio.to_thread(_sync_download_post, shortcode)
        except Exception as e:
            logger.warning(f"Instaloader falló para shortcode {shortcode}, intentando con yt-dlp: {e}")

    # Fallback con yt-dlp para posts/reels
    if not download_success:
        os.makedirs(DOWNLOAD_DIR, exist_ok=True)
        output_template = os.path.join(DOWNLOAD_DIR, "instagram", "%(title)s [%(id)s].%(ext)s")
        cmd = [
            "yt-dlp",
            "--newline",
        ]
        if os.path.exists("/app/cookies.txt"):
            cmd.extend(["--cookies", "/app/cookies.txt"])
        cmd.extend([
            "-o",
            output_template,
            url,
        ])

        process = await asyncio.create_subprocess_exec(
            *cmd,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        _, stderr = await process.communicate()
        if process.returncode == 0:
            download_success = True
        else:
            error_msg = stderr.decode("utf-8", errors="replace").strip()
            logger.error(f"Error yt-dlp descargando Instagram ({url}): {error_msg}")

    if download_success:
        finish_text = "🎉 ¡Contenido de Instagram descargado con éxito! 📸✅"
    else:
        finish_text = "❌ No se pudo descargar la publicación de Instagram ⚠️"

    try:
        await status_msg.edit_text(finish_text)
    except Exception:
        await update.message.reply_text(finish_text)


async def insta_command(update: Update, context: ContextTypes.DEFAULT_TYPE) -> None:
    """Manejador para los comandos /insta y /historias."""
    if not update.message or not is_user_allowed(update.effective_user.id):
        return

    if not context.args:
        await update.message.reply_text(
            "⚠️ Debes especificar el nombre de usuario de Instagram.\nEjemplo: `/insta usuario`",
            parse_mode="Markdown",
        )
        return

    target_user = context.args[0]
    asyncio.create_task(process_instagram_stories(target_user, update, context))


async def process_spotify_download(
    url: str,
    update: Update,
    context: ContextTypes.DEFAULT_TYPE,
    status_msg=None,
) -> None:
    """Procesa la descarga de música de Spotify (canción, álbum, playlist, artista) usando spotDL."""
    initial_text = "🚀 Descargando música de Spotify (buscando metadatos y carátulas)... 🎵"
    if status_msg is None:
        status_msg = await update.message.reply_text(initial_text)
    else:
        try:
            await status_msg.edit_text(initial_text)
        except Exception:
            pass

    spotify_dir = os.path.join(DOWNLOAD_DIR, "spotify")
    os.makedirs(spotify_dir, exist_ok=True)

    output_template = os.path.join(spotify_dir, "{artist} - {title}.{output-ext}")
    cmd = [
        "spotdl",
        "download",
        url,
        "--output",
        output_template,
    ]

    logger.info(f"Iniciando descarga de Spotify: {url}")

    try:
        process = await asyncio.create_subprocess_exec(
            *cmd,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )

        last_edit_time = 0.0
        while True:
            line_bytes = await process.stdout.readline()
            if not line_bytes:
                break

            line = line_bytes.decode("utf-8", errors="replace").strip()
            if not line:
                continue

            current_time = asyncio.get_event_loop().time()
            if (current_time - last_edit_time) >= 2.0:
                last_edit_time = current_time
                if "Downloaded" in line or "Processing" in line or "Found" in line or "Downloading" in line:
                    clean_line = line.replace("[", "").replace("]", "")
                    try:
                        await status_msg.edit_text(
                            f"⏳ Descargando de Spotify 🎵:\n`{clean_line[:100]}`",
                            parse_mode="Markdown",
                        )
                    except Exception:
                        pass

        _, stderr = await process.communicate()

        if process.returncode == 0:
            logger.info(f"Descarga de Spotify completada para {url}")
            finish_text = "🎉 ¡Música de Spotify descargada con éxito! (Carátula y etiquetas MP3 integradas) 🎵✅"
            try:
                await status_msg.edit_text(finish_text)
            except Exception:
                await update.message.reply_text(finish_text)
        else:
            error_msg = stderr.decode("utf-8", errors="replace").strip()
            logger.error(f"Error en spotDL ({url}): {error_msg}")
            try:
                await status_msg.edit_text("❌ Hubo un error al descargar desde Spotify ⚠️")
            except Exception:
                pass

    except Exception as e:
        logger.exception(f"Excepción al descargar de Spotify ({url}): {e}")
        try:
            await status_msg.edit_text("❌ Hubo un error al descargar de Spotify ⚠️")
        except Exception:
            pass


async def spotify_command(update: Update, context: ContextTypes.DEFAULT_TYPE) -> None:
    """Manejador para los comandos /spotify y /sp."""
    if not update.message or not is_user_allowed(update.effective_user.id):
        return

    if not context.args:
        await update.message.reply_text(
            "⚠️ Debes proporcionar un enlace de Spotify (canción, álbum o playlist).\nEjemplo: `/spotify https://open.spotify.com/track/...`",
            parse_mode="Markdown",
        )
        return

    url = context.args[0]
    asyncio.create_task(process_spotify_download(url, update, context))


async def process_single_download(
    url: str,
    update: Update,
    context: ContextTypes.DEFAULT_TYPE,
    fmt: str = "mp4",
    status_msg=None,
) -> None:
    """Procesa la descarga en MP4 (máxima calidad) o MP3 de YouTube notificando el progreso."""
    is_mp3 = fmt.lower() == "mp3"
    fmt_label = "🎵 MP3" if is_mp3 else "📹 MP4 (Máxima Resolución)"
    initial_text = f"🚀 Iniciando descarga de YouTube (0%) 📥\nFormato: {fmt_label}"

    if status_msg is None:
        status_msg = await update.message.reply_text(initial_text)
    else:
        try:
            await status_msg.edit_text(initial_text)
        except Exception:
            pass

    os.makedirs(DOWNLOAD_DIR, exist_ok=True)

    if is_mp3:
        output_template = os.path.join(DOWNLOAD_DIR, "%(title)s [%(id)s].mp3")
        cmd = [
            "yt-dlp",
            "--no-playlist",
            "-x",
            "--audio-format",
            "mp3",
            "--audio-quality",
            "0",
            "--downloader", "aria2c",
            "--downloader-args", "aria2c:-x 16 -s 16 -k 1M",
            "--concurrent-fragments", "16",
            "--newline",
            "--print",
            "title",
            "--no-simulate",
            "-o",
            output_template,
            url,
        ]
    else:
        output_template = os.path.join(DOWNLOAD_DIR, "%(title)s [%(id)s].%(ext)s")
        cmd = [
            "yt-dlp",
            "--no-playlist",
            "-f",
            "bestvideo+bestaudio/best",
            "--merge-output-format",
            "mp4",
            "--recode-video",
            "mp4",
            "--downloader", "aria2c",
            "--downloader-args", "aria2c:-x 16 -s 16 -k 1M",
            "--concurrent-fragments", "16",
            "--newline",
            "--print",
            "title",
            "--no-simulate",
            "-o",
            output_template,
            url,
        ]

    logger.info(f"Iniciando descarga YouTube ({fmt_label}) para: {url}")

    try:
        process = await asyncio.create_subprocess_exec(
            *cmd,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )

        video_title = None
        last_reported_step = 0
        last_edit_time = 0.0
        percent_regex = re.compile(r"\[download\]\s+(\d+(?:\.\d+)?)%")

        while True:
            line_bytes = await process.stdout.readline()
            if not line_bytes:
                break

            line = line_bytes.decode("utf-8", errors="replace").strip()
            if not line:
                continue

            if (
                not line.startswith("[download]")
                and not line.startswith("[ffmpeg]")
                and not line.startswith("[ExtractAudio]")
                and not video_title
            ):
                video_title = line
                continue

            match = percent_regex.search(line)
            if match:
                try:
                    pct = float(match.group(1))
                    step = int(pct // 10) * 10
                    
                    # Generate progress bar
                    blocks_filled = int(pct // 10)
                    blocks_empty = 10 - blocks_filled
                    bar = "█" * blocks_filled + "░" * blocks_empty

                    current_time = asyncio.get_event_loop().time()

                    if (
                        step > last_reported_step
                        and step <= 100
                        and (current_time - last_edit_time) >= 1.0
                    ):
                        last_reported_step = step
                        last_edit_time = current_time
                        try:
                            await status_msg.edit_text(
                                f"⏳ Descargando ({fmt_label}):\n`[{bar}] {pct}%`\n{video_title if video_title else ''}"
                            )
                        except Exception:
                            pass
                except ValueError:
                    pass

        _, stderr = await process.communicate()

        if process.returncode == 0:
            final_title = video_title if video_title else "Archivo"
            logger.info(f"Descarga YouTube {fmt_label} completada: {final_title}")
            
            if is_mp3:
                finish_text = f"🎉 Se ha terminado este audio: {final_title} 🎵"
            else:
                finish_text = f"🎉 Se ha terminado este vídeo: {final_title} ✅"

            try:
                await status_msg.edit_text(finish_text)
            except Exception:
                await update.message.reply_text(finish_text)
        else:
            error_msg = stderr.decode("utf-8", errors="replace").strip()
            logger.error(f"Error en yt-dlp YouTube ({url}): {error_msg}")
            try:
                await status_msg.edit_text("❌ Hubo un error en la descarga de YouTube ⚠️")
            except Exception:
                pass

    except Exception as e:
        logger.exception(f"Excepción al descargar YouTube ({url}): {e}")
        try:
            await status_msg.edit_text("❌ Hubo un error en la descarga de YouTube ⚠️")
        except Exception:
            pass


async def handle_callback_query(
    update: Update, context: ContextTypes.DEFAULT_TYPE
) -> None:
    """Manejador de botones interactivos (Formatos MP4/MP3 y menú de ayuda)."""
    query = update.callback_query
    await query.answer()

    data = query.data

    # Manejo del menú interactivo de ayuda
    if data.startswith("help_"):
        section = data.replace("help_", "")
        try:
            await query.edit_message_text(
                get_help_text(section),
                reply_markup=get_help_keyboard(),
                parse_mode="Markdown",
            )
        except Exception:
            pass
        return

    if "_" not in data:
        return

    fmt, task_id = data.split("_", 1)
    url = PENDING_URLS.pop(task_id, None)

    if not url:
        await query.edit_message_text("⚠️ Enlace no encontrado o ya procesado.")
        return

    asyncio.create_task(
        process_single_download(
            url, update, context, fmt=fmt, status_msg=query.message
        )
    )


async def handle_message(update: Update, context: ContextTypes.DEFAULT_TYPE) -> None:
    """Manejador principal para mensajes entrantes."""
    if not update.message or not update.message.text:
        return

    user_id = update.effective_user.id
    if not is_user_allowed(user_id):
        logger.warning(f"Usuario no autorizado {user_id} intentó usar el bot.")
        await update.message.reply_text("⛔ No estás autorizado para usar este bot 🚫")
        return

    text = update.message.text.strip()

    # Comprobar si es un enlace de Spotify
    spotify_match = SPOTIFY_REGEX.search(text)
    if spotify_match:
        spotify_url = spotify_match.group(0)
        asyncio.create_task(process_spotify_download(spotify_url, update, context))
        return

    # Comprobar enlaces de Instagram en el mensaje
    stories_match = INSTAGRAM_STORIES_REGEX.search(text)
    post_match = INSTAGRAM_POST_REGEX.search(text)
    profile_match = INSTAGRAM_PROFILE_REGEX.search(text)

    if stories_match:
        target_username = stories_match.group(1)
        asyncio.create_task(process_instagram_stories(target_username, update, context))
        return

    if post_match:
        shortcode = post_match.group(1)
        url = post_match.group(0)
        asyncio.create_task(process_instagram_post(url, shortcode, update, context))
        return

    # Buscar URLs generales (YouTube u otros)
    urls = URL_REGEX.findall(text)

    if not urls:
        # Si no hay URL pero mandó un @usuario
        if text.startswith("@"):
            asyncio.create_task(process_instagram_stories(text[1:], update, context))
            return

        await update.message.reply_text(
            "Por favor, envíame un enlace válido 🔗 (YouTube, Instagram o Spotify) 🎶",
            parse_mode="Markdown",
        )
        return

    # Procesar URLs detectadas
    explicit_mp3 = "mp3" in text.lower()

    for url in urls:
        if "spotify.com" in url.lower():
            asyncio.create_task(process_spotify_download(url, update, context))
        elif "instagram.com" in url.lower():
            if profile_match:
                target_user = profile_match.group(1)
                if target_user not in ("p", "reel", "reels", "stories", "tv"):
                    asyncio.create_task(process_instagram_stories(target_user, update, context))
                    continue
            asyncio.create_task(process_instagram_post(url, None, update, context))
        else:
            # Detectar plataformas específicas para un mejor mensaje
            platform_name = "Vídeo/Audio"
            if TIKTOK_REGEX.search(url):
                platform_name = "TikTok"
            elif TWITTER_REGEX.search(url):
                platform_name = "X (Twitter)"
            elif REDDIT_REGEX.search(url):
                platform_name = "Reddit"
            elif FACEBOOK_REGEX.search(url):
                platform_name = "Facebook"
            elif PINTEREST_REGEX.search(url):
                platform_name = "Pinterest"
            elif TWITCH_REGEX.search(url):
                platform_name = "Twitch"
            elif SOUNDCLOUD_REGEX.search(url):
                platform_name = "SoundCloud"
            elif VIMEO_REGEX.search(url):
                platform_name = "Vimeo"
            elif "youtube.com" in url or "youtu.be" in url:
                platform_name = "YouTube"

            # Procesamiento genérico con yt-dlp
            if explicit_mp3:
                asyncio.create_task(
                    process_single_download(url, update, context, fmt="mp3")
                )
            else:
                task_id = str(hash(url + str(asyncio.get_event_loop().time())))
                PENDING_URLS[task_id] = url

                keyboard = [
                    [
                        InlineKeyboardButton(
                            f"1. 📹 {platform_name} a MP4",
                            callback_data=f"mp4_{task_id}",
                        )
                    ],
                    [
                        InlineKeyboardButton(
                            f"2. 🎵 {platform_name} a MP3", callback_data=f"mp3_{task_id}"
                        )
                    ],
                ]

                await update.message.reply_text(
                    f"🔗 He detectado un enlace de {platform_name}.\nSelecciona el formato a descargar:",
                    reply_markup=InlineKeyboardMarkup(keyboard),
                )


async def error_handler(update: object, context: ContextTypes.DEFAULT_TYPE) -> None:
    """Manejo global de errores para informar al usuario de que algo ha fallado."""
    logger.error("Exception while handling an update:", exc_info=context.error)
    if isinstance(update, Update) and update.effective_message:
        try:
            await update.effective_message.reply_text(
                "⚠️ **Ups, ha ocurrido un error inesperado al procesar tu solicitud.**\n"
                "Nuestro sistema ha registrado el problema. Por favor, inténtalo de nuevo más tarde o revisa el formato del enlace.",
                parse_mode="Markdown"
            )
        except Exception:
            pass


async def send_startup_notification(app: Application) -> None:
    """Envía un mensaje automático al usuario autorizado al iniciar el bot."""
    if ALLOWED_USER_ID:
        try:
            await app.bot.send_message(
                chat_id=int(ALLOWED_USER_ID),
                text=(
                    "🚀 **¡Bot Multidescargas Iniciado y Listo!** 🎉\n\n"
                    "El servidor está activo y preparado para procesar tus peticiones.\n\n"
                    "📌 **Instrucciones Rápidas:**\n"
                    "• Envía cualquier enlace (YouTube, Instagram, TikTok, Spotify...) para descargarlo en MP4 o MP3.\n"
                    "• Usa `/help` o `/ayuda` para ver la lista completa de funciones.\n"
                    "• Usa `/status` para monitorizar la salud del servidor."
                ),
                parse_mode="Markdown",
            )
            logger.info(f"Mensaje de bienvenida enviado correctamente al usuario {ALLOWED_USER_ID}")
        except Exception as e:
            logger.warning(f"No se pudo enviar el mensaje automático a Telegram: {e}")


def start_status_web_server():
    """Servidor web de panel de control y estado en puerto 8080 para cuando el bot ya está activo."""
    from http.server import HTTPServer, BaseHTTPRequestHandler
    import threading

    class StatusHandler(BaseHTTPRequestHandler):
        def do_GET(self):
            if self.path == "/reconfig":
                env_path = os.path.join(os.path.dirname(__file__), ".env")
                if os.path.exists(env_path):
                    try:
                        os.remove(env_path)
                    except Exception:
                        pass
                
                self.send_response(200)
                self.send_header("Content-Type", "text/html; charset=utf-8")
                self.end_headers()
                self.wfile.write(
                    "<h2>🔄 Reiniciando servidor web de configuración...</h2><script>setTimeout(() => window.location.href='/', 2000);</script>".encode("utf-8")
                )
                
                def reboot():
                    time.sleep(1)
                    os._exit(0)
                
                threading.Thread(target=reboot, daemon=True).start()
                return

            # Métricas rápidas del sistema
            cpu = psutil.cpu_percent(interval=None)
            ram = psutil.virtual_memory().percent
            uptime_sec = int(time.time() - BOT_START_TIME)
            hours, remainder = divmod(uptime_sec, 3600)
            minutes, seconds = divmod(remainder, 60)
            uptime_str = f"{hours}h {minutes}m {seconds}s"

            html = f"""<!DOCTYPE html>
<html lang="es">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Estado del Bot Multidescargas</title>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@300;400;600;700&display=swap" rel="stylesheet">
    <style>
        * {{ box-sizing: border-box; margin: 0; padding: 0; font-family: 'Inter', sans-serif; }}
        body {{
            background: radial-gradient(circle at top left, #1a1c2e, #0d0e15);
            color: #f3f4f6; min-height: 100vh; display: flex;
            justify-content: center; align-items: center; padding: 20px;
        }}
        .container {{
            width: 100%; max-width: 500px; background: rgba(255, 255, 255, 0.03);
            backdrop-filter: blur(16px); border: 1px solid rgba(255, 255, 255, 0.08);
            border-radius: 20px; padding: 35px; box-shadow: 0 20px 50px rgba(0, 0, 0, 0.5);
            text-align: center;
        }}
        .status-badge {{
            display: inline-flex; align-items: center; gap: 8px;
            background: rgba(16, 185, 129, 0.1); border: 1px solid rgba(16, 185, 129, 0.3);
            color: #34d399; padding: 6px 16px; border-radius: 20px;
            font-size: 13px; font-weight: 600; margin-bottom: 20px;
        }}
        .status-dot {{ width: 8px; height: 8px; border-radius: 50%; background: #10b981; box-shadow: 0 0 10px #10b981; }}
        h1 {{ font-size: 22px; font-weight: 700; color: #ffffff; margin-bottom: 8px; }}
        p {{ color: #9ca3af; font-size: 14px; margin-bottom: 25px; line-height: 1.5; }}
        .stats-grid {{ display: grid; grid-template-columns: repeat(3, 1fr); gap: 12px; margin-bottom: 25px; }}
        .stat-card {{ background: rgba(0, 0, 0, 0.3); border: 1px solid rgba(255, 255, 255, 0.06); border-radius: 12px; padding: 12px; }}
        .stat-value {{ font-size: 15px; font-weight: 700; color: #60a5fa; margin-bottom: 4px; }}
        .stat-label {{ font-size: 11px; color: #6b7280; text-transform: uppercase; }}
        .btn-group {{ display: flex; flex-direction: column; gap: 12px; }}
        .btn {{
            display: flex; align-items: center; justify-content: center; gap: 8px;
            padding: 13px; border-radius: 12px; font-weight: 600; font-size: 14px;
            text-decoration: none; border: none; cursor: pointer; transition: all 0.2s;
        }}
        .btn-primary {{ background: linear-gradient(135deg, #3b82f6, #8b5cf6); color: white; box-shadow: 0 10px 25px -5px rgba(59, 130, 246, 0.4); }}
        .btn-primary:hover {{ transform: translateY(-2px); }}
        .btn-secondary {{ background: rgba(255, 255, 255, 0.05); color: #d1d5db; border: 1px solid rgba(255, 255, 255, 0.1); }}
        .btn-secondary:hover {{ background: rgba(255, 255, 255, 0.1); color: white; }}
    </style>
</head>
<body>
    <div class="container">
        <div class="status-badge">
            <div class="status-dot"></div>
            BOT EN LÍNEA Y OPERATIVO
        </div>
        <h1>🤖 Bot Multidescargas</h1>
        <p>El bot de Telegram está activo y listo para procesar descargas de más de 15 plataformas.</p>

        <div class="stats-grid">
            <div class="stat-card">
                <div class="stat-value">{cpu}%</div>
                <div class="stat-label">CPU</div>
            </div>
            <div class="stat-card">
                <div class="stat-value">{ram}%</div>
                <div class="stat-label">RAM</div>
            </div>
            <div class="stat-card">
                <div class="stat-value">{uptime_str}</div>
                <div class="stat-label">UPTIME</div>
            </div>
        </div>

        <div class="btn-group">
            <a href="https://t.me/" target="_blank" class="btn btn-primary">
                📲 Ir a Telegram para usar el Bot
            </a>
            <a href="/reconfig" onclick="return confirm('¿Deseas resetear la configuración para cambiar Token o ID?');" class="btn btn-secondary">
                ⚙️ Reconfigurar Bot / Cambiar Token
            </a>
        </div>
    </div>
</body>
</html>"""
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.end_headers()
            self.wfile.write(html.encode("utf-8"))

        def log_message(self, format, *args):
            pass

    def run_server():
        try:
            httpd = HTTPServer(("", 8080), StatusHandler)
            httpd.serve_forever()
        except Exception:
            pass

    t = threading.Thread(target=run_server, daemon=True)
    t.start()


def main() -> None:
    """Función principal de arranque del bot."""
    load_env()
    token = os.getenv("TELEGRAM_TOKEN") or TELEGRAM_TOKEN
    if not token:
        logger.error("TELEGRAM_TOKEN no está definido en las variables de entorno.")
        sys.exit(1)

    logger.info(
        "Iniciando Bot Multidescargas (YouTube, Instagram & Spotify)..."
    )

    # Iniciar servidor web de estado permanente en el puerto 8080
    start_status_web_server()

    application = Application.builder().token(token).post_init(send_startup_notification).build()
    
    # Manejador global de errores
    application.add_error_handler(error_handler)

    application.add_handler(CommandHandler("start", start_command))
    application.add_handler(CommandHandler("help", help_command))
    application.add_handler(CommandHandler("ayuda", help_command))
    application.add_handler(CommandHandler("insta", insta_command))
    application.add_handler(CommandHandler("historias", insta_command))
    application.add_handler(CommandHandler("spotify", spotify_command))
    application.add_handler(CommandHandler("sp", spotify_command))
    application.add_handler(CommandHandler("status", status_command))
    application.add_handler(CommandHandler("stats", status_command))
    application.add_handler(CommandHandler("ping", ping_command))
    application.add_handler(CommandHandler("mp3", audio_command))
    application.add_handler(CommandHandler("audio", audio_command))
    application.add_handler(CommandHandler("info", info_command))
    application.add_handler(CommandHandler("clean", clean_command))
    application.add_handler(CommandHandler("limpiar", clean_command))

    application.add_handler(CallbackQueryHandler(handle_callback_query))
    application.add_handler(
        MessageHandler(filters.TEXT & ~filters.COMMAND, handle_message)
    )

    application.run_polling()


if __name__ == "__main__":
    main()
