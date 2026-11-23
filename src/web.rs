use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};
use tokio_stream::wrappers::ReceiverStream;
use tracing::{error, info, warn};

use crate::downloader::format_uptime;
use crate::handlers::{AppState, SystemSnapshot};
use crate::telegram::TelegramClient;

pub const HTML_FORM_TEMPLATE: &str = r##"<!DOCTYPE html>
<html lang="es">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Configuración del Bot Multidescargas</title>
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@300;400;600;700&family=JetBrains+Mono:wght@400;500&display=swap" rel="stylesheet">
    <style>
        * { box-sizing: border-box; margin: 0; padding: 0; font-family: 'Inter', sans-serif; }
        body {
            background: radial-gradient(circle at top left, #1a1c2e, #0d0e15);
            color: #f3f4f6;
            min-height: 100vh;
            display: flex;
            justify-content: center;
            align-items: center;
            padding: 20px;
        }
        .container {
            width: 100%;
            max-width: 540px;
            background: rgba(255, 255, 255, 0.03);
            backdrop-filter: blur(16px);
            -webkit-backdrop-filter: blur(16px);
            border: 1px solid rgba(255, 255, 255, 0.08);
            border-radius: 20px;
            padding: 40px;
            box-shadow: 0 20px 50px rgba(0, 0, 0, 0.5), inset 0 1px 0 rgba(255, 255, 255, 0.1);
        }
        .header { text-align: center; margin-bottom: 30px; }
        .header .icon { font-size: 44px; margin-bottom: 10px; display: inline-block; }
        .header h1 {
            font-size: 24px; font-weight: 700;
            background: linear-gradient(135deg, #60a5fa, #a855f7);
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
            margin-bottom: 8px;
        }
        .header p { color: #9ca3af; font-size: 14px; line-height: 1.5; }
        .form-group { margin-bottom: 20px; }
        label { display: block; font-size: 13px; font-weight: 600; color: #d1d5db; margin-bottom: 8px; }
        label span.required { color: #f87171; }
        label span.optional { color: #6b7280; font-weight: 400; font-size: 12px; }
        input[type="text"], input[type="password"], textarea {
            width: 100%; background: rgba(0, 0, 0, 0.3);
            border: 1px solid rgba(255, 255, 255, 0.12);
            border-radius: 10px; padding: 12px 16px; color: #ffffff;
            font-size: 14px; outline: none; transition: all 0.2s ease;
        }
        input[type="text"]:focus, input[type="password"]:focus, textarea:focus {
            border-color: #60a5fa; box-shadow: 0 0 0 3px rgba(96, 165, 250, 0.2);
            background: rgba(0, 0, 0, 0.5);
        }
        textarea { resize: vertical; min-height: 90px; font-family: 'JetBrains Mono', monospace; font-size: 12px; }
        .btn-submit {
            width: 100%; background: linear-gradient(135deg, #3b82f6, #8b5cf6);
            border: none; border-radius: 12px; color: white; font-weight: 600;
            font-size: 15px; padding: 14px; cursor: pointer; transition: all 0.3s ease;
            box-shadow: 0 10px 25px -5px rgba(59, 130, 246, 0.4); margin-top: 10px;
        }
        .btn-submit:hover {
            transform: translateY(-2px);
            box-shadow: 0 15px 30px -5px rgba(59, 130, 246, 0.6);
            background: linear-gradient(135deg, #2563eb, #7c3aed);
        }
        .footer-note {
            margin-top: 25px; text-align: center; font-size: 12px; color: #6b7280;
            border-top: 1px solid rgba(255, 255, 255, 0.05); padding-top: 15px;
        }
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <span class="icon">🚀</span>
            <h1>Configuración Inicial del Bot</h1>
            <p>Introduce tus datos para inicializar el Bot Multidescargas. Verás el progreso de arranque en pantalla en tiempo real.</p>
        </div>
        <form method="POST" action="/">
            <div class="form-group">
                <label for="telegram_token">🔑 Token del Bot de Telegram <span class="required">*</span></label>
                <input type="text" id="telegram_token" name="telegram_token" placeholder="123456789:ABCdefGhIJKlmNoPQRstUVwxyZ" required>
            </div>

            <div class="form-group">
                <label for="allowed_user_id">👤 ID de Usuario Autorizado (Telegram) <span class="required">*</span></label>
                <input type="text" id="allowed_user_id" name="allowed_user_id" placeholder="123456789" required>
            </div>

            <div class="form-group">
                <label for="instagram_user">📸 Usuario de Instagram <span class="optional">(Opcional)</span></label>
                <input type="text" id="instagram_user" name="instagram_user" placeholder="tu_usuario">
            </div>

            <div class="form-group">
                <label for="instagram_password">🔒 Contraseña de Instagram <span class="optional">(Opcional)</span></label>
                <input type="password" id="instagram_password" name="instagram_password" placeholder="••••••••">
            </div>

            <div class="form-group">
                <label for="download_path">📁 Ruta de Almacenamiento <span class="optional">(Predeterminado: ./downloads)</span></label>
                <input type="text" id="download_path" name="download_path" value="./downloads" placeholder="./downloads">
            </div>

            <div class="form-group">
                <label for="cookies">🍪 Contenido de cookies.txt (Instagram) <span class="optional">(Opcional)</span></label>
                <textarea id="cookies" name="cookies" placeholder="# Netscape HTTP Cookie File... Pega aquí el contenido de cookies.txt"></textarea>
            </div>

            <button type="submit" class="btn-submit">⚡ Guardar e Iniciar Sistema</button>
        </form>
        <div class="footer-note">
            ⚡ Configuración inicial automatizada de 1 solo clic.
        </div>
    </div>
</body>
</html>
"##;

pub const HTML_TERMINAL_TEMPLATE: &str = r##"<!DOCTYPE html>
<html lang="es">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Iniciando Sistema...</title>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;600;700&family=JetBrains+Mono:wght@400;500&display=swap" rel="stylesheet">
    <style>
        * { box-sizing: border-box; margin: 0; padding: 0; font-family: 'Inter', sans-serif; }
        body {
            background: #090a0f;
            color: #f3f4f6;
            min-height: 100vh;
            display: flex;
            justify-content: center;
            align-items: center;
            padding: 20px;
        }
        .container {
            width: 100%;
            max-width: 650px;
            background: rgba(255, 255, 255, 0.03);
            border: 1px solid rgba(255, 255, 255, 0.08);
            border-radius: 20px;
            padding: 30px;
            box-shadow: 0 20px 50px rgba(0, 0, 0, 0.6);
        }
        .header {
            display: flex;
            align-items: center;
            justify-content: space-between;
            margin-bottom: 20px;
            padding-bottom: 15px;
            border-bottom: 1px solid rgba(255, 255, 255, 0.08);
        }
        .header-title {
            display: flex;
            align-items: center;
            gap: 12px;
        }
        .status-dot {
            width: 10px;
            height: 10px;
            border-radius: 50%;
            background: #f59e0b;
            box-shadow: 0 0 10px #f59e0b;
            animation: pulse 1.5s infinite;
        }
        .status-dot.active {
            background: #10b981;
            box-shadow: 0 0 12px #10b981;
        }
        @keyframes pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.4; } }
        h2 { font-size: 18px; font-weight: 700; color: #ffffff; }

        .terminal {
            background: #050508;
            border: 1px solid rgba(255, 255, 255, 0.1);
            border-radius: 12px;
            padding: 20px;
            height: 240px;
            overflow-y: auto;
            font-family: 'JetBrains Mono', monospace;
            font-size: 13px;
            color: #34d399;
            line-height: 1.6;
            margin-bottom: 25px;
            box-shadow: inset 0 2px 8px rgba(0, 0, 0, 0.8);
        }
        .terminal p { margin-bottom: 6px; word-break: break-word; }
        .terminal .error { color: #f87171; }
        .terminal .info { color: #60a5fa; }
        .terminal .success { color: #34d399; font-weight: 600; }

        .actions {
            display: none;
            flex-direction: column;
            gap: 12px;
            animation: fadeIn 0.5s ease forwards;
        }
        @keyframes fadeIn { from { opacity: 0; transform: translateY(10px); } to { opacity: 1; transform: translateY(0); } }

        .btn-telegram {
            display: flex; align-items: center; justify-content: center; gap: 10px;
            width: 100%; background: linear-gradient(135deg, #2563eb, #0284c7);
            color: white; border: none; border-radius: 12px; padding: 14px;
            font-weight: 600; font-size: 15px; text-decoration: none;
            box-shadow: 0 10px 25px -5px rgba(37, 99, 235, 0.4);
            transition: all 0.2s ease; cursor: pointer;
        }
        .btn-telegram:hover {
            transform: translateY(-2px);
            box-shadow: 0 15px 30px -5px rgba(37, 99, 235, 0.6);
        }
        .btn-close {
            background: rgba(255, 255, 255, 0.05);
            border: 1px solid rgba(255, 255, 255, 0.1);
            color: #9ca3af; border-radius: 12px; padding: 12px;
            font-size: 14px; font-weight: 500; cursor: pointer; text-align: center;
            transition: all 0.2s ease;
        }
        .btn-close:hover { background: rgba(255, 255, 255, 0.1); color: #ffffff; }
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <div class="header-title">
                <div class="status-dot" id="statusDot"></div>
                <h2 id="statusTitle">Inicializando Bot...</h2>
            </div>
            <span style="font-size: 12px; color: #6b7280;" id="statusSubtitle">En proceso</span>
        </div>

        <div class="terminal" id="terminal">
            <p class="info">[00:00] ⚙️ Iniciando conexión con el servidor...</p>
        </div>

        <div class="actions" id="actions">
            <a href="#" id="telegramBtn" target="_blank" class="btn-telegram">
                📱 Abrir Telegram y hacer /start
            </a>
            <div class="btn-close" onclick="window.close(); alert('Puedes cerrar esta pestaña y volver a Telegram.');">
                ❌ Puedes cerrar esta ventana
            </div>
        </div>
    </div>

    <script>
        const terminal = document.getElementById('terminal');
        const actions = document.getElementById('actions');
        const statusDot = document.getElementById('statusDot');
        const statusTitle = document.getElementById('statusTitle');
        const statusSubtitle = document.getElementById('statusSubtitle');
        const telegramBtn = document.getElementById('telegramBtn');

        function appendLog(msg, type = 'success') {
            const p = document.createElement('p');
            p.className = type;
            const time = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
            p.innerText = `[${time}] ${msg}`;
            terminal.appendChild(p);
            terminal.scrollTop = terminal.scrollHeight;
        }

        const evtSource = new EventSource('/stream-setup');

        evtSource.onmessage = function(event) {
            try {
                const data = JSON.parse(event.data);
                if (data.type === 'log') {
                    appendLog(data.message, data.style || 'success');
                } else if (data.type === 'complete') {
                    appendLog('🎉 ¡TODO LISTO! El Bot ya está activo y funcionando.', 'success');
                    statusDot.classList.add('active');
                    statusTitle.innerText = '¡Todo Listo!';
                    statusSubtitle.innerText = 'Bot Operativo';

                    if (data.bot_username) {
                        telegramBtn.href = `https://t.me/${data.bot_username}`;
                        telegramBtn.innerText = `📲 Ir a Telegram (@${data.bot_username}) y enviar /start`;
                    }

                    actions.style.display = 'flex';
                    evtSource.close();
                } else if (data.type === 'error') {
                    appendLog(`❌ ${data.message}`, 'error');
                    statusTitle.innerText = 'Error en configuración';
                    evtSource.close();
                }
            } catch(e) {
                console.error(e);
            }
        };

        evtSource.onerror = function() {
            evtSource.close();
        };
    </script>
</body>
</html>
"##;

#[derive(Debug, Clone, Default)]
pub struct SetupCredentials {
    pub token: String,
    pub user_id: String,
    pub insta_user: String,
    pub insta_pass: String,
    pub cookies_txt: String,
}

/// Shared state for the setup web wizard.
pub struct SetupWebState {
    pub config_dir: PathBuf,
    pub telegram_api_base: String,
    pub latest_config: Mutex<SetupCredentials>,
    pub shutdown_tx: watch::Sender<bool>,
    pub step_delay: Duration,
    pub exit_delay: Duration,
}

impl SetupWebState {
    /// Creates a new [`SetupWebState`] with standard production SSE pacing.
    #[must_use]
    pub fn new(
        config_dir: PathBuf,
        telegram_api_base: String,
        shutdown_tx: watch::Sender<bool>,
    ) -> Self {
        Self {
            config_dir,
            telegram_api_base,
            latest_config: Mutex::new(SetupCredentials::default()),
            shutdown_tx,
            step_delay: Duration::from_millis(500),
            exit_delay: Duration::from_millis(1500),
        }
    }
}

/// Shared state for the status web dashboard.
pub struct StatusWebState {
    pub app_state: Arc<AppState>,
    pub shutdown_tx: watch::Sender<bool>,
    pub exit_delay: Duration,
}

#[derive(Serialize)]
struct SetupSsePayload<'a> {
    #[serde(rename = "type")]
    event_type: &'a str,
    message: &'a str,
    style: &'a str,
    bot_username: &'a str,
}

/// Builds the setup wizard Axum router (`GET /`, `POST /`, `GET /stream-setup`).
pub fn setup_router(state: Arc<SetupWebState>) -> Router {
    Router::new()
        .route("/", get(setup_get_form).post(setup_post_form))
        .route("/stream-setup", get(setup_stream))
        .with_state(state)
}

/// Builds the status dashboard Axum router (`GET /`, `GET /reconfig`).
pub fn status_router(state: Arc<StatusWebState>) -> Router {
    Router::new()
        .route("/", get(status_get_dashboard))
        .route("/reconfig", get(status_reconfig))
        .with_state(state)
}

async fn setup_get_form() -> Html<&'static str> {
    Html(HTML_FORM_TEMPLATE)
}

async fn setup_post_form(State(state): State<Arc<SetupWebState>>, body: String) -> Response {
    let parsed = parse_form_urlencoded(&body);
    let get = |key: &str| {
        parsed
            .get(key)
            .map_or("", String::as_str)
            .trim()
            .to_string()
    };

    let token = get("telegram_token");
    let user_id = get("allowed_user_id");
    let insta_user = get("instagram_user");
    let insta_pass = get("instagram_password");
    let dl_path_raw = get("download_path");
    let dl_path = if dl_path_raw.is_empty() {
        "./downloads".to_string()
    } else {
        dl_path_raw
    };
    let cookies_txt = get("cookies");

    if token.is_empty() || user_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Html(
                "<h3>❌ Faltan datos obligatorios (Token o User ID). Vuelve atrás e inténtalo de nuevo.</h3>",
            ),
        )
            .into_response();
    }

    if let Err(err) = tokio::fs::create_dir_all(&state.config_dir).await {
        error!(error = %err, "Failed to ensure config directory");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Html("<h3>❌ Error interno al preparar directorio de configuración.</h3>"),
        )
            .into_response();
    }

    let mut env_content =
        format!("TELEGRAM_TOKEN={token}\nALLOWED_USER_ID={user_id}\nDOWNLOAD_PATH={dl_path}\n");
    if !insta_user.is_empty() {
        let _ = writeln!(env_content, "INSTAGRAM_USER={insta_user}");
    }
    if !insta_pass.is_empty() {
        let _ = writeln!(env_content, "INSTAGRAM_PASSWORD={insta_pass}");
    }

    let env_path = state.config_dir.join(".env");
    if let Err(err) = tokio::fs::write(&env_path, env_content).await {
        error!(error = %err, path = %env_path.display(), "Failed writing .env");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Html("<h3>❌ No se pudo guardar el archivo .env.</h3>"),
        )
            .into_response();
    }

    if !cookies_txt.is_empty() {
        let cookies_path = state.config_dir.join("cookies.txt");
        if let Err(err) = tokio::fs::write(&cookies_path, &cookies_txt).await {
            warn!(error = %err, path = %cookies_path.display(), "Failed writing cookies.txt");
        }
    }

    if let Ok(mut guard) = state.latest_config.lock() {
        guard.token = token;
        guard.user_id = user_id;
        guard.insta_user = insta_user;
        guard.insta_pass = insta_pass;
        guard.cookies_txt = cookies_txt;
    }

    info!(env_path = %env_path.display(), "Saved setup configuration");
    Html(HTML_TERMINAL_TEMPLATE).into_response()
}

async fn setup_stream(State(state): State<Arc<SetupWebState>>) -> impl IntoResponse {
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(16);

    tokio::spawn(async move {
        let creds = state
            .latest_config
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();

        let send_event = |event_type: &str, msg: &str, style: &str, bot_username: &str| {
            let payload = SetupSsePayload {
                event_type,
                message: msg,
                style,
                bot_username,
            };
            if let Ok(json_str) = serde_json::to_string(&payload) {
                let _ = tx.try_send(Ok(Event::default().data(json_str)));
            }
        };

        send_event(
            "log",
            "⚙️ Guardando variables de entorno (.env) y cookies...",
            "info",
            "",
        );
        if !state.step_delay.is_zero() {
            tokio::time::sleep(state.step_delay).await;
        }

        let mut bot_username = String::new();
        if !creds.token.is_empty() {
            send_event(
                "log",
                "🔑 Verificando Token con Telegram Bot API...",
                "info",
                "",
            );
            if !state.step_delay.is_zero() {
                tokio::time::sleep(state.step_delay).await;
            }
            match TelegramClient::with_base_url(&creds.token, &state.telegram_api_base) {
                Ok(client) => match client.get_me().await {
                    Ok(me) => {
                        bot_username = me.username.unwrap_or_default();
                        let msg =
                            format!("✅ Bot Autenticado: {} (@{})", me.first_name, bot_username);
                        send_event("log", &msg, "success", "");
                    }
                    Err(err) => {
                        let msg = format!("⚠️ Aviso de API: No se pudo verificar @getMe ({err})");
                        send_event("log", &msg, "error", "");
                    }
                },
                Err(err) => {
                    let msg = format!("⚠️ Error creando cliente HTTP: {err}");
                    send_event("log", &msg, "error", "");
                }
            }
        }

        if !creds.token.is_empty() && !creds.user_id.is_empty() {
            let info_msg = format!(
                "📩 Enviando mensaje de bienvenida automático a Telegram (ID: {})...",
                creds.user_id
            );
            send_event("log", &info_msg, "info", "");
            if !state.step_delay.is_zero() {
                tokio::time::sleep(state.step_delay).await;
            }

            if let Ok(chat_id) = creds.user_id.parse::<i64>() {
                let welcome_text = "🚀 **¡Bot Multidescargas Configurado y Listo!** 🎉\n\n\
                    Tu servidor web de inicio ha completado la instalación.\n\n\
                    📌 **¿Cómo empezar?**\n\
                    • Envía cualquier enlace (YouTube, TikTok, Instagram, Spotify...) para descargar.\n\
                    • Usa `/help` para ver la lista de comandos disponibles.";
                match TelegramClient::with_base_url(&creds.token, &state.telegram_api_base) {
                    Ok(client) => {
                        match client
                            .send_message(chat_id, welcome_text, Some("Markdown"), None)
                            .await
                        {
                            Ok(_) => send_event(
                                "log",
                                "✅ Mensaje de bienvenida enviado con éxito a Telegram",
                                "success",
                                "",
                            ),
                            Err(err) => {
                                let msg = format!("⚠️ No se pudo enviar el mensaje directo: {err}");
                                send_event("log", &msg, "error", "");
                            }
                        }
                    }
                    Err(err) => {
                        let msg = format!("⚠️ No se pudo enviar el mensaje directo: {err}");
                        send_event("log", &msg, "error", "");
                    }
                }
            } else {
                send_event(
                    "log",
                    "⚠️ ID de usuario no numérico; omitiendo mensaje directo",
                    "error",
                    "",
                );
            }
        }

        if !state.step_delay.is_zero() {
            tokio::time::sleep(state.step_delay).await;
        }
        send_event(
            "log",
            "🚀 Arrancando Bot Multidescargas en segundo plano...",
            "info",
            "",
        );
        if !state.step_delay.is_zero() {
            tokio::time::sleep(state.step_delay).await;
        }
        send_event("complete", "🎉 ¡Todo listo!", "success", &bot_username);

        let shutdown_tx = state.shutdown_tx.clone();
        let exit_delay = state.exit_delay;
        tokio::spawn(async move {
            if !exit_delay.is_zero() {
                tokio::time::sleep(exit_delay).await;
            }
            let _ = shutdown_tx.send(true);
        });
    });

    Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default())
}

async fn status_get_dashboard(State(state): State<Arc<StatusWebState>>) -> Html<String> {
    let snap = state.app_state.collect_metrics().await;
    Html(render_status_html(&snap))
}

async fn status_reconfig(State(state): State<Arc<StatusWebState>>) -> Html<&'static str> {
    let env_path = state.app_state.config.env_file_path();
    if env_path.exists()
        && let Err(err) = tokio::fs::remove_file(&env_path).await
    {
        warn!(error = %err, path = %env_path.display(), "Failed removing .env on /reconfig");
    }

    let shutdown_tx = state.shutdown_tx.clone();
    let delay = state.exit_delay;
    tokio::spawn(async move {
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        let _ = shutdown_tx.send(true);
    });

    Html(
        "<h2>🔄 Reiniciando servidor web de configuración...</h2><script>setTimeout(() => window.location.href='/', 2000);</script>",
    )
}

/// Renders the live HTML status dashboard page.
#[must_use]
pub fn render_status_html(snap: &SystemSnapshot) -> String {
    let uptime_str = format_uptime(snap.uptime_secs);
    format!(
        r#"<!DOCTYPE html>
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
            width: 100%; max-width: 540px; background: rgba(255, 255, 255, 0.03);
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
        .stats-grid {{ display: grid; grid-template-columns: repeat(2, 1fr); gap: 12px; margin-bottom: 25px; }}
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
            BOT EN LÍNEA Y OPERATIVO (MOTOR RUST)
        </div>
        <h1>🤖 Bot Multidescargas</h1>
        <p>El bot de Telegram está activo con motor nativo Rust y listo para procesar descargas de más de 15 plataformas.</p>

        <div class="stats-grid">
            <div class="stat-card">
                <div class="stat-value">{:.1}%</div>
                <div class="stat-label">CPU</div>
            </div>
            <div class="stat-card">
                <div class="stat-value">{:.1}%</div>
                <div class="stat-label">RAM SISTEMA</div>
            </div>
            <div class="stat-card">
                <div class="stat-value">{:.2} MB</div>
                <div class="stat-label">RAM BOT (RSS)</div>
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
</html>"#,
        snap.cpu_percent, snap.ram_percent, snap.process_ram_mb
    )
}

/// Runs the setup wizard HTTP server until SSE completes (or external shutdown).
///
/// # Errors
/// Returns an error if binding port `port` fails or Axum server encounters a fatal I/O error.
pub async fn run_setup_server(
    port: u16,
    config_dir: PathBuf,
    telegram_api_base: String,
    mut external_shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let (shutdown_tx, mut setup_done_rx) = watch::channel(false);
    let state = Arc::new(SetupWebState::new(
        config_dir,
        telegram_api_base,
        shutdown_tx,
    ));
    let app = setup_router(state);

    let listener = TcpListener::bind(("0.0.0.0", port))
        .await
        .with_context(|| format!("failed to bind setup web server on 0.0.0.0:{port}"))?;
    info!(port = port, "Setup web wizard listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::select! {
                _ = setup_done_rx.changed() => {}
                _ = external_shutdown.changed() => {}
            }
        })
        .await
        .context("setup web server error")?;

    info!("Setup web wizard shut down cleanly");
    Ok(())
}

/// Runs the status dashboard HTTP server until `shutdown_rx` receives `true`.
///
/// # Errors
/// Returns an error if binding port `port` fails or Axum server encounters a fatal I/O error.
pub async fn run_status_server(
    port: u16,
    app_state: Arc<AppState>,
    shutdown_tx: watch::Sender<bool>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<()> {
    let web_state = Arc::new(StatusWebState {
        app_state,
        shutdown_tx,
        exit_delay: Duration::from_secs(1),
    });
    let app = status_router(web_state);

    let listener = TcpListener::bind(("0.0.0.0", port))
        .await
        .with_context(|| format!("failed to bind status web server on 0.0.0.0:{port}"))?;
    info!(port = port, "Status web dashboard listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            if !*shutdown_rx.borrow() {
                let _ = shutdown_rx.changed().await;
            }
        })
        .await
        .context("status web server error")?;

    Ok(())
}

/// Parses an `application/x-www-form-urlencoded` body cleanly.
#[must_use]
pub fn parse_form_urlencoded(body: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in body.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (raw_k, raw_v) = pair.split_once('=').unwrap_or((pair, ""));
        let k = decode_form_component(raw_k);
        let v = decode_form_component(raw_v);
        map.insert(k, v);
    }
    map
}

fn decode_form_component(s: &str) -> String {
    let plus_as_space = s.replace('+', " ");
    urlencoding::decode(&plus_as_space)
        .map(std::borrow::Cow::into_owned)
        .unwrap_or(plus_as_space)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_form_urlencoded_decodes_plus_spaces_and_percent_encoding() {
        let body =
            "telegram_token=123%3AABC+DEF&allowed_user_id=98765&cookies=%23+Netscape%0Adomain";
        let parsed = parse_form_urlencoded(body);
        assert_eq!(
            parsed.get("telegram_token").map(String::as_str),
            Some("123:ABC DEF")
        );
        assert_eq!(
            parsed.get("allowed_user_id").map(String::as_str),
            Some("98765")
        );
        assert_eq!(
            parsed.get("cookies").map(String::as_str),
            Some("# Netscape\ndomain")
        );
    }

    #[test]
    fn render_status_html_contains_rust_badge_and_metrics() {
        let snap = SystemSnapshot {
            cpu_percent: 12.5,
            ram_percent: 45.2,
            ram_used_gb: 3.6,
            ram_total_gb: 8.0,
            process_ram_mb: 8.42,
            disk: None,
            uptime_secs: 3723,
        };
        let html = render_status_html(&snap);
        assert!(html.contains("Estado del Bot Multidescargas"));
        assert!(html.contains("BOT EN LÍNEA Y OPERATIVO (MOTOR RUST)"));
        assert!(html.contains("12.5%"));
        assert!(html.contains("45.2%"));
        assert!(html.contains("8.42 MB"));
        assert!(html.contains("1h 2m 3s"));
        assert!(html.contains("/reconfig"));
        assert!(html.contains("Ir a Telegram para usar el Bot"));
    }
}
