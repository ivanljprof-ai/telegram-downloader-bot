use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::telegram::TelegramClient;

static URL_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"https?://(?:www\.)?[-a-zA-Z0-9@:%._+~#=]{1,256}\.[a-zA-Z0-9()]{1,6}\b(?:[-a-zA-Z0-9()@:%_+.~#?&//=]*)",
    )
    .expect("valid URL_REGEX")
});

static SPOTIFY_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"https?://(?:open|play)\.spotify\.com/(?:track|album|playlist|artist)/[a-zA-Z0-9?=&_-]+",
    )
    .expect("valid SPOTIFY_REGEX")
});

static INSTAGRAM_STORIES_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"https?://(?:www\.)?instagram\.com/stories/([a-zA-Z0-9_.]+)")
        .expect("valid INSTAGRAM_STORIES_REGEX")
});

static INSTAGRAM_POST_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"https?://(?:www\.)?instagram\.com/(?:p|reel|reels|tv)/([a-zA-Z0-9_-]+)")
        .expect("valid INSTAGRAM_POST_REGEX")
});

static INSTAGRAM_PROFILE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"https?://(?:www\.)?instagram\.com/([a-zA-Z0-9_.]+)/?")
        .expect("valid INSTAGRAM_PROFILE_REGEX")
});

static TIKTOK_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://(?:www\.|vm\.|vt\.)?tiktok\.com/.*").expect("tiktok"));
static TWITTER_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://(?:www\.)?(?:twitter|x)\.com/.*").expect("twitter"));
static REDDIT_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://(?:www\.|v\.)?reddit\.com/.*").expect("reddit"));
static FACEBOOK_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"https?://(?:www\.|m\.|fb\.)?(?:facebook\.com|fb\.watch)/.*").expect("facebook")
});
static PINTEREST_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"https?://(?:www\.|pin\.)?pinterest\.[a-z.]+/.*").expect("pinterest")
});
static TWITCH_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://(?:www\.|clips\.)?twitch\.tv/.*").expect("twitch"));
static SOUNDCLOUD_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://(?:www\.|m\.)?soundcloud\.com/.*").expect("soundcloud"));
static VIMEO_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://(?:www\.)?vimeo\.com/.*").expect("vimeo"));
static PERCENT_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[download\]\s+(\d+(?:\.\d+)?)%").expect("percent"));

/// Detected download action for a single URL or reference in a user message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlAction {
    Spotify(String),
    InstagramStories(String),
    InstagramPost {
        url: String,
        shortcode: Option<String>,
    },
    DirectMp3 {
        url: String,
        platform: &'static str,
    },
    PromptFormat {
        url: String,
        platform: &'static str,
    },
}

/// High-level interpretation of an incoming non-command text message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageAnalysis {
    Actions(Vec<UrlAction>),
    NoValidUrl,
}

/// Summary of files and bytes inside a download directory.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CleanStats {
    pub file_count: usize,
    pub total_bytes: u64,
}

#[derive(Debug, Deserialize)]
struct YtDlpMetadata {
    title: Option<String>,
    duration: Option<f64>,
    uploader: Option<String>,
    view_count: Option<u64>,
    extractor_key: Option<String>,
}

/// Detects the friendly platform label for a generic media URL.
#[must_use]
pub fn detect_platform(url: &str) -> &'static str {
    let lower = url.to_ascii_lowercase();
    if TIKTOK_REGEX.is_match(&lower) {
        "TikTok"
    } else if TWITTER_REGEX.is_match(&lower) {
        "X (Twitter)"
    } else if REDDIT_REGEX.is_match(&lower) {
        "Reddit"
    } else if FACEBOOK_REGEX.is_match(&lower) {
        "Facebook"
    } else if PINTEREST_REGEX.is_match(&lower) {
        "Pinterest"
    } else if TWITCH_REGEX.is_match(&lower) {
        "Twitch"
    } else if SOUNDCLOUD_REGEX.is_match(&lower) {
        "SoundCloud"
    } else if VIMEO_REGEX.is_match(&lower) {
        "Vimeo"
    } else if lower.contains("youtube.com") || lower.contains("youtu.be") {
        "YouTube"
    } else {
        "Vídeo/Audio"
    }
}

/// Extracts all HTTP/HTTPS URLs from a message string.
#[must_use]
pub fn extract_urls(text: &str) -> Vec<String> {
    URL_REGEX
        .find_iter(text)
        .map(|m| m.as_str().to_string())
        .collect()
}

/// Analyzes incoming message text and classifies platform-specific download actions.
#[must_use]
pub fn analyze_message_text(text: &str) -> MessageAnalysis {
    let trimmed = text.trim();

    if let Some(m) = SPOTIFY_REGEX.find(trimmed) {
        return MessageAnalysis::Actions(vec![UrlAction::Spotify(m.as_str().to_string())]);
    }

    if let Some(caps) = INSTAGRAM_STORIES_REGEX.captures(trimmed)
        && let Some(user) = caps.get(1)
    {
        return MessageAnalysis::Actions(vec![UrlAction::InstagramStories(
            user.as_str().to_string(),
        )]);
    }

    if let Some(caps) = INSTAGRAM_POST_REGEX.captures(trimmed) {
        let full = caps.get(0).map_or("", |m| m.as_str()).to_string();
        let shortcode = caps.get(1).map(|m| m.as_str().to_string());
        return MessageAnalysis::Actions(vec![UrlAction::InstagramPost {
            url: full,
            shortcode,
        }]);
    }

    let urls = extract_urls(trimmed);
    if urls.is_empty() {
        if let Some(rest) = trimmed.strip_prefix('@') {
            let username = rest.trim();
            if !username.is_empty() {
                return MessageAnalysis::Actions(vec![UrlAction::InstagramStories(
                    username.to_string(),
                )]);
            }
        }
        return MessageAnalysis::NoValidUrl;
    }

    let explicit_mp3 = trimmed.to_ascii_lowercase().contains("mp3");
    let mut actions = Vec::with_capacity(urls.len());

    for url in urls {
        let lower = url.to_ascii_lowercase();
        if lower.contains("spotify.com") {
            actions.push(UrlAction::Spotify(url));
        } else if lower.contains("instagram.com") {
            if let Some(caps) = INSTAGRAM_PROFILE_REGEX.captures(&url)
                && let Some(user_match) = caps.get(1)
            {
                let user = user_match.as_str();
                if !matches!(user, "p" | "reel" | "reels" | "stories" | "tv") {
                    actions.push(UrlAction::InstagramStories(user.to_string()));
                    continue;
                }
            }
            actions.push(UrlAction::InstagramPost {
                url,
                shortcode: None,
            });
        } else {
            let platform = detect_platform(&url);
            if explicit_mp3 {
                actions.push(UrlAction::DirectMp3 { url, platform });
            } else {
                actions.push(UrlAction::PromptFormat { url, platform });
            }
        }
    }

    MessageAnalysis::Actions(actions)
}

/// Parses a `yt-dlp` progress percentage from a stdout line (`[download]  42.5% ...`).
#[must_use]
pub fn parse_progress_percent(line: &str) -> Option<f64> {
    let caps = PERCENT_REGEX.captures(line)?;
    let val = caps.get(1)?.as_str().parse::<f64>().ok()?;
    if val.is_finite() {
        Some(val.clamp(0.0, 100.0))
    } else {
        None
    }
}

/// Computes the 10%-quantized step (`0, 10, ..., 100`) for a percentage.
#[must_use]
pub fn progress_step(pct: f64) -> u8 {
    let clamped = pct.clamp(0.0, 100.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let buckets = (clamped / 10.0).floor() as u8;
    buckets.min(10) * 10
}

/// Renders a 10-block visual progress bar like `██████░░░░`.
#[must_use]
pub fn render_progress_bar(pct: f64) -> String {
    let clamped = pct.clamp(0.0, 100.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let filled = ((clamped / 10.0).floor() as usize).min(10);
    let empty = 10 - filled;
    let mut bar = String::with_capacity(30);
    for _ in 0..filled {
        bar.push('█');
    }
    for _ in 0..empty {
        bar.push('░');
    }
    bar
}

/// Determines whether a progress update should be emitted based on step increase and throttle interval.
#[must_use]
pub fn should_report_progress(
    step: u8,
    last_reported_step: u8,
    last_edit: Option<Instant>,
    now: Instant,
    min_interval: Duration,
) -> bool {
    if step <= last_reported_step || step > 100 {
        return false;
    }
    match last_edit {
        None => true,
        Some(prev) => now.saturating_duration_since(prev) >= min_interval,
    }
}

/// Formats an unsigned integer with comma thousand separators (e.g. `1,234,567`).
#[must_use]
pub fn format_thousands(n: u64) -> String {
    let digits = n.to_string();
    let len = digits.len();
    let mut out = String::with_capacity(len + len / 3);
    for (idx, ch) in digits.chars().enumerate() {
        if idx > 0 && (len - idx).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Formats video duration seconds into `Xm Ys` or `Desconocida`.
#[must_use]
pub fn format_duration(duration: Option<f64>) -> String {
    match duration {
        Some(secs) if secs.is_finite() && secs > 0.0 => {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let total = secs.round() as u64;
            let mins = total / 60;
            let rem = total % 60;
            format!("{mins}m {rem}s")
        }
        _ => "Desconocida".to_string(),
    }
}

/// Formats a byte count into MB or GB string matching the original `/clean` output.
#[must_use]
pub fn format_size_mb_or_gb(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let bytes_f = bytes as f64;
    let gb = bytes_f / (1024.0 * 1024.0 * 1024.0);
    if gb >= 1.0 {
        format!("{gb:.2} GB")
    } else {
        let mb = bytes_f / (1024.0 * 1024.0);
        format!("{mb:.2} MB")
    }
}

/// Formats `Uptime` seconds as `Xh Ym Zs`.
#[must_use]
pub fn format_uptime(uptime_secs: u64) -> String {
    let hours = uptime_secs / 3600;
    let minutes = (uptime_secs % 3600) / 60;
    let seconds = uptime_secs % 60;
    format!("{hours}h {minutes}m {seconds}s")
}

/// Parses JSON output from `yt-dlp --dump-json` into the formatted Telegram `/info` text.
///
/// # Errors
/// Returns an error if the JSON payload cannot be parsed.
pub fn parse_ytdlp_info_json(json_str: &str, url: &str) -> Result<String> {
    let data: YtDlpMetadata =
        serde_json::from_str(json_str).context("failed to parse yt-dlp metadata JSON")?;

    let title = data
        .title
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Desconocido".to_string())
        .replace('`', "'");
    let uploader = data
        .uploader
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Desconocido".to_string());
    let extractor = data
        .extractor_key
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "General".to_string());
    let dur_str = format_duration(data.duration);
    let views_str = match data.view_count {
        Some(v) if v > 0 => format_thousands(v),
        _ => "No disponible".to_string(),
    };

    Ok(format!(
        "ℹ️ **Metadatos de Enlace** ({extractor})\n\n\
         📌 **Título:** `{title}`\n\
         👤 **Canal/Autor:** {uploader}\n\
         ⏱️ **Duración:** {dur_str}\n\
         👁️ **Visualizaciones:** {views_str}\n\
         🔗 **URL:** {url}"
    ))
}

/// Checks whether an external CLI binary is present in `PATH`.
#[must_use]
pub fn is_command_available(name: &str) -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path_var).any(|dir| {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return true;
        }
        #[cfg(windows)]
        {
            if dir.join(format!("{name}.exe")).is_file() {
                return true;
            }
        }
        false
    })
}

/// Returns the first existing Instagram `cookies.txt` file from candidate paths.
#[must_use]
pub fn find_instagram_cookies(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|p| p.is_file()).cloned()
}

/// Builds the `yt-dlp` CLI argument vector for MP3 or MP4 downloads.
#[must_use]
pub fn build_ytdlp_args(
    url: &str,
    download_dir: &Path,
    is_mp3: bool,
    use_aria2: bool,
) -> Vec<String> {
    let mut args = Vec::with_capacity(20);
    args.push("--no-playlist".to_string());

    if is_mp3 {
        args.extend([
            "-x".to_string(),
            "--audio-format".to_string(),
            "mp3".to_string(),
            "--audio-quality".to_string(),
            "0".to_string(),
        ]);
    } else {
        args.extend([
            "-f".to_string(),
            "bestvideo+bestaudio/best".to_string(),
            "--merge-output-format".to_string(),
            "mp4".to_string(),
            "--recode-video".to_string(),
            "mp4".to_string(),
        ]);
    }

    if use_aria2 {
        args.extend([
            "--downloader".to_string(),
            "aria2c".to_string(),
            "--downloader-args".to_string(),
            "aria2c:-x 16 -s 16 -k 1M".to_string(),
        ]);
    }

    let output_template = if is_mp3 {
        download_dir.join("%(title)s [%(id)s].mp3")
    } else {
        download_dir.join("%(title)s [%(id)s].%(ext)s")
    };

    args.extend([
        "--concurrent-fragments".to_string(),
        "16".to_string(),
        "--newline".to_string(),
        "--print".to_string(),
        "title".to_string(),
        "--no-simulate".to_string(),
        "-o".to_string(),
        output_template.to_string_lossy().into_owned(),
        url.to_string(),
    ]);

    args
}

/// Recursively inspects files and total byte size inside `dir`.
///
/// # Errors
/// Returns an error if the blocking task panics.
pub async fn inspect_download_dir(dir: &Path) -> Result<CleanStats> {
    let dir_owned = dir.to_path_buf();
    tokio::task::spawn_blocking(move || inspect_dir_sync(&dir_owned))
        .await
        .context("inspect_download_dir task failed")
}

/// Recursively deletes all files inside `dir` and reports count and freed bytes.
///
/// # Errors
/// Returns an error if the blocking task panics.
pub async fn purge_download_dir(dir: &Path) -> Result<CleanStats> {
    let dir_owned = dir.to_path_buf();
    tokio::task::spawn_blocking(move || purge_dir_sync(&dir_owned))
        .await
        .context("purge_download_dir task failed")
}

fn inspect_dir_sync(root: &Path) -> CleanStats {
    if !root.exists() {
        return CleanStats::default();
    }
    let mut stats = CleanStats::default();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() {
                stats.file_count += 1;
                stats.total_bytes = stats.total_bytes.saturating_add(meta.len());
            }
        }
    }
    stats
}

fn purge_dir_sync(root: &Path) -> CleanStats {
    if !root.exists() {
        return CleanStats::default();
    }
    let mut stats = CleanStats::default();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() {
                let len = meta.len();
                if std::fs::remove_file(&path).is_ok() {
                    stats.file_count += 1;
                    stats.total_bytes = stats.total_bytes.saturating_add(len);
                }
            }
        }
    }
    stats
}

/// Queries video metadata using `yt-dlp --dump-json --no-playlist`.
///
/// # Errors
/// Returns an error if `yt-dlp` cannot be executed, exits non-zero, or returns invalid JSON.
pub async fn fetch_video_info(url: &str) -> Result<String> {
    let output = Command::new("yt-dlp")
        .args(["--dump-json", "--no-playlist", url])
        .output()
        .await
        .context("failed to execute yt-dlp")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("yt-dlp metadata lookup failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8(output.stdout).context("yt-dlp stdout is not valid UTF-8")?;
    parse_ytdlp_info_json(&stdout, url)
}

async fn send_or_edit_status(
    telegram: &TelegramClient,
    chat_id: i64,
    existing_msg_id: Option<i64>,
    text: &str,
    parse_mode: Option<&str>,
) -> Option<i64> {
    if let Some(msg_id) = existing_msg_id
        && telegram
            .edit_message_text(chat_id, msg_id, text, parse_mode, None)
            .await
            .is_ok()
    {
        return Some(msg_id);
    }
    telegram
        .send_message(chat_id, text, parse_mode, None)
        .await
        .ok()
        .map(|m| m.message_id)
}

async fn update_final_status(
    telegram: &TelegramClient,
    chat_id: i64,
    status_msg_id: Option<i64>,
    text: &str,
    parse_mode: Option<&str>,
) {
    if let Some(msg_id) = status_msg_id
        && telegram
            .edit_message_text(chat_id, msg_id, text, parse_mode, None)
            .await
            .is_ok()
    {
        return;
    }
    let _ = telegram.send_message(chat_id, text, parse_mode, None).await;
}

/// Runs a generic `yt-dlp` download (MP4 or MP3) with real-time 10-block progress edits.
pub async fn run_single_download(
    telegram: &TelegramClient,
    chat_id: i64,
    existing_status_msg_id: Option<i64>,
    url: &str,
    download_dir: &Path,
    fmt: &str,
) {
    let is_mp3 = fmt.eq_ignore_ascii_case("mp3");
    let fmt_label = if is_mp3 {
        "🎵 MP3"
    } else {
        "📹 MP4 (Máxima Resolución)"
    };
    let initial_text = format!("🚀 Iniciando descarga de YouTube (0%) 📥\nFormato: {fmt_label}");

    let status_msg_id = send_or_edit_status(
        telegram,
        chat_id,
        existing_status_msg_id,
        &initial_text,
        None,
    )
    .await;

    if let Err(err) = tokio::fs::create_dir_all(download_dir).await {
        error!(error = %err, dir = %download_dir.display(), "Failed to create download directory");
        update_final_status(
            telegram,
            chat_id,
            status_msg_id,
            "❌ No se pudo crear el directorio de descargas ⚠️",
            None,
        )
        .await;
        return;
    }

    let use_aria2 = is_command_available("aria2c");
    let args = build_ytdlp_args(url, download_dir, is_mp3, use_aria2);
    info!(
        format = fmt_label,
        aria2 = use_aria2,
        "Starting yt-dlp download"
    );

    let mut child = match Command::new("yt-dlp")
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(err) => {
            error!(error = %err, "Failed to spawn yt-dlp");
            update_final_status(
                telegram,
                chat_id,
                status_msg_id,
                "❌ Hubo un error en la descarga de YouTube ⚠️",
                None,
            )
            .await;
            return;
        }
    };

    let Some(stdout) = child.stdout.take() else {
        update_final_status(
            telegram,
            chat_id,
            status_msg_id,
            "❌ Hubo un error en la descarga de YouTube ⚠️",
            None,
        )
        .await;
        return;
    };

    let stderr_handle = child.stderr.take().map(|mut stderr| {
        tokio::spawn(async move {
            let mut buf = String::new();
            let _ = stderr.read_to_string(&mut buf).await;
            buf
        })
    });

    let mut reader = BufReader::new(stdout).lines();
    let mut video_title: Option<String> = None;
    let mut last_reported_step: u8 = 0;
    let mut last_edit_time: Option<Instant> = None;

    while let Ok(Some(raw_line)) = reader.next_line().await {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if video_title.is_none()
            && !line.starts_with("[download]")
            && !line.starts_with("[ffmpeg]")
            && !line.starts_with("[ExtractAudio]")
        {
            video_title = Some(line.to_string());
            continue;
        }

        if let Some(pct) = parse_progress_percent(line) {
            let step = progress_step(pct);
            let now = Instant::now();
            if should_report_progress(
                step,
                last_reported_step,
                last_edit_time,
                now,
                Duration::from_secs(1),
            ) {
                last_reported_step = step;
                last_edit_time = Some(now);
                let bar = render_progress_bar(pct);
                let title_suffix = video_title.as_deref().unwrap_or("");
                let msg =
                    format!("⏳ Descargando ({fmt_label}):\n`[{bar}] {pct:.1}%`\n{title_suffix}");
                if let Some(msg_id) = status_msg_id {
                    let _ = telegram
                        .edit_message_text(chat_id, msg_id, &msg, None, None)
                        .await;
                }
            }
        }
    }

    let status = child.wait().await;
    let stderr_text = if let Some(handle) = stderr_handle {
        handle.await.unwrap_or_default()
    } else {
        String::new()
    };

    match status {
        Ok(exit) if exit.success() => {
            let final_title = video_title.as_deref().unwrap_or("Archivo");
            info!(title = final_title, format = fmt_label, "Download finished");
            let finish_text = if is_mp3 {
                format!("🎉 Se ha terminado este audio: {final_title} 🎵")
            } else {
                format!("🎉 Se ha terminado este vídeo: {final_title} ✅")
            };
            update_final_status(telegram, chat_id, status_msg_id, &finish_text, None).await;
        }
        Ok(exit) => {
            error!(code = ?exit.code(), stderr = %stderr_text.trim(), "yt-dlp failed");
            update_final_status(
                telegram,
                chat_id,
                status_msg_id,
                "❌ Hubo un error en la descarga de YouTube ⚠️",
                None,
            )
            .await;
        }
        Err(err) => {
            error!(error = %err, "Failed waiting for yt-dlp");
            update_final_status(
                telegram,
                chat_id,
                status_msg_id,
                "❌ Hubo un error en la descarga de YouTube ⚠️",
                None,
            )
            .await;
        }
    }
}

/// Runs a Spotify download using `spotdl`, falling back to `yt-dlp` on failure.
pub async fn run_spotify_download(
    telegram: &TelegramClient,
    chat_id: i64,
    existing_status_msg_id: Option<i64>,
    url: &str,
    download_dir: &Path,
) {
    let initial_text = "🚀 Descargando música de Spotify (buscando metadatos y carátulas)... 🎵";
    let status_msg_id = send_or_edit_status(
        telegram,
        chat_id,
        existing_status_msg_id,
        initial_text,
        None,
    )
    .await;

    let spotify_dir = download_dir.join("spotify");
    if let Err(err) = tokio::fs::create_dir_all(&spotify_dir).await {
        error!(error = %err, "Failed to create spotify directory");
        update_final_status(
            telegram,
            chat_id,
            status_msg_id,
            "❌ Hubo un error al descargar desde Spotify ⚠️",
            None,
        )
        .await;
        return;
    }

    let output_template = spotify_dir
        .join("{artist} - {title}.{output-ext}")
        .to_string_lossy()
        .into_owned();

    info!(url = url, "Starting Spotify download");
    let mut spotdl_success = false;

    if let Ok(mut child) = Command::new("spotdl")
        .args(["download", url, "--output", &output_template])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        let stderr_handle = child.stderr.take().map(|mut stderr| {
            tokio::spawn(async move {
                let mut buf = String::new();
                let _ = stderr.read_to_string(&mut buf).await;
                buf
            })
        });

        if let Some(stdout) = child.stdout.take() {
            let mut reader = BufReader::new(stdout).lines();
            let mut last_edit_time: Option<Instant> = None;

            while let Ok(Some(raw_line)) = reader.next_line().await {
                let line = raw_line.trim();
                if line.is_empty() {
                    continue;
                }
                let now = Instant::now();
                let due = last_edit_time.is_none_or(|prev| {
                    now.saturating_duration_since(prev) >= Duration::from_secs(2)
                });
                if due
                    && (line.contains("Downloaded")
                        || line.contains("Processing")
                        || line.contains("Found")
                        || line.contains("Downloading"))
                {
                    last_edit_time = Some(now);
                    let clean_line: String = line
                        .replace(['[', ']', '`'], "")
                        .chars()
                        .take(100)
                        .collect();
                    let msg = format!("⏳ Descargando de Spotify 🎵:\n`{clean_line}`");
                    if let Some(msg_id) = status_msg_id {
                        let _ = telegram
                            .edit_message_text(chat_id, msg_id, &msg, Some("Markdown"), None)
                            .await;
                    }
                }
            }
        }

        let status = child.wait().await;
        let stderr_text = if let Some(handle) = stderr_handle {
            handle.await.unwrap_or_default()
        } else {
            String::new()
        };

        if let Ok(exit) = status
            && exit.success()
        {
            spotdl_success = true;
        } else {
            warn!(stderr = %stderr_text.trim(), "spotdl failed, attempting yt-dlp fallback");
        }
    } else {
        warn!("spotdl binary not found, attempting yt-dlp fallback");
    }

    if !spotdl_success {
        let fallback_template = spotify_dir
            .join("%(title)s [%(id)s].mp3")
            .to_string_lossy()
            .into_owned();
        if let Ok(output) = Command::new("yt-dlp")
            .args([
                "-x",
                "--audio-format",
                "mp3",
                "--audio-quality",
                "0",
                "-o",
                &fallback_template,
                url,
            ])
            .output()
            .await
            && output.status.success()
        {
            spotdl_success = true;
        }
    }

    if spotdl_success {
        let finish_text = "🎉 ¡Música de Spotify descargada con éxito! (Carátula y etiquetas MP3 integradas) 🎵✅";
        update_final_status(telegram, chat_id, status_msg_id, finish_text, None).await;
    } else {
        update_final_status(
            telegram,
            chat_id,
            status_msg_id,
            "❌ Hubo un error al descargar desde Spotify ⚠️",
            None,
        )
        .await;
    }
}

/// Downloads active Instagram stories for a username via `instaloader` CLI or `yt-dlp` fallback.
pub async fn run_instagram_stories(
    telegram: &TelegramClient,
    chat_id: i64,
    existing_status_msg_id: Option<i64>,
    raw_username: &str,
    config: &Config,
) {
    let username = raw_username.trim().trim_start_matches('@');
    let initial_text =
        format!("🚀 Buscando e iniciando descarga de historias para @{username}... 📸");
    let status_msg_id = send_or_edit_status(
        telegram,
        chat_id,
        existing_status_msg_id,
        &initial_text,
        None,
    )
    .await;

    let target_name = format!("{username}_stories");
    let target_dir = config.download_dir.join(&target_name);
    let before_count = inspect_dir_sync(&target_dir).file_count;

    let cookies = find_instagram_cookies(&config.cookie_candidates());
    let mut succeeded = false;
    let mut combined_err = String::new();

    // Primary attempt: instaloader CLI
    if is_command_available("instaloader") {
        let dirname_pattern = config
            .download_dir
            .join("{target}")
            .to_string_lossy()
            .into_owned();
        let mut cmd = Command::new("instaloader");
        cmd.args([
            "--stories",
            "--no-posts",
            "--no-profile-pic",
            "--no-video-thumbnails",
            "--no-captions",
            "--no-metadata-json",
            "--quiet",
            "--dirname-pattern",
            &dirname_pattern,
            "--filename-pattern",
            "{date_utc:_%Y%m%d_%H%M%S}_{shortcode}",
        ]);
        if let (Some(u), Some(p)) = (&config.instagram_user, &config.instagram_password) {
            cmd.args(["--login", u, "--password", p]);
        }
        cmd.arg("--").arg(username);

        match cmd.output().await {
            Ok(out) if out.status.success() => {
                succeeded = true;
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let stdout = String::from_utf8_lossy(&out.stdout);
                let _ = write!(combined_err, "{stderr} {stdout}");
                warn!(username = username, err = %combined_err.trim(), "instaloader failed for stories");
            }
            Err(err) => {
                let _ = write!(combined_err, "{err}");
            }
        }
    }

    // Fallback: yt-dlp with cookies.txt if available
    if !succeeded {
        let _ = tokio::fs::create_dir_all(&target_dir).await;
        let output_template = target_dir
            .join("%(title)s [%(id)s].%(ext)s")
            .to_string_lossy()
            .into_owned();
        let story_url = format!("https://www.instagram.com/stories/{username}/");
        let mut cmd = Command::new("yt-dlp");
        cmd.arg("--newline");
        if let Some(cookie_file) = &cookies {
            cmd.arg("--cookies").arg(cookie_file);
        }
        cmd.args(["-o", &output_template, &story_url]);

        if let Ok(out) = cmd.output().await {
            if out.status.success() {
                succeeded = true;
            } else {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let _ = write!(combined_err, " {}", stderr.trim());
            }
        }
    }

    let finish_text = if succeeded {
        let after_count = inspect_dir_sync(&target_dir).file_count;
        let downloaded = after_count.saturating_sub(before_count);
        if downloaded > 0 {
            format!("🎉 ¡Se han descargado {downloaded} historias de @{username}! 📸✨")
        } else {
            format!(
                "ℹ️ @{username} no tiene historias activas en este momento (o el perfil es privado) 🔒"
            )
        }
    } else {
        let lower_err = combined_err.to_ascii_lowercase();
        if lower_err.contains("does not exist") || lower_err.contains("not found") {
            format!("❌ El perfil de Instagram @{username} no existe.")
        } else if lower_err.contains("login")
            || lower_err.contains("redirect")
            || lower_err.contains("404")
            || lower_err.contains("cookies")
        {
            format!(
                "🔒 Se requiere iniciar sesión en Instagram para ver las historias de @{username}."
            )
        } else {
            format!("❌ Error al descargar historias de @{username} ⚠️")
        }
    };

    update_final_status(telegram, chat_id, status_msg_id, &finish_text, None).await;
}

/// Downloads an Instagram post or reel via `instaloader` CLI or `yt-dlp` fallback.
pub async fn run_instagram_post(
    telegram: &TelegramClient,
    chat_id: i64,
    existing_status_msg_id: Option<i64>,
    url: &str,
    shortcode: Option<&str>,
    config: &Config,
) {
    let initial_text = "🚀 Descargando publicación/reel de Instagram... 📸";
    let status_msg_id = send_or_edit_status(
        telegram,
        chat_id,
        existing_status_msg_id,
        initial_text,
        None,
    )
    .await;

    let target_dir = config.download_dir.join("instagram");
    let _ = tokio::fs::create_dir_all(&target_dir).await;
    let cookies = find_instagram_cookies(&config.cookie_candidates());

    let mut download_success = false;

    if let Some(code) = shortcode
        && is_command_available("instaloader")
    {
        let dirname = target_dir.to_string_lossy().into_owned();
        let mut cmd = Command::new("instaloader");
        cmd.args([
            "--no-video-thumbnails",
            "--no-captions",
            "--no-metadata-json",
            "--quiet",
            "--dirname-pattern",
            &dirname,
            "--filename-pattern",
            "{date_utc:_%Y%m%d_%H%M%S}_{shortcode}",
        ]);
        if let (Some(u), Some(p)) = (&config.instagram_user, &config.instagram_password) {
            cmd.args(["--login", u, "--password", p]);
        }
        cmd.arg("--").arg(format!("-{code}"));

        if let Ok(out) = cmd.output().await
            && out.status.success()
        {
            download_success = true;
        }
    }

    if !download_success {
        let output_template = target_dir
            .join("%(title)s [%(id)s].%(ext)s")
            .to_string_lossy()
            .into_owned();
        let mut cmd = Command::new("yt-dlp");
        cmd.arg("--newline");
        if let Some(cookie_file) = &cookies {
            cmd.arg("--cookies").arg(cookie_file);
        }
        cmd.args(["-o", &output_template, url]);

        match cmd.output().await {
            Ok(out) if out.status.success() => {
                download_success = true;
            }
            Ok(out) => {
                let err = String::from_utf8_lossy(&out.stderr);
                error!(url = url, stderr = %err.trim(), "yt-dlp failed for Instagram post");
            }
            Err(err) => {
                error!(url = url, error = %err, "Failed to run yt-dlp for Instagram post");
            }
        }
    }

    let finish_text = if download_success {
        "🎉 ¡Contenido de Instagram descargado con éxito! 📸✅"
    } else {
        "❌ No se pudo descargar la publicación de Instagram ⚠️"
    };

    update_final_status(telegram, chat_id, status_msg_id, finish_text, None).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn detect_platform_identifies_all_supported_networks() {
        assert_eq!(
            detect_platform("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            "YouTube"
        );
        assert_eq!(detect_platform("https://youtu.be/dQw4w9WgXcQ"), "YouTube");
        assert_eq!(
            detect_platform("https://vm.tiktok.com/ZM6abc123/"),
            "TikTok"
        );
        assert_eq!(
            detect_platform("https://x.com/rustlang/status/12345"),
            "X (Twitter)"
        );
        assert_eq!(
            detect_platform("https://twitter.com/user/status/12345"),
            "X (Twitter)"
        );
        assert_eq!(
            detect_platform("https://www.reddit.com/r/rust/comments/abc/title"),
            "Reddit"
        );
        assert_eq!(detect_platform("https://fb.watch/abc123/"), "Facebook");
        assert_eq!(
            detect_platform("https://www.pinterest.es/pin/12345/"),
            "Pinterest"
        );
        assert_eq!(
            detect_platform("https://clips.twitch.tv/ClipName"),
            "Twitch"
        );
        assert_eq!(
            detect_platform("https://soundcloud.com/artist/track"),
            "SoundCloud"
        );
        assert_eq!(detect_platform("https://vimeo.com/123456"), "Vimeo");
        assert_eq!(
            detect_platform("https://example.com/video.mp4"),
            "Vídeo/Audio"
        );
    }

    #[test]
    fn analyze_message_text_routes_spotify_instagram_urls_and_handles() {
        assert_eq!(
            analyze_message_text("https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC"),
            MessageAnalysis::Actions(vec![UrlAction::Spotify(
                "https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC".to_string()
            )])
        );

        assert_eq!(
            analyze_message_text("https://www.instagram.com/stories/natgeo/12345/"),
            MessageAnalysis::Actions(vec![UrlAction::InstagramStories("natgeo".to_string())])
        );

        assert_eq!(
            analyze_message_text("https://www.instagram.com/reel/Cxyz123_abc/"),
            MessageAnalysis::Actions(vec![UrlAction::InstagramPost {
                url: "https://www.instagram.com/reel/Cxyz123_abc".to_string(),
                shortcode: Some("Cxyz123_abc".to_string()),
            }])
        );

        assert_eq!(
            analyze_message_text("https://www.instagram.com/nasa/"),
            MessageAnalysis::Actions(vec![UrlAction::InstagramStories("nasa".to_string())])
        );

        assert_eq!(
            analyze_message_text("@nasa"),
            MessageAnalysis::Actions(vec![UrlAction::InstagramStories("nasa".to_string())])
        );

        assert_eq!(
            analyze_message_text("descarga esto mp3 https://youtu.be/dQw4w9WgXcQ"),
            MessageAnalysis::Actions(vec![UrlAction::DirectMp3 {
                url: "https://youtu.be/dQw4w9WgXcQ".to_string(),
                platform: "YouTube",
            }])
        );

        assert_eq!(
            analyze_message_text("hola sin enlaces"),
            MessageAnalysis::NoValidUrl
        );
    }

    #[test]
    fn progress_bar_and_step_throttling_behave_correctly() {
        assert_eq!(
            parse_progress_percent("[download]  47.8% of 10.00MiB"),
            Some(47.8)
        );
        assert_eq!(parse_progress_percent("[ffmpeg] Merging formats"), None);

        assert_eq!(progress_step(0.0), 0);
        assert_eq!(progress_step(9.9), 0);
        assert_eq!(progress_step(10.0), 10);
        assert_eq!(progress_step(55.4), 50);
        assert_eq!(progress_step(100.0), 100);

        assert_eq!(render_progress_bar(0.0), "░░░░░░░░░░");
        assert_eq!(render_progress_bar(55.4), "█████░░░░░");
        assert_eq!(render_progress_bar(100.0), "██████████");

        let t0 = Instant::now();
        assert!(should_report_progress(
            10,
            0,
            None,
            t0,
            Duration::from_secs(1)
        ));
        assert!(!should_report_progress(
            20,
            10,
            Some(t0),
            t0 + Duration::from_millis(400),
            Duration::from_secs(1)
        ));
        assert!(should_report_progress(
            20,
            10,
            Some(t0),
            t0 + Duration::from_secs(1),
            Duration::from_secs(1)
        ));
        assert!(!should_report_progress(
            20,
            20,
            Some(t0),
            t0 + Duration::from_secs(5),
            Duration::from_secs(1)
        ));
    }

    #[test]
    fn metadata_and_number_formatting_match_expected_output() {
        assert_eq!(format_thousands(0), "0");
        assert_eq!(format_thousands(999), "999");
        assert_eq!(format_thousands(1234), "1,234");
        assert_eq!(format_thousands(1_234_567_890), "1,234,567,890");
        assert_eq!(format_duration(Some(125.0)), "2m 5s");
        assert_eq!(format_duration(None), "Desconocida");
        assert_eq!(format_uptime(3665), "1h 1m 5s");

        let sample_json = r#"{
            "title": "Test Video `Awesome`",
            "duration": 212,
            "uploader": "TestChannel",
            "view_count": 1500250,
            "extractor_key": "Youtube"
        }"#;
        let rendered = parse_ytdlp_info_json(sample_json, "https://youtu.be/abc").expect("json");
        assert!(rendered.contains("Youtube"));
        assert!(rendered.contains("Test Video 'Awesome'"));
        assert!(rendered.contains("3m 32s"));
        assert!(rendered.contains("1,500,250"));
    }

    #[tokio::test]
    async fn inspect_and_purge_download_dir_with_tempdir() {
        let tmp = TempDir::new().expect("tempdir");
        let sub = tmp.path().join("spotify");
        std::fs::create_dir_all(&sub).expect("mkdir");
        std::fs::write(tmp.path().join("video.mp4"), vec![b'a'; 1024]).expect("write");
        std::fs::write(sub.join("song.mp3"), vec![b'b'; 2048]).expect("write");

        let inspected = inspect_download_dir(tmp.path()).await.expect("inspect");
        assert_eq!(inspected.file_count, 2);
        assert_eq!(inspected.total_bytes, 3072);

        let purged = purge_download_dir(tmp.path()).await.expect("purge");
        assert_eq!(purged.file_count, 2);
        assert_eq!(purged.total_bytes, 3072);

        let after = inspect_download_dir(tmp.path()).await.expect("after");
        assert_eq!(after.file_count, 0);
        assert_eq!(after.total_bytes, 0);
    }

    #[test]
    fn build_ytdlp_args_switches_format_and_aria2() {
        let dir = Path::new("/downloads");
        let mp4_args = build_ytdlp_args("https://youtu.be/1", dir, false, true);
        assert!(mp4_args.contains(&"bestvideo+bestaudio/best".to_string()));
        assert!(mp4_args.contains(&"aria2c".to_string()));

        let mp3_args = build_ytdlp_args("https://youtu.be/1", dir, true, false);
        assert!(mp3_args.contains(&"--audio-format".to_string()));
        assert!(!mp3_args.contains(&"aria2c".to_string()));
    }
}
