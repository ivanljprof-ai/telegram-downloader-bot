use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Generic Telegram Bot API response envelope.
#[derive(Debug, Deserialize)]
pub struct TelegramResponse<T> {
    pub ok: bool,
    pub result: Option<T>,
    pub description: Option<String>,
    pub error_code: Option<i64>,
}

/// Telegram bot identity returned by `getMe`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct BotUser {
    pub id: i64,
    pub is_bot: bool,
    pub first_name: String,
    #[serde(default)]
    pub username: Option<String>,
}

/// Telegram user object.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct User {
    pub id: i64,
    #[serde(default)]
    pub first_name: String,
    #[serde(default)]
    pub username: Option<String>,
}

/// Telegram chat object.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Chat {
    pub id: i64,
    #[serde(rename = "type", default)]
    pub chat_type: String,
}

/// Telegram message object.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Message {
    pub message_id: i64,
    #[serde(default)]
    pub from: Option<User>,
    pub chat: Chat,
    #[serde(default)]
    pub text: Option<String>,
}

/// Telegram inline callback query object.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CallbackQuery {
    pub id: String,
    pub from: User,
    #[serde(default)]
    pub message: Option<Message>,
    #[serde(default)]
    pub data: Option<String>,
}

/// Telegram update payload returned by `getUpdates`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Update {
    pub update_id: i64,
    #[serde(default)]
    pub message: Option<Message>,
    #[serde(default)]
    pub callback_query: Option<CallbackQuery>,
}

/// Single button in an inline keyboard.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InlineKeyboardButton {
    pub text: String,
    pub callback_data: String,
}

/// Inline keyboard markup sent alongside messages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InlineKeyboardMarkup {
    pub inline_keyboard: Vec<Vec<InlineKeyboardButton>>,
}

#[derive(Serialize)]
struct GetUpdatesRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    offset: Option<i64>,
    timeout: u64,
    allowed_updates: &'a [&'a str],
}

#[derive(Serialize)]
struct SendMessageRequest<'a> {
    chat_id: i64,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_mode: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_markup: Option<&'a InlineKeyboardMarkup>,
}

#[derive(Serialize)]
struct EditMessageTextRequest<'a> {
    chat_id: i64,
    message_id: i64,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_mode: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_markup: Option<&'a InlineKeyboardMarkup>,
}

#[derive(Serialize)]
struct AnswerCallbackQueryRequest<'a> {
    callback_query_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<&'a str>,
}

/// Asynchronous HTTP client for the Telegram Bot API backed by `reqwest` + `rustls`.
#[derive(Clone, Debug)]
pub struct TelegramClient {
    client: Client,
    token: String,
    base_url: String,
}

impl TelegramClient {
    /// Creates a new Telegram Bot API client targeting `https://api.telegram.org`.
    ///
    /// # Errors
    /// Returns an error if the underlying `reqwest::Client` cannot be constructed.
    pub fn new(token: impl Into<String>) -> Result<Self> {
        Self::with_base_url(token, crate::config::DEFAULT_TELEGRAM_API_BASE)
    }

    /// Creates a new Telegram Bot API client targeting a custom base URL (useful for testing).
    ///
    /// # Errors
    /// Returns an error if the underlying `reqwest::Client` cannot be constructed.
    pub fn with_base_url(token: impl Into<String>, base_url: impl Into<String>) -> Result<Self> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .context("failed to build HTTP client")?;
        let base = base_url.into().trim_end_matches('/').to_string();
        Ok(Self {
            client,
            token: token.into(),
            base_url: base,
        })
    }

    fn endpoint(&self, method: &str) -> String {
        format!("{}/bot{}/{}", self.base_url, self.token, method)
    }

    /// Calls `getMe` to validate the token and retrieve bot identity.
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails, response JSON is invalid, or Telegram returns `ok: false`.
    pub async fn get_me(&self) -> Result<BotUser> {
        let resp = self
            .client
            .get(self.endpoint("getMe"))
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .context("Telegram getMe request failed")?;
        let payload: TelegramResponse<BotUser> = resp
            .json()
            .await
            .context("failed to parse Telegram getMe response")?;
        Self::unwrap_response("getMe", payload)
    }

    /// Long-polls Telegram for updates via `getUpdates`.
    ///
    /// # Errors
    /// Returns an error on transport failure, JSON parse failure, or API error.
    pub async fn get_updates(&self, offset: Option<i64>, timeout_secs: u64) -> Result<Vec<Update>> {
        let body = GetUpdatesRequest {
            offset,
            timeout: timeout_secs,
            allowed_updates: &["message", "callback_query"],
        };
        let resp = self
            .client
            .post(self.endpoint("getUpdates"))
            .timeout(Duration::from_secs(timeout_secs.saturating_add(15)))
            .json(&body)
            .send()
            .await
            .context("Telegram getUpdates request failed")?;
        let payload: TelegramResponse<Vec<Update>> = resp
            .json()
            .await
            .context("failed to parse Telegram getUpdates response")?;
        Self::unwrap_response("getUpdates", payload)
    }

    /// Sends a message to a Telegram chat.
    ///
    /// # Errors
    /// Returns an error if the API call fails or Telegram returns `ok: false`.
    pub async fn send_message(
        &self,
        chat_id: i64,
        text: &str,
        parse_mode: Option<&str>,
        reply_markup: Option<&InlineKeyboardMarkup>,
    ) -> Result<Message> {
        let body = SendMessageRequest {
            chat_id,
            text,
            parse_mode,
            reply_markup,
        };
        let resp = self
            .client
            .post(self.endpoint("sendMessage"))
            .json(&body)
            .send()
            .await
            .context("Telegram sendMessage request failed")?;
        let payload: TelegramResponse<Message> = resp
            .json()
            .await
            .context("failed to parse Telegram sendMessage response")?;
        Self::unwrap_response("sendMessage", payload)
    }

    /// Edits the text (and optional inline keyboard) of an existing message.
    ///
    /// Returns `Ok(None)` if Telegram reports `"message is not modified"`.
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails or Telegram returns another error.
    pub async fn edit_message_text(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        parse_mode: Option<&str>,
        reply_markup: Option<&InlineKeyboardMarkup>,
    ) -> Result<Option<Message>> {
        let body = EditMessageTextRequest {
            chat_id,
            message_id,
            text,
            parse_mode,
            reply_markup,
        };
        let resp = self
            .client
            .post(self.endpoint("editMessageText"))
            .json(&body)
            .send()
            .await
            .context("Telegram editMessageText request failed")?;
        let payload: TelegramResponse<Value> = resp
            .json()
            .await
            .context("failed to parse Telegram editMessageText response")?;

        if !payload.ok {
            let desc = payload.description.as_deref().unwrap_or("");
            if desc.contains("message is not modified") {
                return Ok(None);
            }
            bail!(
                "Telegram editMessageText error ({}): {}",
                payload.error_code.unwrap_or(0),
                desc
            );
        }

        match payload.result {
            Some(val) if val.is_object() => {
                let msg: Message =
                    serde_json::from_value(val).context("invalid Message in editMessageText")?;
                Ok(Some(msg))
            }
            _ => Ok(None),
        }
    }

    /// Acknowledges an inline button callback query.
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails or Telegram returns `ok: false`.
    pub async fn answer_callback_query(
        &self,
        callback_query_id: &str,
        text: Option<&str>,
    ) -> Result<()> {
        let body = AnswerCallbackQueryRequest {
            callback_query_id,
            text,
        };
        let resp = self
            .client
            .post(self.endpoint("answerCallbackQuery"))
            .json(&body)
            .send()
            .await
            .context("Telegram answerCallbackQuery request failed")?;
        let payload: TelegramResponse<Value> = resp
            .json()
            .await
            .context("failed to parse Telegram answerCallbackQuery response")?;
        let _ = Self::unwrap_response("answerCallbackQuery", payload)?;
        Ok(())
    }

    fn unwrap_response<T>(method: &str, payload: TelegramResponse<T>) -> Result<T> {
        if payload.ok {
            payload
                .result
                .with_context(|| format!("Telegram {method} returned ok=true without result"))
        } else {
            bail!(
                "Telegram {} error ({}): {}",
                method,
                payload.error_code.unwrap_or(0),
                payload
                    .description
                    .unwrap_or_else(|| "unknown error".into())
            )
        }
    }
}
