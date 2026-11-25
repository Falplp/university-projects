use serde::Deserialize;
use std::{fs, path::Path};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_workspace")]
    pub workspace: String,
    #[serde(default = "default_event_source")]
    pub event_source: String,
    #[serde(default = "default_database_path")]
    pub database_path: String,
    #[serde(default)]
    pub agents: Vec<AgentConfig>,
    #[serde(default)]
    pub workspaces: Vec<WorkspaceConfig>,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
    #[serde(default)]
    pub notifications: NotificationConfig,
    #[serde(default)]
    pub openclaw: OpenClawConfig,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    pub id: String,
    pub role: String,
    pub model: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub soul_file: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceConfig {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub agents: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SecurityConfig {
    #[serde(default = "default_workspace_root")]
    pub workspace_root: String,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub allow_terminal: bool,
    #[serde(default = "default_terminal_allowlist")]
    pub terminal_allowlist: Vec<String>,
    #[serde(default)]
    pub validation_command: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub kind: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key_env: String,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NotificationConfig {
    #[serde(default = "default_webhook_env")]
    pub webhook_url_env: String,
    #[serde(default = "default_telegram_token_env")]
    pub telegram_token_env: String,
    #[serde(default = "default_telegram_chat_env")]
    pub telegram_chat_id_env: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenClawConfig {
    #[serde(default)]
    pub enabled: bool,
    #[allow(dead_code)]
    #[serde(default = "default_openclaw_command")]
    pub command: String,
    #[serde(default = "default_openclaw_url")]
    pub url: String,
    #[serde(default = "default_openclaw_token_env")]
    pub token_env: String,
    #[serde(default = "default_openclaw_agent")]
    pub agent_id: String,
    #[serde(default = "default_openclaw_task_input")]
    pub task_input_file: String,
    #[serde(default = "default_openclaw_task_report")]
    pub task_report_file: String,
}

impl Default for OpenClawConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            command: default_openclaw_command(),
            url: default_openclaw_url(),
            token_env: default_openclaw_token_env(),
            agent_id: default_openclaw_agent(),
            task_input_file: default_openclaw_task_input(),
            task_report_file: default_openclaw_task_report(),
        }
    }
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            webhook_url_env: default_webhook_env(),
            telegram_token_env: default_telegram_token_env(),
            telegram_chat_id_env: default_telegram_chat_env(),
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            workspace_root: default_workspace_root(),
            read_only: false,
            allow_terminal: false,
            terminal_allowlist: default_terminal_allowlist(),
            validation_command: Vec::new(),
        }
    }
}

fn default_workspace() -> String {
    "coding-space".into()
}
fn default_event_source() -> String {
    "events.jsonl".into()
}
fn default_database_path() -> String {
    "mission_control.sqlite3".into()
}
fn default_workspace_root() -> String {
    ".".into()
}
fn default_terminal_allowlist() -> Vec<String> {
    vec!["git".into(), "cargo".into(), "openclaw".into()]
}
fn default_openclaw_command() -> String {
    "openclaw".into()
}
fn default_openclaw_url() -> String {
    "ws://127.0.0.1:18789".into()
}
fn default_openclaw_token_env() -> String {
    "OPENCLAW_GATEWAY_TOKEN".into()
}
fn default_openclaw_agent() -> String {
    "main".into()
}
fn default_openclaw_task_input() -> String {
    "tasks.txt".into()
}
fn default_openclaw_task_report() -> String {
    "logs/openclaw-task-results.log".into()
}
fn default_timeout() -> u64 {
    60
}
fn default_webhook_env() -> String {
    "MISSION_CONTROL_WEBHOOK_URL".into()
}
fn default_telegram_token_env() -> String {
    "TELEGRAM_BOT_TOKEN".into()
}
fn default_telegram_chat_env() -> String {
    "TELEGRAM_CHAT_ID".into()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            workspace: default_workspace(),
            event_source: default_event_source(),
            database_path: default_database_path(),
            agents: Vec::new(),
            workspaces: Vec::new(),
            security: SecurityConfig::default(),
            providers: Vec::new(),
            models: Vec::new(),
            notifications: NotificationConfig::default(),
            openclaw: OpenClawConfig::default(),
        }
    }
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        fs::read_to_string(path)
            .or_else(|_| fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)))
            .ok()
            .and_then(|raw| toml::from_str::<Self>(&raw).ok())
            .unwrap_or_default()
            .with_environment()
    }

    fn with_environment(mut self) -> Self {
        self.apply_overrides(
            std::env::var("MISSION_CONTROL_DB").ok(),
            std::env::var("OLLAMA_BASE_URL").ok(),
            std::env::var("MISSION_CONTROL_WORKSPACE_ROOT").ok(),
        );
        self
    }

    fn apply_overrides(
        &mut self,
        database: Option<String>,
        ollama: Option<String>,
        workspace: Option<String>,
    ) {
        if let Some(database) = database {
            self.database_path = database;
        }
        if let Some(workspace) = workspace {
            self.security.workspace_root = workspace;
        }
        if let Some(ollama) = ollama {
            if let Some(provider) = self
                .providers
                .iter_mut()
                .find(|provider| provider.id == "ollama")
            {
                provider.base_url = ollama;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_config_uses_safe_defaults() {
        let config = Config::load("this-file-does-not-exist.toml");
        assert_eq!(config.workspace, "coding-space");
        assert_eq!(config.event_source, "events.jsonl");
        assert_eq!(config.database_path, "mission_control.sqlite3");
        assert!(config.agents.is_empty());
        assert!(config.workspaces.is_empty());
        assert_eq!(config.security.workspace_root, ".");
        assert_eq!(
            config.security.terminal_allowlist,
            ["git", "cargo", "openclaw"]
        );
        assert!(!config.openclaw.enabled);
        assert_eq!(config.openclaw.task_input_file, "tasks.txt");
        assert_eq!(
            config.openclaw.task_report_file,
            "logs/openclaw-task-results.log"
        );
        assert!(config.providers.is_empty());
        assert!(config.models.is_empty());
    }

    #[test]
    fn workspace_config_contains_four_pipeline_agents() {
        let config = Config::load("config.toml");
        assert_eq!(config.workspace, "coding-space");
        assert_eq!(config.agents.len(), 4);
        assert_eq!(config.agents[0].provider, "openai");
        assert_eq!(config.agents[0].model, "gpt-4o");
        assert_eq!(config.agents[1].provider, "ollama");
        assert_eq!(config.workspaces.len(), 4);
        assert_eq!(config.providers.len(), 4);
        assert_eq!(config.models.len(), 4);
        assert!(config.openclaw.enabled);
        assert_eq!(config.openclaw.url, "ws://127.0.0.1:18789");
        assert_eq!(config.openclaw.token_env, "OPENCLAW_GATEWAY_TOKEN");
        assert_eq!(config.openclaw.agent_id, "main");
        assert_eq!(config.openclaw.task_input_file, "tasks.txt");
        assert_eq!(
            config.openclaw.task_report_file,
            "logs/openclaw-task-results.log"
        );
    }

    #[test]
    fn deployment_overrides_change_database_workspace_and_ollama() {
        let mut config = Config::load("config.toml");
        config.apply_overrides(
            Some("/data/app.db".into()),
            Some("http://host:11434".into()),
            Some("/workspace".into()),
        );
        assert_eq!(config.database_path, "/data/app.db");
        assert_eq!(config.security.workspace_root, "/workspace");
        assert_eq!(
            config
                .providers
                .iter()
                .find(|provider| provider.id == "ollama")
                .unwrap()
                .base_url,
            "http://host:11434"
        );
    }
}
