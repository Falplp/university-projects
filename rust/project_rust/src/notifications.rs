use crate::config::NotificationConfig;
use reqwest::blocking::Client;
use serde_json::json;
use std::{env, time::Duration};

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn configured_value(name: &str) -> Option<String> {
    non_empty(env::var(name).ok())
}

pub fn send(config: &NotificationConfig, message: &str) -> Vec<String> {
    let client = match Client::builder().timeout(Duration::from_secs(10)).build() {
        Ok(client) => client,
        Err(error) => return vec![error.to_string()],
    };
    let mut errors = Vec::new();
    if let Some(url) = configured_value(&config.webhook_url_env) {
        if let Err(error) = client
            .post(url)
            .json(&json!({"event":"approval_required", "message":message}))
            .send()
        {
            errors.push(format!("webhook: {error}"));
        }
    }
    if let (Some(token), Some(chat_id)) = (
        configured_value(&config.telegram_token_env),
        configured_value(&config.telegram_chat_id_env),
    ) {
        let url = format!("https://api.telegram.org/bot{token}/sendMessage");
        if let Err(error) = client
            .post(url)
            .json(&json!({"chat_id":chat_id, "text":message}))
            .send()
        {
            errors.push(format!("telegram: {error}"));
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_reference_secret_environment_variables() {
        let config = NotificationConfig::default();
        assert_eq!(config.webhook_url_env, "MISSION_CONTROL_WEBHOOK_URL");
        assert_eq!(config.telegram_token_env, "TELEGRAM_BOT_TOKEN");
        assert_eq!(config.telegram_chat_id_env, "TELEGRAM_CHAT_ID");
    }

    #[test]
    fn empty_notification_values_are_treated_as_disabled() {
        assert_eq!(non_empty(None), None);
        assert_eq!(non_empty(Some("   ".into())), None);
        assert_eq!(
            non_empty(Some(" https://example.test ".into())),
            Some("https://example.test".into())
        );
    }
}
