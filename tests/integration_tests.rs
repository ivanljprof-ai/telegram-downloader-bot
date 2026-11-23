use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Path as AxumPath, State};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::sync::watch;

use telegram_downloader_bot::config::Config;
use telegram_downloader_bot::handlers::{
    AppState, handle_callback_query_inner, handle_message_inner, run_polling_loop,
    send_startup_notification,
};
use telegram_downloader_bot::telegram::{
    CallbackQuery, Chat, Message, TelegramClient, Update, User,
};
use telegram_downloader_bot::web::{SetupWebState, StatusWebState, setup_router, status_router};

#[derive(Debug, Clone)]
struct RecordedCall {
    method: String,
    body: Value,
}

struct MockTelegramState {
    calls: Mutex<Vec<RecordedCall>>,
    next_msg_id: AtomicI64,
    queued_updates: Mutex<Vec<Update>>,
}

impl MockTelegramState {
    fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            next_msg_id: AtomicI64::new(100),
            queued_updates: Mutex::new(Vec::new()),
        }
    }

    fn record(&self, method: &str, body: Value) {
        if let Ok(mut guard) = self.calls.lock() {
            guard.push(RecordedCall {
                method: method.to_string(),
                body,
            });
        }
    }

    fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

async fn mock_telegram_handler(
    State(state): State<Arc<MockTelegramState>>,
    AxumPath((_token_part, method)): AxumPath<(String, String)>,
    body: Option<Json<Value>>,
) -> Json<Value> {
    let payload = body.map_or_else(|| json!({}), |b| b.0);
    state.record(&method, payload.clone());

    match method.as_str() {
        "getMe" => Json(json!({
            "ok": true,
            "result": {
                "id": 777,
                "is_bot": true,
                "first_name": "RustTestBot",
                "username": "rust_test_bot"
            }
        })),
        "sendMessage" => {
            let msg_id = state.next_msg_id.fetch_add(1, Ordering::Relaxed);
            let chat_id = payload.get("chat_id").and_then(Value::as_i64).unwrap_or(1);
            let text = payload
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            Json(json!({
                "ok": true,
                "result": {
                    "message_id": msg_id,
                    "chat": { "id": chat_id, "type": "private" },
                    "text": text
                }
            }))
        }
        "editMessageText" => {
            let msg_id = payload
                .get("message_id")
                .and_then(Value::as_i64)
                .unwrap_or(100);
            let chat_id = payload.get("chat_id").and_then(Value::as_i64).unwrap_or(1);
            let text = payload
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            Json(json!({
                "ok": true,
                "result": {
                    "message_id": msg_id,
                    "chat": { "id": chat_id, "type": "private" },
                    "text": text
                }
            }))
        }
        "answerCallbackQuery" => Json(json!({
            "ok": true,
            "result": true
        })),
        "getUpdates" => {
            let updates = state
                .queued_updates
                .lock()
                .map(|mut q| std::mem::take(&mut *q))
                .unwrap_or_default();
            Json(json!({
                "ok": true,
                "result": updates
            }))
        }
        _ => Json(json!({
            "ok": false,
            "error_code": 404,
            "description": "unknown method"
        })),
    }
}

async fn spawn_mock_telegram() -> (String, Arc<MockTelegramState>) {
    let state = Arc::new(MockTelegramState::new());
    let app = Router::new()
        .route(
            "/{bot_token}/{method}",
            get(mock_telegram_handler).post(mock_telegram_handler),
        )
        .with_state(Arc::clone(&state));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), state)
}

fn test_config(tmp: &TempDir, api_base: String) -> Config {
    Config {
        telegram_token: Some("123:TEST_TOKEN".to_string()),
        allowed_user_id: Some(42),
        download_dir: tmp.path().join("downloads"),
        instagram_user: None,
        instagram_password: None,
        config_dir: tmp.path().to_path_buf(),
        telegram_api_base: api_base,
        web_port: 0,
    }
}

fn make_message(user_id: i64, first_name: &str, text: &str) -> Message {
    Message {
        message_id: 10,
        from: Some(User {
            id: user_id,
            first_name: first_name.to_string(),
            username: Some("tester".to_string()),
        }),
        chat: Chat {
            id: user_id,
            chat_type: "private".to_string(),
        },
        text: Some(text.to_string()),
    }
}

#[tokio::test]
async fn web_setup_get_post_creates_env_and_cookies_and_validates_fields() {
    let tmp = TempDir::new().expect("tempdir");
    let (shutdown_tx, _shutdown_rx) = watch::channel(false);
    let state = Arc::new(SetupWebState::new(
        tmp.path().to_path_buf(),
        "http://127.0.0.1:1".to_string(),
        shutdown_tx,
    ));
    let app = setup_router(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let client = reqwest::Client::new();
    let base = format!("http://{addr}");

    let get_res = client.get(&base).send().await.expect("GET /");
    assert_eq!(get_res.status(), reqwest::StatusCode::OK);
    let html = get_res.text().await.expect("body");
    assert!(html.contains("Configuración Inicial del Bot"));
    assert!(html.contains("name=\"telegram_token\""));
    assert!(html.contains("name=\"allowed_user_id\""));
    assert!(html.contains("name=\"cookies\""));

    // Missing required fields returns 400
    let mut bad_form = HashMap::new();
    bad_form.insert("telegram_token", "");
    bad_form.insert("allowed_user_id", "42");
    let bad_res = client
        .post(&base)
        .form(&bad_form)
        .send()
        .await
        .expect("POST invalid");
    assert_eq!(bad_res.status(), reqwest::StatusCode::BAD_REQUEST);
    let bad_body = bad_res.text().await.expect("bad body");
    assert!(bad_body.contains("Faltan datos obligatorios"));

    // Valid submission creates .env and cookies.txt
    let mut good_form = HashMap::new();
    good_form.insert("telegram_token", "999:SECRET_TOK");
    good_form.insert("allowed_user_id", "42");
    good_form.insert("instagram_user", "ig_user");
    good_form.insert("instagram_password", "ig_pass");
    good_form.insert("download_path", "./my_downloads");
    good_form.insert(
        "cookies",
        "# Netscape HTTP Cookie File\n.instagram.com\tTRUE",
    );

    let ok_res = client
        .post(&base)
        .form(&good_form)
        .send()
        .await
        .expect("POST valid");
    assert_eq!(ok_res.status(), reqwest::StatusCode::OK);
    let terminal_html = ok_res.text().await.expect("terminal html");
    assert!(terminal_html.contains("EventSource('/stream-setup')"));

    let env_written = std::fs::read_to_string(tmp.path().join(".env")).expect("read .env");
    assert!(env_written.contains("TELEGRAM_TOKEN=999:SECRET_TOK"));
    assert!(env_written.contains("ALLOWED_USER_ID=42"));
    assert!(env_written.contains("DOWNLOAD_PATH=./my_downloads"));
    assert!(env_written.contains("INSTAGRAM_USER=ig_user"));
    assert!(env_written.contains("INSTAGRAM_PASSWORD=ig_pass"));

    let cookies_written =
        std::fs::read_to_string(tmp.path().join("cookies.txt")).expect("read cookies.txt");
    assert!(cookies_written.contains(".instagram.com"));

    let loaded =
        Config::load_from_paths(&[tmp.path().join(".env")], tmp.path().to_path_buf(), |_| {
            None
        });
    assert_eq!(loaded.telegram_token.as_deref(), Some("999:SECRET_TOK"));
    assert_eq!(loaded.allowed_user_id, Some(42));
    assert_eq!(loaded.download_dir, PathBuf::from("./my_downloads"));
    assert_eq!(loaded.instagram_user.as_deref(), Some("ig_user"));
    assert_eq!(loaded.instagram_password.as_deref(), Some("ig_pass"));
}

#[tokio::test]
async fn web_setup_sse_stream_authenticates_and_signals_shutdown() {
    let tmp = TempDir::new().expect("tempdir");
    let (mock_base, mock_state) = spawn_mock_telegram().await;
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

    let mut web_state = SetupWebState::new(tmp.path().to_path_buf(), mock_base, shutdown_tx);
    web_state.step_delay = Duration::ZERO;
    web_state.exit_delay = Duration::from_millis(20);
    let app = setup_router(Arc::new(web_state));

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let client = reqwest::Client::new();
    let base = format!("http://{addr}");

    let mut form = HashMap::new();
    form.insert("telegram_token", "555:TOK");
    form.insert("allowed_user_id", "42");
    let post_res = client.post(&base).form(&form).send().await.expect("POST");
    assert_eq!(post_res.status(), reqwest::StatusCode::OK);

    let sse_res = client
        .get(format!("{base}/stream-setup"))
        .send()
        .await
        .expect("GET /stream-setup");
    assert_eq!(sse_res.status(), reqwest::StatusCode::OK);
    let body = sse_res.text().await.expect("sse text");

    assert!(body.contains("Guardando variables de entorno (.env) y cookies"));
    assert!(body.contains("Bot Autenticado: RustTestBot (@rust_test_bot)"));
    assert!(body.contains("Mensaje de bienvenida enviado con éxito a Telegram"));
    assert!(body.contains("\"type\":\"complete\""));
    assert!(body.contains("\"bot_username\":\"rust_test_bot\""));

    tokio::time::timeout(Duration::from_secs(2), shutdown_rx.changed())
        .await
        .expect("shutdown signal timeout")
        .expect("watch ok");
    assert!(*shutdown_rx.borrow());

    let calls = mock_state.calls();
    assert!(calls.iter().any(|c| c.method == "getMe"));
    assert!(calls.iter().any(|c| {
        c.method == "sendMessage" && c.body.get("chat_id").and_then(Value::as_i64) == Some(42)
    }));
}

#[tokio::test]
async fn web_status_dashboard_and_reconfig_removes_env() {
    let tmp = TempDir::new().expect("tempdir");
    let env_path = tmp.path().join(".env");
    std::fs::write(&env_path, "TELEGRAM_TOKEN=123:TEST\nALLOWED_USER_ID=42\n").expect("write .env");

    let (mock_base, _) = spawn_mock_telegram().await;
    let cfg = test_config(&tmp, mock_base.clone());
    let tg = TelegramClient::with_base_url("123:TEST", &mock_base).expect("tg");
    let app_state = Arc::new(AppState::new(cfg, tg).expect("app state"));

    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let status_state = Arc::new(StatusWebState {
        app_state,
        shutdown_tx,
        exit_delay: Duration::from_millis(20),
    });
    let app = status_router(status_state);

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    let client = reqwest::Client::new();
    let base = format!("http://{addr}");

    let dash_res = client.get(&base).send().await.expect("GET /");
    assert_eq!(dash_res.status(), reqwest::StatusCode::OK);
    let dash_html = dash_res.text().await.expect("dash html");
    assert!(dash_html.contains("Estado del Bot Multidescargas"));
    assert!(dash_html.contains("BOT EN LÍNEA Y OPERATIVO (MOTOR RUST)"));
    assert!(dash_html.contains("RAM BOT (RSS)"));
    assert!(dash_html.contains("Ir a Telegram para usar el Bot"));
    assert!(dash_html.contains("/reconfig"));

    let reconfig_res = client
        .get(format!("{base}/reconfig"))
        .send()
        .await
        .expect("GET /reconfig");
    assert_eq!(reconfig_res.status(), reqwest::StatusCode::OK);
    let reconfig_html = reconfig_res.text().await.expect("reconfig html");
    assert!(reconfig_html.contains("Reiniciando servidor web de configuración"));
    assert!(!env_path.exists());

    tokio::time::timeout(Duration::from_secs(2), shutdown_rx.changed())
        .await
        .expect("reconfig shutdown timeout")
        .expect("watch ok");
    assert!(*shutdown_rx.borrow());
}

#[tokio::test]
async fn telegram_commands_and_authorization_work_end_to_end() {
    let tmp = TempDir::new().expect("tempdir");
    let dl_dir = tmp.path().join("downloads");
    std::fs::create_dir_all(&dl_dir).expect("mkdir");
    std::fs::write(dl_dir.join("clip.mp4"), vec![1u8; 4096]).expect("write");
    std::fs::write(dl_dir.join("audio.mp3"), vec![2u8; 2048]).expect("write");

    let (mock_base, mock_state) = spawn_mock_telegram().await;
    let cfg = test_config(&tmp, mock_base.clone());
    let tg = TelegramClient::with_base_url("123:TEST_TOKEN", &mock_base).expect("tg");
    let state = Arc::new(AppState::new(cfg, tg).expect("state"));

    // Unauthorized text message gets denied
    handle_message_inner(
        Arc::clone(&state),
        make_message(999, "Intruder", "https://youtu.be/abc"),
    )
    .await;
    assert!(mock_state.calls().iter().any(|c| {
        c.method == "sendMessage"
            && c.body
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|t| t.contains("No estás autorizado"))
    }));

    // /start
    handle_message_inner(Arc::clone(&state), make_message(42, "Ivan", "/start")).await;
    assert!(mock_state.calls().iter().any(|c| {
        c.method == "sendMessage"
            && c.body
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|t| t.contains("¡Hola, Ivan!"))
    }));

    // /ping
    handle_message_inner(Arc::clone(&state), make_message(42, "Ivan", "/ping")).await;
    assert!(mock_state.calls().iter().any(|c| {
        c.method == "editMessageText"
            && c.body
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|t| t.contains("¡Pong!") && t.contains("Rust"))
    }));

    // /help + callback navigation
    handle_message_inner(Arc::clone(&state), make_message(42, "Ivan", "/help")).await;
    assert!(mock_state.calls().iter().any(|c| {
        c.method == "sendMessage"
            && c.body
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|t| t.contains("CENTRO DE AYUDA INTERACTIVO"))
            && c.body
                .get("reply_markup")
                .and_then(|m| m.get("inline_keyboard"))
                .and_then(Value::as_array)
                .is_some_and(|rows| rows.len() == 3)
    }));

    let help_query = CallbackQuery {
        id: "cb_help_1".to_string(),
        from: User {
            id: 42,
            first_name: "Ivan".to_string(),
            username: None,
        },
        message: Some(make_message(42, "Bot", "menu")),
        data: Some("help_networks".to_string()),
    };
    handle_callback_query_inner(Arc::clone(&state), help_query).await;
    assert!(mock_state.calls().iter().any(|c| {
        c.method == "answerCallbackQuery"
            && c.body.get("callback_query_id").and_then(Value::as_str) == Some("cb_help_1")
    }));
    assert!(mock_state.calls().iter().any(|c| {
        c.method == "editMessageText"
            && c.body
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|t| t.contains("REDES Y PLATAFORMAS SOPORTADAS"))
    }));

    // /status
    handle_message_inner(Arc::clone(&state), make_message(42, "Ivan", "/status")).await;
    assert!(mock_state.calls().iter().any(|c| {
        c.method == "sendMessage"
            && c.body
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|t| t.contains("Rust (Tokio)") && t.contains("RAM Bot (RSS)"))
    }));

    // /clean (inspect)
    handle_message_inner(Arc::clone(&state), make_message(42, "Ivan", "/clean")).await;
    assert!(mock_state.calls().iter().any(|c| {
        c.method == "sendMessage"
            && c.body
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|t| t.contains("Archivos guardados: `2`"))
    }));

    // /clean confirm (purge)
    handle_message_inner(
        Arc::clone(&state),
        make_message(42, "Ivan", "/clean confirm"),
    )
    .await;
    assert!(mock_state.calls().iter().any(|c| {
        c.method == "sendMessage"
            && c.body
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|t| t.contains("Archivos eliminados: `2`"))
    }));
    assert!(!dl_dir.join("clip.mp4").exists());
    assert!(!dl_dir.join("audio.mp3").exists());
}

#[tokio::test]
async fn url_inline_keyboard_and_callback_lifecycle() {
    let tmp = TempDir::new().expect("tempdir");
    let (mock_base, mock_state) = spawn_mock_telegram().await;
    let cfg = test_config(&tmp, mock_base.clone());
    let tg = TelegramClient::with_base_url("123:TEST_TOKEN", &mock_base).expect("tg");
    let state = Arc::new(AppState::new(cfg, tg).expect("state"));

    // Send YouTube link -> inline keyboard generated with compact task ID (< 64 bytes)
    handle_message_inner(
        Arc::clone(&state),
        make_message(42, "Ivan", "https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
    )
    .await;

    let calls = mock_state.calls();
    let kb_call = calls
        .iter()
        .find(|c| {
            c.method == "sendMessage"
                && c.body
                    .get("text")
                    .and_then(Value::as_str)
                    .is_some_and(|t| t.contains("YouTube"))
        })
        .expect("youtube inline keyboard message");

    let rows = kb_call.body["reply_markup"]["inline_keyboard"]
        .as_array()
        .expect("rows");
    let mp4_cb = rows[0][0]["callback_data"].as_str().expect("mp4 cb");
    let mp3_cb = rows[1][0]["callback_data"].as_str().expect("mp3 cb");
    assert_eq!(mp4_cb, "mp4_1");
    assert_eq!(mp3_cb, "mp3_1");
    assert!(mp4_cb.len() < 64);

    // Replace stored URL #1 with unreachable local .invalid URL so callback test never touches network
    let popped = state.take_pending_url("1");
    assert_eq!(
        popped.as_deref(),
        Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
    );
    state.insert_pending_url(
        "1".to_string(),
        "https://nonexistent.invalid/video".to_string(),
    );

    let cb_query = CallbackQuery {
        id: "cb_dl_1".to_string(),
        from: User {
            id: 42,
            first_name: "Ivan".to_string(),
            username: None,
        },
        message: Some(make_message(42, "Bot", "select")),
        data: Some("mp4_1".to_string()),
    };
    handle_callback_query_inner(Arc::clone(&state), cb_query.clone()).await;

    // Give the spawned task a brief moment to emit initial editMessageText
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(mock_state.calls().iter().any(|c| {
        c.method == "editMessageText"
            && c.body
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|t| t.contains("Iniciando descarga"))
    }));

    // Repeating same callback reports already processed
    handle_callback_query_inner(Arc::clone(&state), cb_query).await;
    assert!(mock_state.calls().iter().any(|c| {
        c.method == "editMessageText"
            && c.body
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|t| t.contains("Enlace no encontrado o ya procesado"))
    }));
}

#[tokio::test]
async fn startup_notification_and_polling_loop_process_updates_cleanly() {
    let tmp = TempDir::new().expect("tempdir");
    let (mock_base, mock_state) = spawn_mock_telegram().await;
    let cfg = test_config(&tmp, mock_base.clone());
    let tg = TelegramClient::with_base_url("123:TEST_TOKEN", &mock_base).expect("tg");
    let state = Arc::new(AppState::new(cfg, tg).expect("state"));

    send_startup_notification(&state)
        .await
        .expect("startup notification");
    assert!(mock_state.calls().iter().any(|c| {
        c.method == "sendMessage"
            && c.body.get("chat_id").and_then(Value::as_i64) == Some(42)
            && c.body
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|t| t.contains("Motor Rust"))
    }));

    if let Ok(mut q) = mock_state.queued_updates.lock() {
        q.push(Update {
            update_id: 1,
            message: Some(make_message(42, "Ivan", "/start")),
            callback_query: None,
        });
    }

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let poll_state = Arc::clone(&state);
    let poll_handle = tokio::spawn(async move { run_polling_loop(poll_state, shutdown_rx).await });

    // Wait until /start response is sent
    let mut processed = false;
    for _ in 0..40 {
        if mock_state.calls().iter().any(|c| {
            c.method == "sendMessage"
                && c.body
                    .get("text")
                    .and_then(Value::as_str)
                    .is_some_and(|t| t.contains("¡Hola, Ivan!"))
        }) {
            processed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(processed, "polling loop did not process /start update");

    let _ = shutdown_tx.send(true);
    let res = tokio::time::timeout(Duration::from_secs(2), poll_handle)
        .await
        .expect("polling shutdown timeout")
        .expect("join ok");
    assert!(res.is_ok());
}

#[tokio::test]
async fn telegram_commands_missing_args_prompt_usage() {
    let tmp = TempDir::new().expect("tempdir");
    let (mock_base, mock_state) = spawn_mock_telegram().await;
    let cfg = test_config(&tmp, mock_base.clone());
    let tg = TelegramClient::with_base_url("123:TEST_TOKEN", &mock_base).expect("tg");
    let state = Arc::new(AppState::new(cfg, tg).expect("state"));

    for cmd in [
        "/info",
        "/mp3",
        "/audio",
        "/spotify",
        "/sp",
        "/insta",
        "/historias",
    ] {
        handle_message_inner(Arc::clone(&state), make_message(42, "Ivan", cmd)).await;
    }

    let sent_texts: Vec<String> = mock_state
        .calls()
        .into_iter()
        .filter(|c| c.method == "sendMessage")
        .filter_map(|c| {
            c.body
                .get("text")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();

    assert_eq!(sent_texts.len(), 7);
    assert!(sent_texts.iter().all(|t| t.contains("⚠️ Debes")));
}
