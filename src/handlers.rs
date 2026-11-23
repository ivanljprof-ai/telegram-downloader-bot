use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use sysinfo::{
    CpuRefreshKind, Disks, MINIMUM_CPU_UPDATE_INTERVAL, MemoryRefreshKind, ProcessRefreshKind,
    ProcessesToUpdate, RefreshKind, System, get_current_pid,
};
use tokio::sync::watch;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::downloader::{
    MessageAnalysis, UrlAction, analyze_message_text, fetch_video_info, format_size_mb_or_gb,
    format_uptime, inspect_download_dir, purge_download_dir, run_instagram_post,
    run_instagram_stories, run_single_download, run_spotify_download,
};
use crate::telegram::{
    CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup, Message, TelegramClient, Update,
};

const MAX_PENDING_URLS: usize = 512;

/// Disk usage summary for the download directory mount point.
#[derive(Debug, Clone, Copy)]
pub struct DiskStats {
    pub percent: f64,
    pub used_gb: f64,
    pub total_gb: f64,
}

/// Snapshot of system and process resource metrics.
#[derive(Debug, Clone, Copy)]
pub struct SystemSnapshot {
    pub cpu_percent: f32,
    pub ram_percent: f64,
    pub ram_used_gb: f64,
    pub ram_total_gb: f64,
    pub process_ram_mb: f64,
    pub disk: Option<DiskStats>,
    pub uptime_secs: u64,
}

struct SystemTracker {
    system: System,
    last_cpu_refresh: Instant,
}

struct PendingUrls {
    map: HashMap<String, String>,
    order: VecDeque<String>,
}

impl PendingUrls {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn insert(&mut self, id: String, url: String) {
        if self.map.len() >= MAX_PENDING_URLS
            && let Some(oldest) = self.order.pop_front()
        {
            self.map.remove(&oldest);
        }
        self.order.push_back(id.clone());
        self.map.insert(id, url);
    }

    fn take(&mut self, id: &str) -> Option<String> {
        self.map.remove(id)
    }
}

/// Shared bot runtime state.
pub struct AppState {
    pub config: Config,
    pub telegram: TelegramClient,
    pub start_time: Instant,
    pending_urls: Mutex<PendingUrls>,
    task_counter: AtomicU64,
    tracker: Mutex<SystemTracker>,
}

impl AppState {
    /// Creates a new [`AppState`] and warms up `sysinfo` CPU counters.
    ///
    /// # Errors
    /// Returns an error if `sysinfo::System` initialization fails.
    pub fn new(config: Config, telegram: TelegramClient) -> Result<Self> {
        let mut system = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::nothing().with_usage())
                .with_memory(MemoryRefreshKind::everything()),
        )?;
        system.refresh_cpu_usage();
        system.refresh_memory();

        Ok(Self {
            config,
            telegram,
            start_time: Instant::now(),
            pending_urls: Mutex::new(PendingUrls::new()),
            task_counter: AtomicU64::new(0),
            tracker: Mutex::new(SystemTracker {
                system,
                last_cpu_refresh: Instant::now(),
            }),
        })
    }

    /// Generates the next compact numeric task identifier for inline callback data.
    pub fn next_task_id(&self) -> String {
        self.task_counter
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1)
            .to_string()
    }

    /// Stores a pending URL keyed by compact `task_id`.
    pub fn insert_pending_url(&self, task_id: String, url: String) {
        if let Ok(mut guard) = self.pending_urls.lock() {
            guard.insert(task_id, url);
        }
    }

    /// Pops a pending URL by `task_id`.
    #[must_use]
    pub fn take_pending_url(&self, task_id: &str) -> Option<String> {
        self.pending_urls
            .lock()
            .ok()
            .and_then(|mut guard| guard.take(task_id))
    }

    /// Collects current CPU, system RAM, process RSS, disk usage, and uptime metrics.
    pub async fn collect_metrics(&self) -> SystemSnapshot {
        let wait_needed = self.tracker.lock().ok().and_then(|guard| {
            let elapsed = guard.last_cpu_refresh.elapsed();
            if elapsed < MINIMUM_CPU_UPDATE_INTERVAL {
                Some(MINIMUM_CPU_UPDATE_INTERVAL.saturating_sub(elapsed))
            } else {
                None
            }
        });

        if let Some(delay) = wait_needed {
            tokio::time::sleep(delay).await;
        }

        let (cpu_percent, ram_percent, ram_used_gb, ram_total_gb, process_ram_mb) =
            if let Ok(mut guard) = self.tracker.lock() {
                guard.system.refresh_cpu_usage();
                guard.system.refresh_memory();
                guard.last_cpu_refresh = Instant::now();

                let pid_mem = if let Ok(pid) = get_current_pid() {
                    guard.system.refresh_processes_specifics(
                        ProcessesToUpdate::Some(&[pid]),
                        true,
                        ProcessRefreshKind::nothing().with_memory(),
                    );
                    guard.system.process(pid).map_or(0, |p| p.memory())
                } else {
                    0
                };

                let total_mem = guard.system.total_memory();
                let used_mem = guard.system.used_memory();
                #[allow(clippy::cast_precision_loss)]
                let ram_pct = if total_mem > 0 {
                    (used_mem as f64 / total_mem as f64) * 100.0
                } else {
                    0.0
                };
                #[allow(clippy::cast_precision_loss)]
                let used_gb = used_mem as f64 / (1024.0 * 1024.0 * 1024.0);
                #[allow(clippy::cast_precision_loss)]
                let total_gb = total_mem as f64 / (1024.0 * 1024.0 * 1024.0);
                #[allow(clippy::cast_precision_loss)]
                let proc_mb = pid_mem as f64 / (1024.0 * 1024.0);

                (
                    guard.system.global_cpu_usage(),
                    ram_pct,
                    used_gb,
                    total_gb,
                    proc_mb,
                )
            } else {
                (0.0, 0.0, 0.0, 0.0, 0.0)
            };

        let target = if self.config.download_dir.exists() {
            self.config.download_dir.clone()
        } else {
            std::path::PathBuf::from("/")
        };

        let disk = tokio::task::spawn_blocking(move || compute_disk_stats(&target))
            .await
            .ok()
            .flatten();

        SystemSnapshot {
            cpu_percent,
            ram_percent,
            ram_used_gb,
            ram_total_gb,
            process_ram_mb,
            disk,
            uptime_secs: self.start_time.elapsed().as_secs(),
        }
    }
}

fn compute_disk_stats(path: &std::path::Path) -> Option<DiskStats> {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let disks = Disks::new_with_refreshed_list().ok()?;
    let best = disks
        .iter()
        .filter(|d| canonical.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())?;

    let total = best.total_space();
    let available = best.available_space();
    let used = total.saturating_sub(available);
    #[allow(clippy::cast_precision_loss)]
    let percent = if total > 0 {
        (used as f64 / total as f64) * 100.0
    } else {
        0.0
    };
    #[allow(clippy::cast_precision_loss)]
    let used_gb = used as f64 / (1024.0 * 1024.0 * 1024.0);
    #[allow(clippy::cast_precision_loss)]
    let total_gb = total as f64 / (1024.0 * 1024.0 * 1024.0);

    Some(DiskStats {
        percent,
        used_gb,
        total_gb,
    })
}

/// Returns the interactive inline keyboard for `/help`.
#[must_use]
pub fn get_help_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup {
        inline_keyboard: vec![
            vec![
                InlineKeyboardButton {
                    text: "📥 Descargas".to_string(),
                    callback_data: "help_downloads".to_string(),
                },
                InlineKeyboardButton {
                    text: "🌐 Redes Soportadas".to_string(),
                    callback_data: "help_networks".to_string(),
                },
            ],
            vec![
                InlineKeyboardButton {
                    text: "🛠️ Sistema".to_string(),
                    callback_data: "help_system".to_string(),
                },
                InlineKeyboardButton {
                    text: "💡 Ejemplos".to_string(),
                    callback_data: "help_examples".to_string(),
                },
            ],
            vec![InlineKeyboardButton {
                text: "🏠 Menú Principal".to_string(),
                callback_data: "help_main".to_string(),
            }],
        ],
    }
}

/// Returns the formatted Markdown help text for the given section (`main`, `downloads`, `networks`, `system`, `examples`).
#[must_use]
pub fn get_help_text(section: &str) -> &'static str {
    match section {
        "downloads" => {
            "━━━━━━━━━━━━━━━━━━━━━\n\
             \x20\x20📥 **DESCARGAS Y FORMATOS** 📹🎵\n\
             ━━━━━━━━━━━━━━━━━━━━━\n\n\
             • **Enviar Enlace Directo**:\n\
             \x20\x20Envía cualquier URL en el chat y elige:\n\
             \x20\x20├ 📹 **MP4**: Máxima resolución disponible\n\
             \x20\x20└ 🎵 **MP3**: Extracción de audio pura\n\n\
             • `/mp3 <enlace>` o `/audio <enlace>`:\n\
             \x20\x20Fuerza la descarga directa en audio MP3 sin preguntar.\n\n\
             • `/insta <usuario>` o `/historias <usuario>`:\n\
             \x20\x20Descarga las historias activas de un perfil público de Instagram. 📸\n\n\
             • `/spotify <enlace>` o `/sp <enlace>`:\n\
             \x20\x20Descarga canciones, álbumes o playlists de Spotify con carátula y tags. 🎵\n\n\
             • `/info <enlace>`:\n\
             \x20\x20Muestra título, duración, canal y vistas sin descargar el archivo. 🔍\n\n\
             📊 *Todas las descargas incluyen barra de progreso visual en tiempo real.*"
        }
        "networks" => {
            "━━━━━━━━━━━━━━━━━━━━━\n\
             \x20\x20🌐 **REDES Y PLATAFORMAS SOPORTADAS** 📱\n\
             ━━━━━━━━━━━━━━━━━━━━━\n\n\
             🔴 **Vídeo & Streaming:**\n\
             • YouTube, Twitch, Vimeo, Dailymotion, Streamable, Kick, Rumble\n\n\
             📸 **Redes Sociales:**\n\
             • Instagram (Posts, Reels, Historias, IGTV)\n\
             • TikTok, Twitter / X, Facebook, Reddit, Pinterest, Douyin\n\n\
             🎵 **Música & Audio:**\n\
             • Spotify (Pistas, Álbumes, Playlists), SoundCloud, Mixcloud\n\n\
             🔗 *Simplemente copia cualquier enlace y pégalo en el chat.*"
        }
        "system" => {
            "━━━━━━━━━━━━━━━━━━━━━\n\
             \x20\x20🛠️ **COMANDOS DE SISTEMA Y UTILIDADES** 📊\n\
             ━━━━━━━━━━━━━━━━━━━━━\n\n\
             • `/status` o `/stats`:\n\
             \x20\x20Muestra CPU, RAM, uso de Disco y tiempo activo (*uptime*) del bot. 💻\n\n\
             • `/clean` o `/limpiar`:\n\
             \x20\x20Muestra el total de archivos guardados y el espacio ocupado en `/downloads`. 📁\n\n\
             • `/clean confirm`:\n\
             \x20\x20Vacía por completo la carpeta de descargas para liberar disco duro. 🧹\n\n\
             • `/ping`:\n\
             \x20\x20Muestra la latencia del bot en milisegundos. 🏓"
        }
        "examples" => {
            "━━━━━━━━━━━━━━━━━━━━━\n\
             \x20\x20💡 **EJEMPLOS RÁPIDOS DE USO** 🚀\n\
             ━━━━━━━━━━━━━━━━━━━━━\n\n\
             1️⃣ **Descargar MP3 de YouTube directamente:**\n\
             \x20\x20\x20`/mp3 https://www.youtube.com/watch?v=dQw4w9WgXcQ`\n\n\
             2️⃣ **Descargar historias de Instagram:**\n\
             \x20\x20\x20`/insta nombre_usuario`\n\n\
             3️⃣ **Consultar información de un vídeo sin descargarlo:**\n\
             \x20\x20\x20`/info https://youtu.be/dQw4w9WgXcQ`\n\n\
             4️⃣ **Liberar espacio en el servidor:**\n\
             \x20\x20\x20`/clean confirm`"
        }
        _ => {
            "━━━━━━━━━━━━━━━━━━━━━\n\
             \x20\x20📖 **CENTRO DE AYUDA INTERACTIVO** 🤖\n\
             ━━━━━━━━━━━━━━━━━━━━━\n\n\
             Bienvenido a la guía visual del **Bot Multidescargas**. Selecciona una categoría para ver los detalles:\n\n\
             📥 **Descargas**: Formatos, MP3 directo, Instagram y Spotify\n\
             🌐 **Redes Soportadas**: YouTube, TikTok, X, Twitch, etc.\n\
             🛠️ **Sistema**: Estado del servidor, limpieza y latencia\n\
             💡 **Ejemplos**: Guía rápida paso a paso\n\n\
             👇 *Haz clic en los botones inferiores para navegar:* "
        }
    }
}

/// Sends a startup notification message to `ALLOWED_USER_ID` if configured.
///
/// # Errors
/// Returns an error if the Telegram API call fails.
pub async fn send_startup_notification(state: &AppState) -> Result<()> {
    let Some(user_id) = state.config.allowed_user_id else {
        return Ok(());
    };
    let text = "🚀 **¡Bot Multidescargas Iniciado y Listo!** 🎉\n\n\
        El servidor está activo (Motor Rust ⚡) y preparado para procesar tus peticiones.\n\n\
        📌 **Instrucciones Rápidas:**\n\
        • Envía cualquier enlace (YouTube, Instagram, TikTok, Spotify...) para descargarlo en MP4 o MP3.\n\
        • Usa `/help` o `/ayuda` para ver la lista completa de funciones.\n\
        • Usa `/status` para monitorizar la salud del servidor.";
    state
        .telegram
        .send_message(user_id, text, Some("Markdown"), None)
        .await?;
    info!(user_id = user_id, "Startup notification sent");
    Ok(())
}

/// Runs the Telegram long-polling loop until `shutdown` signals `true`.
///
/// # Errors
/// Returns only when shutdown is requested cleanly.
pub async fn run_polling_loop(
    state: Arc<AppState>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let mut offset: Option<i64> = None;
    info!("Starting Telegram long-polling loop");

    loop {
        if *shutdown.borrow() {
            break;
        }

        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_ok() && *shutdown.borrow() {
                    info!("Shutdown signal received in polling loop");
                    break;
                }
            }
            updates_res = state.telegram.get_updates(offset, 30) => {
                match updates_res {
                    Ok(updates) => {
                        for update in updates {
                            offset = Some(update.update_id.saturating_add(1));
                            let st = Arc::clone(&state);
                            tokio::spawn(async move {
                                handle_update(st, update).await;
                            });
                        }
                    }
                    Err(err) => {
                        warn!(error = %err, "Error polling Telegram getUpdates, retrying in 3s");
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_secs(3)) => {}
                            _ = shutdown.changed() => {
                                if *shutdown.borrow() {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Dispatches a single Telegram [`Update`] (message or callback query).
pub async fn handle_update(state: Arc<AppState>, update: Update) {
    if let Some(msg) = update.message {
        handle_message_inner(state, msg).await;
    } else if let Some(query) = update.callback_query {
        handle_callback_query_inner(state, query).await;
    }
}

/// Processes a single incoming Telegram [`Message`] (commands and media URLs).
pub async fn handle_message_inner(state: Arc<AppState>, msg: Message) {
    let Some(raw_text) = msg.text.as_deref() else {
        return;
    };
    let text = raw_text.trim();
    if text.is_empty() {
        return;
    }

    let chat_id = msg.chat.id;
    let user_id = msg.from.as_ref().map_or(chat_id, |u| u.id);

    if let Some(rest) = text.strip_prefix('/') {
        let mut parts = rest.split_whitespace();
        let cmd_token = parts.next().unwrap_or("");
        let cmd = cmd_token
            .split('@')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let args: Vec<&str> = parts.collect();

        match cmd.as_str() {
            "start" => {
                let first_name = msg.from.as_ref().map_or("amigo", |u| u.first_name.as_str());
                info!(user_id = user_id, command = "start", "Executed /start");
                let welcome = format!(
                    "👋 **¡Hola, {first_name}! Bienvenido al Bot Multidescargas & Utilidades** 🚀\n\n\
                     Conmigo puedes descargar vídeo y audio de prácticamente cualquier plataforma web, gestionar almacenamiento y consultar información. ⚡\n\n\
                     📌 **Redes soportadas:**\n\
                     • 📹 **YouTube**, TikTok, Instagram (Posts/Reels/Historias), Spotify, Twitter/X, Facebook, Reddit, Pinterest, Twitch, SoundCloud, Vimeo...\n\n\
                     💡 **¿Cómo empezar?**\n\
                     • ¡Simplemente pega un enlace en el chat y elige el formato! 📥\n\
                     • Escribe `/help` o `/ayuda` para ver la lista completa de comandos disponibles."
                );
                let _ = state
                    .telegram
                    .send_message(chat_id, &welcome, Some("Markdown"), None)
                    .await;
            }
            "help" | "ayuda" => {
                let kb = get_help_keyboard();
                let _ = state
                    .telegram
                    .send_message(chat_id, get_help_text("main"), Some("Markdown"), Some(&kb))
                    .await;
            }
            "ping" => {
                if !state.config.is_user_allowed(user_id) {
                    return;
                }
                let started = Instant::now();
                if let Ok(sent) = state
                    .telegram
                    .send_message(chat_id, "🏓 Calculando latencia...", None, None)
                    .await
                {
                    let latency_ms = started.elapsed().as_secs_f64() * 1000.0;
                    let reply = format!(
                        "🏓 **¡Pong!**\n⚡ Latencia de respuesta: `{latency_ms:.2} ms`\n🦀 Motor: Rust (Ultra bajo consumo)\n🟢 Bot en línea y funcionando."
                    );
                    let _ = state
                        .telegram
                        .edit_message_text(chat_id, sent.message_id, &reply, Some("Markdown"), None)
                        .await;
                }
            }
            "status" | "stats" => {
                if !state.config.is_user_allowed(user_id) {
                    return;
                }
                let snap = state.collect_metrics().await;
                let uptime_str = format_uptime(snap.uptime_secs);
                let mut status_text = format!(
                    "📊 **Estado del Bot y Servidor** 🤖\n\n\
                     🦀 **Motor:** `Rust (Tokio)`\n\
                     ⏱️ **Tiempo activo:** `{uptime_str}`\n\
                     💻 **CPU:** `{:.1}%`\n\
                     🧠 **RAM Sistema:** `{:.1}%` ({:.2} GB / {:.2} GB)\n\
                     🪶 **RAM Bot (RSS):** `{:.2} MB`\n",
                    snap.cpu_percent,
                    snap.ram_percent,
                    snap.ram_used_gb,
                    snap.ram_total_gb,
                    snap.process_ram_mb,
                );
                if let Some(disk) = snap.disk {
                    use std::fmt::Write as _;
                    let _ = writeln!(
                        status_text,
                        "💾 **Disco:** `{:.1}%` ({:.2} GB / {:.2} GB)",
                        disk.percent, disk.used_gb, disk.total_gb
                    );
                }
                let _ = state
                    .telegram
                    .send_message(chat_id, &status_text, Some("Markdown"), None)
                    .await;
            }
            "clean" | "limpiar" => {
                if !state.config.is_user_allowed(user_id) {
                    return;
                }
                let confirm = args.first().is_some_and(|arg| {
                    matches!(
                        arg.to_ascii_lowercase().as_str(),
                        "confirm" | "purge" | "borrar"
                    )
                });
                if confirm {
                    match purge_download_dir(&state.config.download_dir).await {
                        Ok(stats) => {
                            #[allow(clippy::cast_precision_loss)]
                            let size_mb = stats.total_bytes as f64 / (1024.0 * 1024.0);
                            let reply = format!(
                                "🧹 **Limpieza Completada**\n\n\
                                 🗑️ Archivos eliminados: `{}`\n\
                                 💾 Espacio liberado: `{size_mb:.2} MB`",
                                stats.file_count
                            );
                            let _ = state
                                .telegram
                                .send_message(chat_id, &reply, Some("Markdown"), None)
                                .await;
                        }
                        Err(err) => {
                            error!(error = %err, "Failed to purge download directory");
                        }
                    }
                } else if let Ok(stats) = inspect_download_dir(&state.config.download_dir).await {
                    let size_str = format_size_mb_or_gb(stats.total_bytes);
                    let reply = format!(
                        "📁 **Estado del Almacenamiento** (`{}`)\n\n\
                         📄 Archivos guardados: `{}`\n\
                         💾 Espacio ocupado: `{size_str}`\n\n\
                         ⚠️ Para vaciar la carpeta y liberar espacio en disco, usa:\n`/clean confirm`",
                        state.config.download_dir.display(),
                        stats.file_count
                    );
                    let _ = state
                        .telegram
                        .send_message(chat_id, &reply, Some("Markdown"), None)
                        .await;
                }
            }
            "info" => {
                if !state.config.is_user_allowed(user_id) {
                    return;
                }
                let Some(url) = args.first().copied() else {
                    let _ = state
                        .telegram
                        .send_message(
                            chat_id,
                            "⚠️ Debes especificar un enlace.\nEjemplo: `/info https://youtube.com/...`",
                            Some("Markdown"),
                            None,
                        )
                        .await;
                    return;
                };
                let sent = state
                    .telegram
                    .send_message(
                        chat_id,
                        "🔍 Obteniendo información del enlace...",
                        None,
                        None,
                    )
                    .await;
                let msg_id = sent.ok().map(|m| m.message_id);
                let url_owned = url.to_string();
                let tg = state.telegram.clone();
                tokio::spawn(async move {
                    let text = match fetch_video_info(&url_owned).await {
                        Ok(info_text) => info_text,
                        Err(err) => {
                            warn!(error = %err, "Failed to fetch metadata");
                            "❌ No se pudieron obtener metadatos para este enlace.".to_string()
                        }
                    };
                    if let Some(id) = msg_id {
                        if tg
                            .edit_message_text(chat_id, id, &text, Some("Markdown"), None)
                            .await
                            .is_err()
                        {
                            let _ = tg.send_message(chat_id, &text, None, None).await;
                        }
                    } else {
                        let _ = tg
                            .send_message(chat_id, &text, Some("Markdown"), None)
                            .await;
                    }
                });
            }
            "mp3" | "audio" => {
                if !state.config.is_user_allowed(user_id) {
                    return;
                }
                let Some(url) = args.first().copied() else {
                    let _ = state
                        .telegram
                        .send_message(
                            chat_id,
                            "⚠️ Debes especificar un enlace.\nEjemplo: `/mp3 https://youtube.com/...`",
                            Some("Markdown"),
                            None,
                        )
                        .await;
                    return;
                };
                let tg = state.telegram.clone();
                let dl_dir = state.config.download_dir.clone();
                let url_owned = url.to_string();
                tokio::spawn(async move {
                    run_single_download(&tg, chat_id, None, &url_owned, &dl_dir, "mp3").await;
                });
            }
            "spotify" | "sp" => {
                if !state.config.is_user_allowed(user_id) {
                    return;
                }
                let Some(url) = args.first().copied() else {
                    let _ = state
                        .telegram
                        .send_message(
                            chat_id,
                            "⚠️ Debes proporcionar un enlace de Spotify (canción, álbum o playlist).\nEjemplo: `/spotify https://open.spotify.com/track/...`",
                            Some("Markdown"),
                            None,
                        )
                        .await;
                    return;
                };
                let tg = state.telegram.clone();
                let dl_dir = state.config.download_dir.clone();
                let url_owned = url.to_string();
                tokio::spawn(async move {
                    run_spotify_download(&tg, chat_id, None, &url_owned, &dl_dir).await;
                });
            }
            "insta" | "historias" => {
                if !state.config.is_user_allowed(user_id) {
                    return;
                }
                let Some(target) = args.first().copied() else {
                    let _ = state
                        .telegram
                        .send_message(
                            chat_id,
                            "⚠️ Debes especificar el nombre de usuario de Instagram.\nEjemplo: `/insta usuario`",
                            Some("Markdown"),
                            None,
                        )
                        .await;
                    return;
                };
                let st = Arc::clone(&state);
                let target_owned = target.to_string();
                tokio::spawn(async move {
                    run_instagram_stories(&st.telegram, chat_id, None, &target_owned, &st.config)
                        .await;
                });
            }
            _ => {}
        }
        return;
    }

    if !state.config.is_user_allowed(user_id) {
        warn!(user_id = user_id, "Unauthorized user attempted to use bot");
        let _ = state
            .telegram
            .send_message(
                chat_id,
                "⛔ No estás autorizado para usar este bot 🚫",
                None,
                None,
            )
            .await;
        return;
    }

    match analyze_message_text(text) {
        MessageAnalysis::NoValidUrl => {
            let _ = state
                .telegram
                .send_message(
                    chat_id,
                    "Por favor, envíame un enlace válido 🔗 (YouTube, Instagram o Spotify) 🎶",
                    Some("Markdown"),
                    None,
                )
                .await;
        }
        MessageAnalysis::Actions(actions) => {
            for action in actions {
                match action {
                    UrlAction::Spotify(url) => {
                        let tg = state.telegram.clone();
                        let dl_dir = state.config.download_dir.clone();
                        tokio::spawn(async move {
                            run_spotify_download(&tg, chat_id, None, &url, &dl_dir).await;
                        });
                    }
                    UrlAction::InstagramStories(username) => {
                        let st = Arc::clone(&state);
                        tokio::spawn(async move {
                            run_instagram_stories(
                                &st.telegram,
                                chat_id,
                                None,
                                &username,
                                &st.config,
                            )
                            .await;
                        });
                    }
                    UrlAction::InstagramPost { url, shortcode } => {
                        let st = Arc::clone(&state);
                        tokio::spawn(async move {
                            run_instagram_post(
                                &st.telegram,
                                chat_id,
                                None,
                                &url,
                                shortcode.as_deref(),
                                &st.config,
                            )
                            .await;
                        });
                    }
                    UrlAction::DirectMp3 { url, .. } => {
                        let tg = state.telegram.clone();
                        let dl_dir = state.config.download_dir.clone();
                        tokio::spawn(async move {
                            run_single_download(&tg, chat_id, None, &url, &dl_dir, "mp3").await;
                        });
                    }
                    UrlAction::PromptFormat { url, platform } => {
                        let task_id = state.next_task_id();
                        state.insert_pending_url(task_id.clone(), url);

                        let markup = InlineKeyboardMarkup {
                            inline_keyboard: vec![
                                vec![InlineKeyboardButton {
                                    text: format!("1. 📹 {platform} a MP4"),
                                    callback_data: format!("mp4_{task_id}"),
                                }],
                                vec![InlineKeyboardButton {
                                    text: format!("2. 🎵 {platform} a MP3"),
                                    callback_data: format!("mp3_{task_id}"),
                                }],
                            ],
                        };
                        let prompt = format!(
                            "🔗 He detectado un enlace de {platform}.\nSelecciona el formato a descargar:"
                        );
                        let _ = state
                            .telegram
                            .send_message(chat_id, &prompt, None, Some(&markup))
                            .await;
                    }
                }
            }
        }
    }
}

/// Handles inline keyboard callback queries (`/help` sections and download format choices).
pub async fn handle_callback_query_inner(state: Arc<AppState>, query: CallbackQuery) {
    let _ = state.telegram.answer_callback_query(&query.id, None).await;

    if !state.config.is_user_allowed(query.from.id) {
        return;
    }

    let Some(data) = query.data.as_deref() else {
        return;
    };
    let Some(message) = query.message else {
        return;
    };
    let chat_id = message.chat.id;
    let message_id = message.message_id;

    if let Some(section) = data.strip_prefix("help_") {
        let kb = get_help_keyboard();
        let _ = state
            .telegram
            .edit_message_text(
                chat_id,
                message_id,
                get_help_text(section),
                Some("Markdown"),
                Some(&kb),
            )
            .await;
        return;
    }

    let Some((fmt, task_id)) = data.split_once('_') else {
        return;
    };

    let Some(url) = state.take_pending_url(task_id) else {
        let _ = state
            .telegram
            .edit_message_text(
                chat_id,
                message_id,
                "⚠️ Enlace no encontrado o ya procesado.",
                None,
                None,
            )
            .await;
        return;
    };

    let tg = state.telegram.clone();
    let dl_dir = state.config.download_dir.clone();
    let fmt_owned = fmt.to_string();
    tokio::spawn(async move {
        run_single_download(&tg, chat_id, Some(message_id), &url, &dl_dir, &fmt_owned).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_keyboard_and_all_sections_are_valid() {
        let kb = get_help_keyboard();
        assert_eq!(kb.inline_keyboard.len(), 3);
        assert_eq!(kb.inline_keyboard[0][0].callback_data, "help_downloads");
        assert_eq!(kb.inline_keyboard[0][1].callback_data, "help_networks");
        assert_eq!(kb.inline_keyboard[1][0].callback_data, "help_system");
        assert_eq!(kb.inline_keyboard[1][1].callback_data, "help_examples");
        assert_eq!(kb.inline_keyboard[2][0].callback_data, "help_main");

        assert!(get_help_text("main").contains("CENTRO DE AYUDA INTERACTIVO"));
        assert!(get_help_text("downloads").contains("DESCARGAS Y FORMATOS"));
        assert!(get_help_text("networks").contains("REDES Y PLATAFORMAS SOPORTADAS"));
        assert!(get_help_text("system").contains("COMANDOS DE SISTEMA"));
        assert!(get_help_text("examples").contains("EJEMPLOS RÁPIDOS DE USO"));
        assert!(get_help_text("unknown").contains("CENTRO DE AYUDA INTERACTIVO"));
    }

    #[test]
    fn pending_urls_task_ids_are_compact_and_bounded() {
        let mut pending = PendingUrls::new();
        for i in 0..600 {
            pending.insert(i.to_string(), format!("https://youtu.be/{i}"));
        }
        assert!(pending.map.len() <= MAX_PENDING_URLS);
        assert!(pending.take("0").is_none());
        assert_eq!(pending.take("599").as_deref(), Some("https://youtu.be/599"));
    }
}
