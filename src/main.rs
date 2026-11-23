#![forbid(unsafe_code)]

use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::watch;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use telegram_downloader_bot::config::{CliMode, Config, parse_cli_mode};
use telegram_downloader_bot::handlers::{AppState, run_polling_loop, send_startup_notification};
use telegram_downloader_bot::telegram::TelegramClient;
use telegram_downloader_bot::web::{run_setup_server, run_status_server};

#[tokio::main]
async fn main() {
    init_tracing();

    if let Err(err) = run().await {
        error!(error = %err, "Fatal application error");
        std::process::exit(1);
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

async fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mode = parse_cli_mode(&args);

    let (signal_tx, signal_rx) = watch::channel(false);
    tokio::spawn(async move {
        if let Err(err) = wait_for_termination_signal().await {
            warn!(error = %err, "Signal listener failed");
        }
        let _ = signal_tx.send(true);
    });

    let mut config = Config::load();

    if mode == CliMode::SetupOnly {
        info!("Running in setup-only mode on port {}", config.web_port);
        return run_setup_server(
            config.web_port,
            config.config_dir,
            config.telegram_api_base,
            signal_rx,
        )
        .await;
    }

    if !config.has_token() {
        info!(
            "TELEGRAM_TOKEN missing or empty; starting web setup wizard on port {}",
            config.web_port
        );
        run_setup_server(
            config.web_port,
            config.config_dir.clone(),
            config.telegram_api_base.clone(),
            signal_rx.clone(),
        )
        .await?;

        if *signal_rx.borrow() {
            return Ok(());
        }

        config = Config::load();
        if !config.has_token() {
            anyhow::bail!("TELEGRAM_TOKEN is still missing after setup wizard completed");
        }
    }

    let token = config
        .telegram_token
        .clone()
        .context("TELEGRAM_TOKEN missing")?;
    let telegram = TelegramClient::with_base_url(token, &config.telegram_api_base)
        .context("failed to initialize Telegram API client")?;

    let state = Arc::new(AppState::new(config, telegram)?);
    info!("Iniciando Bot Multidescargas (Motor Rust)...");

    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

    // Forward OS termination signals into runtime shutdown channel
    let mut sig_rx = signal_rx;
    let fwd_tx = shutdown_tx.clone();
    tokio::spawn(async move {
        if *sig_rx.borrow() || sig_rx.changed().await.is_ok() {
            let _ = fwd_tx.send(true);
        }
    });

    if let Err(err) = send_startup_notification(&state).await {
        warn!(error = %err, "Could not send startup notification to Telegram");
    }

    let web_state = Arc::clone(&state);
    let web_shutdown_tx = shutdown_tx.clone();
    let web_shutdown_rx = shutdown_rx.clone();
    let web_port = state.config.web_port;
    let web_handle = tokio::spawn(async move {
        if let Err(err) =
            run_status_server(web_port, web_state, web_shutdown_tx, web_shutdown_rx).await
        {
            error!(error = %err, "Status web server failed");
        }
    });

    let poll_state = Arc::clone(&state);
    let poll_shutdown_rx = shutdown_rx.clone();
    let poll_handle = tokio::spawn(async move {
        if let Err(err) = run_polling_loop(poll_state, poll_shutdown_rx).await {
            error!(error = %err, "Telegram polling loop exited with error");
        }
    });

    while !*shutdown_rx.borrow() {
        if shutdown_rx.changed().await.is_err() {
            break;
        }
    }

    let _ = web_handle.await;
    let _ = poll_handle.await;
    info!("Bot shut down cleanly");
    Ok(())
}

async fn wait_for_termination_signal() -> Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sigterm = signal(SignalKind::terminate())?;
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await?;
    }
    Ok(())
}
