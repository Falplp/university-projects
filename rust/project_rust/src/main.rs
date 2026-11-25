mod config;
mod event_source;
mod llm;
mod notifications;
mod openclaw;
mod sandbox;
mod soul;
mod storage;
mod task_batch;

use config::Config;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use event_source::{AgentEvent, read_new_events};
use llm::LlmRequest;
use openclaw::{GatewayCommand, GatewayUpdate};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, Gauge, List, ListItem, Paragraph, Row, Table, Tabs, Wrap,
    },
};
use sandbox::{CommandSandbox, FileSandbox};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    io,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread,
    time::{Duration, Instant},
};
use storage::Storage;
use task_batch::TaskOutcome;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Running,
    Waiting,
    Done,
    Error,
}
impl Status {
    fn label(self) -> &'static str {
        match self {
            Self::Running => "RUNNING",
            Self::Waiting => "WAITING",
            Self::Done => "DONE",
            Self::Error => "ERROR",
        }
    }
    fn color(self) -> Color {
        match self {
            Self::Running => Color::Cyan,
            Self::Waiting => Color::Yellow,
            Self::Done => Color::Green,
            Self::Error => Color::Red,
        }
    }
}
struct Agent {
    name: String,
    role: String,
    model: String,
    status: Status,
    tokens: u32,
    duration_secs: u64,
    tool: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PipelineState {
    Running,
    AwaitingApproval,
    Completed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct QueuedTask {
    id: u64,
    text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Theme {
    Dark,
    Light,
    Starfall,
}

impl Theme {
    fn next(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Starfall,
            Self::Starfall => Self::Dark,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
            Self::Starfall => "Starfall",
        }
    }
    fn background(self) -> Color {
        match self {
            Self::Dark => Color::Reset,
            Self::Light => Color::Rgb(238, 242, 247),
            Self::Starfall => Color::Rgb(7, 8, 24),
        }
    }
    fn foreground(self) -> Color {
        match self {
            Self::Light => Color::Rgb(20, 28, 38),
            _ => Color::White,
        }
    }
    fn accent(self) -> Color {
        match self {
            Self::Dark => Color::Cyan,
            Self::Light => Color::Blue,
            Self::Starfall => Color::Magenta,
        }
    }
}

struct StepRecord {
    step: String,
    duration_secs: u64,
    tokens: u32,
    status: Status,
}

struct LlmJobResult {
    agent_id: String,
    provider_id: String,
    result: Result<llm::LlmResponse, String>,
}

struct App {
    config: Config,
    storage: Option<Storage>,
    sandbox: Option<FileSandbox>,
    command_sandbox: Option<CommandSandbox>,
    pipeline_id: String,
    workspace_index: usize,
    agents: Vec<Agent>,
    history: Vec<StepRecord>,
    events: Vec<String>,
    errors: Vec<String>,
    pipeline: PipelineState,
    selected: usize,
    tab: usize,
    elapsed: u64,
    step_elapsed: u64,
    paused: bool,
    help: bool,
    task: String,
    task_queue: VecDeque<QueuedTask>,
    next_task_id: u64,
    diff_output: String,
    input_mode: bool,
    feedback_mode: bool,
    model_mode: bool,
    model_index: usize,
    input_buffer: String,
    theme: Theme,
    event_cursor: usize,
    llm_receiver: Option<Receiver<LlmJobResult>>,
    llm_running: Option<String>,
    llm_outputs: HashMap<String, String>,
    real_pipeline: bool,
    real_stage: Option<usize>,
    real_context: String,
    pressed_keys: HashSet<KeyCode>,
    openclaw_commands: Option<Sender<GatewayCommand>>,
    openclaw_updates: Option<Receiver<GatewayUpdate>>,
    openclaw_connected: bool,
    openclaw_input_mode: bool,
    openclaw_sessions: Vec<String>,
    active_openclaw_session: Option<String>,
    openclaw_batch_queue: VecDeque<QueuedTask>,
    openclaw_batch_active: Option<QueuedTask>,
    openclaw_batch_total: usize,
    openclaw_batch_success: usize,
    openclaw_batch_errors: usize,
}

fn build_agents(config: &Config) -> Vec<Agent> {
    if config.agents.len() >= 4 {
        return config
            .agents
            .iter()
            .map(|agent| Agent {
                name: agent.id.clone(),
                role: agent.role.clone(),
                model: agent.model.clone(),
                status: Status::Waiting,
                tokens: 0,
                duration_secs: 0,
                tool: agent
                    .tools
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "none".into()),
            })
            .enumerate()
            .map(|(index, mut agent)| {
                match index {
                    0 => {
                        agent.status = Status::Done;
                        agent.tokens = 1_842;
                        agent.duration_secs = 12;
                    }
                    1 => {
                        agent.status = Status::Running;
                        agent.tokens = 4_276;
                    }
                    _ => {}
                }
                agent
            })
            .collect();
    }
    [
        ("prompt-architect", "Architect", "local-demo", "none"),
        ("code-generator", "Developer", "local-demo", "filesystem"),
        ("code-reviewer", "Reviewer", "local-demo", "filesystem"),
        (
            "quality-gate",
            "Human approval",
            "control-plane",
            "approval.request",
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (name, role, model, tool))| Agent {
        name: name.into(),
        role: role.into(),
        model: model.into(),
        status: match index {
            0 => Status::Done,
            1 => Status::Running,
            _ => Status::Waiting,
        },
        tokens: if index == 0 {
            1_842
        } else if index == 1 {
            4_276
        } else {
            0
        },
        duration_secs: if index == 0 { 12 } else { 0 },
        tool: tool.into(),
    })
    .collect()
}

impl App {
    fn new() -> Self {
        let config = Config::load("config.toml");
        let agents = build_agents(&config);
        let database_path = config.database_path.clone();
        let workspace_root = Path::new(&config.security.workspace_root);
        let sandbox = FileSandbox::new(workspace_root, config.security.read_only)
            .or_else(|_| {
                FileSandbox::new(
                    Path::new(env!("CARGO_MANIFEST_DIR")).join(workspace_root),
                    config.security.read_only,
                )
            })
            .ok();
        let resolved_workspace_root = sandbox.as_ref().map(|sandbox| sandbox.root().to_path_buf());
        let command_sandbox = resolved_workspace_root.and_then(|root| {
            CommandSandbox::new(
                root,
                config.security.allow_terminal,
                config.security.terminal_allowlist.clone(),
            )
            .ok()
        });
        let workspace_index = config
            .workspaces
            .iter()
            .position(|workspace| workspace.id == config.workspace)
            .unwrap_or(0);
        let mut app = Self {
            config,
            storage: Storage::open(database_path).ok(),
            sandbox,
            command_sandbox,
            pipeline_id: "mc-2048".into(),
            workspace_index,
            agents,
            history: vec![StepRecord {
                step: "Prompt Architect".into(),
                duration_secs: 12,
                tokens: 1_842,
                status: Status::Done,
            }],
            events: vec![
                "14:32:07  pipeline created  #mc-2048".into(),
                "14:32:09  prompt-architect  completed".into(),
                "14:32:10  code-generator  started  [local:qwen2.5-coder:14b]".into(),
            ],
            errors: Vec::new(),
            pipeline: PipelineState::Running,
            selected: 1,
            tab: 0,
            elapsed: 0,
            step_elapsed: 0,
            paused: false,
            help: false,
            task: "Create a backend module in Python".into(),
            task_queue: VecDeque::new(),
            next_task_id: 1,
            diff_output: String::new(),
            input_mode: false,
            feedback_mode: false,
            model_mode: false,
            model_index: 0,
            input_buffer: String::new(),
            theme: Theme::Dark,
            event_cursor: 0,
            llm_receiver: None,
            llm_running: None,
            llm_outputs: HashMap::new(),
            real_pipeline: false,
            real_stage: None,
            real_context: String::new(),
            pressed_keys: HashSet::new(),
            openclaw_commands: None,
            openclaw_updates: None,
            openclaw_connected: false,
            openclaw_input_mode: false,
            openclaw_sessions: Vec::new(),
            active_openclaw_session: None,
            openclaw_batch_queue: VecDeque::new(),
            openclaw_batch_active: None,
            openclaw_batch_total: 0,
            openclaw_batch_success: 0,
            openclaw_batch_errors: 0,
        };
        app.persist_pipeline("RUNNING", "14:32:07");
        app.restore_persisted_summary();
        app.load_external_events();
        app
    }

    fn restore_persisted_summary(&mut self) {
        let Some(storage) = &self.storage else {
            return;
        };
        if let Ok(events) = storage.recent_events(&self.pipeline_id) {
            if !events.is_empty() {
                self.events.push(format!(
                    "14:32:06  storage  restored {} persisted events",
                    events.len()
                ));
            }
        }
        if let Ok(errors) = storage.count_errors(&self.pipeline_id) {
            if errors > 0 {
                self.errors.push(format!(
                    "storage restored {} persisted error records",
                    errors
                ));
            }
        }
    }

    fn load_external_events(&mut self) {
        let path = self.event_source_path();
        if let Ok(batch) = read_new_events(path, &mut self.event_cursor) {
            for error in batch.malformed {
                self.errors.push(format!("Event source: {error}"));
            }
            for event in batch.events {
                self.ingest_event(event);
            }
        }
    }

    fn event_source_path(&self) -> PathBuf {
        let configured = PathBuf::from(&self.config.event_source);
        if configured.exists() {
            configured
        } else {
            Path::new(env!("CARGO_MANIFEST_DIR")).join(configured)
        }
    }

    fn ingest_event(&mut self, event: AgentEvent) {
        if let Some(storage) = &self.storage {
            let _ = storage.record_event(&self.pipeline_id, &event);
            if event.kind == "error" {
                let _ = storage.record_error(&self.pipeline_id, &event);
            }
        }
        let suffix = if event.kind == "tool" {
            format!("  [tool, {}ms]", event.duration_ms)
        } else {
            String::new()
        };
        self.events.push(format!(
            "{}  {}  {}{}",
            event.timestamp, event.agent, event.message, suffix
        ));
        if event.kind == "error" {
            self.errors.push(format!(
                "{}  {}  {}",
                event.timestamp, event.agent, event.message
            ));
        }
        if let Some(agent) = self
            .agents
            .iter_mut()
            .find(|agent| agent.name == event.agent)
        {
            agent.tokens = agent.tokens.saturating_add(event.tokens);
            if event.duration_ms > 0 {
                agent.duration_secs = agent.duration_secs.max(event.duration_ms / 1000);
            }
        }
    }
    fn persist_pipeline(&self, state: &str, timestamp: &str) {
        if let Some(storage) = &self.storage {
            let workspace = self
                .active_workspace()
                .map(|workspace| workspace.id.as_str())
                .unwrap_or(self.config.workspace.as_str());
            let _ = storage.save_pipeline(&self.pipeline_id, workspace, state, timestamp);
        }
    }
    fn selected_agent(&self) -> &Agent {
        &self.agents[self.selected]
    }
    fn active_workspace(&self) -> Option<&config::WorkspaceConfig> {
        self.config.workspaces.get(self.workspace_index)
    }
    fn cycle_workspace(&mut self) {
        if !self.config.workspaces.is_empty() {
            self.workspace_index = (self.workspace_index + 1) % self.config.workspaces.len();
        }
    }
    fn open_model_selector(&mut self) {
        if self.config.models.is_empty() {
            self.errors
                .push("No selectable models are configured in config.toml".into());
            return;
        }
        let selected = self.selected_agent().name.clone();
        if let Some(agent) = self.config.agents.iter().find(|agent| agent.id == selected) {
            self.model_index = self
                .config
                .models
                .iter()
                .position(|profile| {
                    profile.provider == agent.provider && profile.model == agent.model
                })
                .unwrap_or(0);
        }
        self.model_mode = true;
    }

    fn apply_model_selection(&mut self) {
        let Some(profile) = self.config.models.get(self.model_index).cloned() else {
            return;
        };
        let selected = self.selected_agent().name.clone();
        if let Some(agent) = self
            .config
            .agents
            .iter_mut()
            .find(|agent| agent.id == selected)
        {
            agent.provider = profile.provider.clone();
            agent.model = profile.model.clone();
        }
        if let Some(agent) = self.agents.iter_mut().find(|agent| agent.name == selected) {
            agent.model = profile.model.clone();
        }
        self.events.push(format!(
            "Model binding changed  agent={} profile={} provider={} model={}",
            selected, profile.id, profile.provider, profile.model
        ));
        self.model_mode = false;
    }

    fn selected_provider(&self) -> &str {
        let selected = &self.selected_agent().name;
        self.config
            .agents
            .iter()
            .find(|agent| &agent.id == selected)
            .map(|agent| agent.provider.as_str())
            .unwrap_or("not configured")
    }

    fn provider_status(&self, provider_id: &str) -> String {
        let Some(provider) = self
            .config
            .providers
            .iter()
            .find(|provider| provider.id == provider_id)
        else {
            return "NOT CONFIGURED".into();
        };
        match provider.kind.as_str() {
            "demo" => "READY (offline)".into(),
            "ollama" => format!("requires service at {}", provider.base_url),
            "openai" | "anthropic" => {
                if !provider.api_key_env.is_empty()
                    && std::env::var(&provider.api_key_env)
                        .ok()
                        .is_some_and(|value| !value.trim().is_empty())
                {
                    "READY (API key found)".into()
                } else {
                    format!("missing {}", provider.api_key_env)
                }
            }
            other => format!("unsupported provider kind {other}"),
        }
    }
    fn run_selected_agent(&mut self) {
        let prompt = self.task.clone();
        self.launch_agent_job(self.selected, prompt);
    }

    fn launch_agent_job(&mut self, index: usize, prompt: String) {
        if let Some(agent) = &self.llm_running {
            self.errors
                .push(format!("LLM request for {agent} is already running"));
            return;
        }
        let Some(runtime) = self.agents.get(index) else {
            self.errors
                .push(format!("Agent index {index} is out of range"));
            return;
        };
        let selected = runtime.name.clone();
        let Some(agent) = self
            .config
            .agents
            .iter()
            .find(|agent| agent.id == selected)
            .cloned()
        else {
            self.errors
                .push(format!("Agent {selected} is missing from config.toml"));
            return;
        };
        let Some(provider) = self
            .config
            .providers
            .iter()
            .find(|provider| provider.id == agent.provider)
            .cloned()
        else {
            self.errors
                .push(format!("Provider {} is not configured", agent.provider));
            return;
        };
        let request = LlmRequest {
            model: agent.model.clone(),
            system: soul::load(&agent.soul_file).unwrap_or_default(),
            prompt,
        };
        self.events.push(format!(
            "LLM request started  agent={} provider={} model={}",
            agent.id, provider.id, agent.model
        ));
        self.llm_running = Some(agent.id.clone());
        if provider.kind == "demo" {
            let result = llm::generate(&provider, &request);
            self.finish_llm_job(LlmJobResult {
                agent_id: agent.id,
                provider_id: provider.id,
                result,
            });
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let agent_id = agent.id;
        let provider_id = provider.id.clone();
        thread::spawn(move || {
            let result = llm::generate(&provider, &request);
            let _ = sender.send(LlmJobResult {
                agent_id,
                provider_id,
                result,
            });
        });
        self.llm_receiver = Some(receiver);
    }

    fn poll_llm(&mut self) {
        let result = match self.llm_receiver.as_ref().map(Receiver::try_recv) {
            Some(Ok(result)) => Some(result),
            Some(Err(TryRecvError::Disconnected)) => {
                self.errors
                    .push("LLM worker disconnected unexpectedly".into());
                self.llm_running = None;
                self.llm_receiver = None;
                None
            }
            _ => None,
        };
        if let Some(result) = result {
            self.llm_receiver = None;
            self.finish_llm_job(result);
        }
    }

    fn finish_llm_job(&mut self, job: LlmJobResult) {
        self.llm_running = None;
        match job.result {
            Ok(response) => {
                let tokens = response.input_tokens.saturating_add(response.output_tokens);
                self.llm_outputs
                    .insert(job.agent_id.clone(), response.text.clone());
                if let Some(runtime) = self
                    .agents
                    .iter_mut()
                    .find(|runtime| runtime.name == job.agent_id)
                {
                    runtime.tokens = runtime.tokens.saturating_add(tokens);
                }
                let summary = if response.text.chars().count() > 80 {
                    format!("{}…", response.text.chars().take(79).collect::<String>())
                } else {
                    response.text.clone()
                };
                self.events.push(format!(
                    "LLM response  agent={} tokens={} text={}",
                    job.agent_id, tokens, summary
                ));
                if self.real_pipeline {
                    self.advance_real_pipeline(&job.agent_id, response.text);
                }
            }
            Err(error) => {
                self.llm_outputs
                    .insert(job.agent_id.clone(), format!("ERROR: {error}"));
                self.errors.push(format!("LLM {}: {}", job.agent_id, error));
                self.events.push(format!(
                    "LLM error  agent={} provider={}",
                    job.agent_id, job.provider_id
                ));
                if self.real_pipeline {
                    if let Some(agent) = self
                        .agents
                        .iter_mut()
                        .find(|agent| agent.name == job.agent_id)
                    {
                        agent.status = Status::Error;
                    }
                    self.real_pipeline = false;
                    self.real_stage = None;
                }
            }
        }
    }

    fn begin_real_pipeline(&mut self) {
        self.real_pipeline = true;
        self.real_stage = Some(0);
        self.real_context.clear();
        for agent in &mut self.agents {
            agent.status = Status::Waiting;
            agent.tokens = 0;
            agent.duration_secs = 0;
        }
        if let Some(agent) = self.agents.get_mut(0) {
            agent.status = Status::Running;
            agent.tool = "llm.plan".into();
        }
        self.pipeline = PipelineState::Running;
        self.step_elapsed = 0;
        self.events.push(format!(
            "Real pipeline started  task={}  stage=Architect",
            self.task
        ));
        self.launch_agent_job(
            0,
            format!(
                "User task:\n{}\n\nCreate a precise implementation specification for the next coding agent. Include required files, interfaces, behavior, tests, and acceptance criteria. Do not write code yet.",
                self.task
            ),
        );
    }

    fn advance_real_pipeline(&mut self, agent_id: &str, response: String) {
        let Some(stage) = self.real_stage else {
            return;
        };
        let expected = self
            .agents
            .get(stage)
            .map(|agent| agent.name.as_str())
            .unwrap_or_default();
        if expected != agent_id {
            return;
        }
        if let Some(agent) = self.agents.get_mut(stage) {
            agent.status = Status::Done;
            agent.tool = "completed".into();
        }
        self.history.push(StepRecord {
            step: self
                .agents
                .get(stage)
                .map(|agent| agent.role.clone())
                .unwrap_or_else(|| format!("Stage {stage}")),
            duration_secs: self.step_elapsed,
            tokens: self
                .agents
                .get(stage)
                .map(|agent| agent.tokens)
                .unwrap_or(0),
            status: Status::Done,
        });
        self.step_elapsed = 0;
        match stage {
            0 => {
                self.real_stage = Some(1);
                self.real_context = response.clone();
                if let Some(agent) = self.agents.get_mut(1) {
                    agent.status = Status::Running;
                    agent.tool = "filesystem.write".into();
                }
                self.events
                    .push("Architect specification ready; starting Code Generator".into());
                self.launch_agent_job(
                    1,
                    format!(
                        "Implement this task in the workspace. Return every created or changed file using exactly <file path=\"relative/path\">file contents</file>. Also include a short summary after the file blocks.\n\nOriginal task:\n{}\n\nArchitect specification:\n{}",
                        self.task, response
                    ),
                );
            }
            1 => {
                let files = llm::extract_files(&response);
                let mut artifact_summary = Vec::new();
                let mut diff = String::new();
                if files.is_empty() {
                    match self.sandbox.as_ref() {
                        Some(sandbox) => {
                            match sandbox.write_text("generated-response.md", &response) {
                                Ok(()) => {
                                    diff.push_str(&file_diff(
                                        "generated-response.md",
                                        None,
                                        &response,
                                    ));
                                    artifact_summary.push("generated-response.md".into())
                                }
                                Err(error) => {
                                    self.errors.push(format!("MCP filesystem.write: {error}"))
                                }
                            }
                        }
                        None => self.errors.push("Filesystem sandbox is unavailable".into()),
                    }
                } else if let Some(sandbox) = &self.sandbox {
                    for (path, content) in files {
                        let previous = sandbox.read_text(&path).ok();
                        match sandbox.write_text(&path, &content) {
                            Ok(()) => {
                                diff.push_str(&file_diff(&path, previous.as_deref(), &content));
                                artifact_summary.push(path)
                            }
                            Err(error) => self
                                .errors
                                .push(format!("MCP filesystem.write {path}: {error}")),
                        }
                    }
                }
                self.diff_output = if diff.is_empty() {
                    "No file changes were produced.".into()
                } else {
                    diff
                };
                self.run_automatic_validation();
                self.real_stage = Some(2);
                self.real_context = format!(
                    "Generated files: {}\n\n{}",
                    artifact_summary.join(", "),
                    response
                );
                if let Some(agent) = self.agents.get_mut(2) {
                    agent.status = Status::Running;
                    agent.tool = "filesystem.read".into();
                }
                self.events.push(format!(
                    "Code Generator wrote {} artifact(s); starting Reviewer",
                    artifact_summary.len()
                ));
                self.launch_agent_job(
                    2,
                    format!(
                        "Review the implementation against the task and specification. Report concrete errors, missing tests, and an APPROVE or REJECT recommendation.\n\nTask:\n{}\n\nSpecification and generated output:\n{}",
                        self.task, self.real_context
                    ),
                );
            }
            2 => {
                // Keep the real-run context alive while waiting for the human
                // decision so Reject can send feedback back to the Generator.
                self.real_pipeline = true;
                self.real_stage = None;
                if let Some(agent) = self.agents.get_mut(3) {
                    agent.status = Status::Running;
                    agent.tool = "approval.request".into();
                }
                self.pipeline = PipelineState::AwaitingApproval;
                self.real_context = response;
                self.persist_pipeline("AWAITING_APPROVAL", "now");
                self.events
                    .push("Reviewer report ready; waiting for Human Approval".into());
            }
            _ => {}
        }
    }
    fn inspect_workspace(&mut self) {
        let Some(sandbox) = &self.sandbox else {
            self.errors.push("Filesystem sandbox is unavailable".into());
            return;
        };
        match sandbox.list_workspace() {
            Ok(entries) => self
                .events
                .push(format!("MCP filesystem.list  {}", entries.join(", "))),
            Err(error) => self.errors.push(format!("MCP filesystem.list: {error}")),
        }
    }

    fn run_automatic_validation(&mut self) {
        let command = self.config.security.validation_command.clone();
        if command.is_empty() {
            return;
        }
        if !self.config.security.allow_terminal {
            self.events.push(
                "Validation skipped: terminal sandbox is disabled (set allow_terminal = true)"
                    .into(),
            );
            return;
        }
        let Some(sandbox) = self.command_sandbox.clone() else {
            self.errors
                .push("Validation skipped: terminal sandbox is unavailable".into());
            return;
        };
        let program = command[0].clone();
        let args = command[1..].iter().map(String::as_str).collect::<Vec<_>>();
        match sandbox.run(&program, &args) {
            Ok(result) => {
                self.events.push(format!(
                    "MCP validation  {} {}  exit={} time={}ms",
                    program,
                    args.join(" "),
                    result.status,
                    result.duration.as_millis()
                ));
                if result.status == 0 {
                    self.events.push("Automatic tests passed".into());
                } else {
                    self.errors.push(format!(
                        "Automatic tests failed (exit {}): {}",
                        result.status,
                        if result.stderr.is_empty() {
                            result.stdout
                        } else {
                            result.stderr
                        }
                    ));
                }
            }
            Err(error) => self
                .errors
                .push(format!("MCP validation {}: {error}", program)),
        }
    }
    fn inspect_git(&mut self) {
        let Some(sandbox) = &self.command_sandbox else {
            self.errors.push("Terminal sandbox is unavailable".into());
            return;
        };
        match sandbox.run("git", &["status", "--short"]) {
            Ok(result) => {
                let output = if result.stdout.is_empty() {
                    "working tree clean".to_string()
                } else {
                    result
                        .stdout
                        .lines()
                        .take(5)
                        .collect::<Vec<_>>()
                        .join(" | ")
                };
                self.events.push(format!(
                    "MCP terminal.exec  git status --short  exit={} time={}ms  {}",
                    result.status,
                    result.duration.as_millis(),
                    output
                ));
                if result.status != 0 {
                    self.errors.push(format!("Git status: {}", result.stderr));
                }
            }
            Err(error) => self.errors.push(format!("MCP terminal.exec: {error}")),
        }
    }
    fn inspect_openclaw(&mut self) {
        if !self.config.openclaw.enabled {
            self.errors.push(
                "OpenClaw integration is disabled; set openclaw.enabled = true in config.toml"
                    .into(),
            );
            return;
        }
        if self.openclaw_connected {
            self.input_mode = true;
            self.feedback_mode = false;
            self.openclaw_input_mode = true;
            self.input_buffer.clear();
            return;
        }
        if self.openclaw_updates.is_some() {
            self.events
                .push("OpenClaw connection is already starting".into());
            return;
        }
        let token = match std::env::var(&self.config.openclaw.token_env) {
            Ok(token) if !token.trim().is_empty() => token,
            _ => {
                self.errors.push(format!(
                    "OpenClaw needs {}. Run 'openclaw gateway auth-token --show' in a private terminal and export the value without sharing it.",
                    self.config.openclaw.token_env
                ));
                return;
            }
        };
        let handle = openclaw::spawn(openclaw::GatewayConfig {
            url: self.config.openclaw.url.clone(),
            token,
            agent_id: self.config.openclaw.agent_id.clone(),
        });
        self.openclaw_commands = Some(handle.commands);
        self.openclaw_updates = Some(handle.updates);
        self.events.push(format!(
            "OpenClaw WebSocket connecting  url={}  protocol=v4",
            self.config.openclaw.url
        ));
    }

    fn submit_openclaw_task(&mut self, task: String) {
        let Some(commands) = &self.openclaw_commands else {
            self.errors
                .push("OpenClaw is not connected; press o first".into());
            return;
        };
        match commands.send(GatewayCommand::CreateTask { task: task.clone() }) {
            Ok(()) => {
                self.task = task.clone();
                self.events
                    .push(format!("OpenClaw task submitted  task={task}"));
            }
            Err(error) => self
                .errors
                .push(format!("OpenClaw command channel: {error}")),
        }
    }

    fn load_openclaw_batch(&mut self) {
        if self.openclaw_batch_active.is_some() || !self.openclaw_batch_queue.is_empty() {
            self.errors
                .push("An OpenClaw batch is already running".into());
            return;
        }
        let input_path = project_path(&self.config.openclaw.task_input_file);
        let tasks = match task_batch::load_tasks(&input_path) {
            Ok(tasks) if !tasks.is_empty() => tasks,
            Ok(_) => {
                self.errors.push(format!(
                    "OpenClaw batch file {} contains no tasks",
                    input_path.display()
                ));
                return;
            }
            Err(error) => {
                self.errors.push(format!(
                    "Cannot read OpenClaw batch file {}: {error}",
                    input_path.display()
                ));
                return;
            }
        };
        let report_path = project_path(&self.config.openclaw.task_report_file);
        if let Err(error) = task_batch::begin_report(&report_path, tasks.len()) {
            self.errors.push(format!(
                "Cannot initialize batch report {}: {error}",
                report_path.display()
            ));
            return;
        }

        self.openclaw_batch_total = tasks.len();
        self.openclaw_batch_success = 0;
        self.openclaw_batch_errors = 0;
        for text in tasks {
            let id = self.next_task_id;
            self.next_task_id = self.next_task_id.saturating_add(1);
            self.openclaw_batch_queue.push_back(QueuedTask { id, text });
        }
        self.events.push(format!(
            "OpenClaw batch loaded  tasks={}  input={}  report={}",
            self.openclaw_batch_total,
            input_path.display(),
            report_path.display()
        ));
        if self.openclaw_connected {
            self.start_next_openclaw_batch_task();
        } else {
            self.events
                .push("OpenClaw batch is waiting for Gateway connection".into());
            self.inspect_openclaw();
        }
    }

    fn start_next_openclaw_batch_task(&mut self) {
        if !self.openclaw_connected || self.openclaw_batch_active.is_some() {
            return;
        }
        let Some(task) = self.openclaw_batch_queue.pop_front() else {
            return;
        };
        let Some(commands) = &self.openclaw_commands else {
            self.openclaw_batch_queue.push_front(task);
            return;
        };
        self.active_openclaw_session = None;
        match commands.send(GatewayCommand::CreateTask {
            task: task.text.clone(),
        }) {
            Ok(()) => {
                self.task = task.text.clone();
                self.events.push(format!(
                    "OpenClaw batch task started  #{}  task={}",
                    task.id, task.text
                ));
                self.openclaw_batch_active = Some(task);
            }
            Err(error) => {
                self.openclaw_batch_queue.push_front(task);
                self.errors
                    .push(format!("OpenClaw batch command channel: {error}"));
            }
        }
    }

    fn finish_openclaw_batch_task(&mut self, outcome: TaskOutcome, detail: &str) {
        let Some(task) = self.openclaw_batch_active.take() else {
            return;
        };
        let session = self.active_openclaw_session.take();
        match outcome {
            TaskOutcome::Success => self.openclaw_batch_success += 1,
            TaskOutcome::Error => self.openclaw_batch_errors += 1,
        }
        let report_path = project_path(&self.config.openclaw.task_report_file);
        if let Err(error) = task_batch::append_result(
            &report_path,
            task.id,
            &task.text,
            outcome,
            session.as_deref(),
            detail,
        ) {
            self.errors
                .push(format!("Cannot append OpenClaw batch report: {error}"));
        }
        let status = match outcome {
            TaskOutcome::Success => "SUCCESS",
            TaskOutcome::Error => "ERROR",
        };
        self.events.push(format!(
            "OpenClaw batch task finished  #{}  status={}  detail={}",
            task.id, status, detail
        ));

        if self.openclaw_batch_queue.is_empty() {
            if let Err(error) = task_batch::finish_report(
                &report_path,
                self.openclaw_batch_total,
                self.openclaw_batch_success,
                self.openclaw_batch_errors,
            ) {
                self.errors
                    .push(format!("Cannot finalize OpenClaw batch report: {error}"));
            }
            self.events.push(format!(
                "OpenClaw batch complete  success={}  errors={}  report={}",
                self.openclaw_batch_success,
                self.openclaw_batch_errors,
                report_path.display()
            ));
        } else {
            self.start_next_openclaw_batch_task();
        }
    }

    fn abort_openclaw_session(&mut self) {
        let Some(session_key) = self.active_openclaw_session.clone() else {
            self.errors
                .push("No active OpenClaw session is selected".into());
            return;
        };
        let Some(commands) = &self.openclaw_commands else {
            self.errors.push("OpenClaw is not connected".into());
            return;
        };
        match commands.send(GatewayCommand::AbortSession {
            session_key: session_key.clone(),
        }) {
            Ok(()) => self
                .events
                .push(format!("OpenClaw abort requested  session={session_key}")),
            Err(error) => self.errors.push(format!("OpenClaw abort channel: {error}")),
        }
    }

    fn poll_openclaw(&mut self) {
        let mut updates = Vec::new();
        if let Some(receiver) = &self.openclaw_updates {
            loop {
                match receiver.try_recv() {
                    Ok(update) => updates.push(update),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        updates.push(GatewayUpdate::Disconnected);
                        break;
                    }
                }
            }
        }
        for update in updates {
            match update {
                GatewayUpdate::Connected => {
                    self.openclaw_connected = true;
                    self.events
                        .push("OpenClaw connected with operator.read/operator.write scopes".into());
                    self.start_next_openclaw_batch_task();
                }
                GatewayUpdate::Sessions(payload) => {
                    self.openclaw_sessions = extract_openclaw_sessions(&payload);
                    self.events.push(format!(
                        "OpenClaw session roster loaded  sessions={}",
                        self.openclaw_sessions.len()
                    ));
                }
                GatewayUpdate::Event { name, payload } => {
                    let compact = compact_json(&payload, 150);
                    self.events
                        .push(format!("OpenClaw event  {name}  {compact}"));
                    if let Some((outcome, detail)) = openclaw_event_outcome(&name, &payload) {
                        let belongs_to_active = payload
                            .get("sessionKey")
                            .and_then(serde_json::Value::as_str)
                            .zip(self.active_openclaw_session.as_deref())
                            .is_some_and(|(event, active)| event == active);
                        if belongs_to_active {
                            self.finish_openclaw_batch_task(outcome, &detail);
                        }
                    }
                }
                GatewayUpdate::Response { method, payload } => {
                    if method.starts_with("create-") {
                        if let Some(key) = payload
                            .get("sessionKey")
                            .or_else(|| payload.get("key"))
                            .and_then(serde_json::Value::as_str)
                        {
                            self.active_openclaw_session = Some(key.to_string());
                            self.openclaw_sessions.push(key.to_string());
                            self.events.push(format!(
                                "OpenClaw session created  key={key}  streaming before task dispatch"
                            ));
                        }
                    } else {
                        self.events.push(format!(
                            "OpenClaw response  {method}  {}",
                            compact_json(&payload, 150)
                        ));
                    }
                }
                GatewayUpdate::Error(error) => {
                    self.events.push(format!("OpenClaw ERROR  {error}"));
                    self.errors.push(format!("OpenClaw WebSocket: {error}"));
                    if error.starts_with("RPC create-")
                        || error.starts_with("RPC observe-")
                        || error.starts_with("RPC send-")
                    {
                        self.finish_openclaw_batch_task(TaskOutcome::Error, &error);
                    }
                }
                GatewayUpdate::Disconnected => {
                    if self.openclaw_connected {
                        self.events.push("OpenClaw WebSocket disconnected".into());
                    }
                    self.openclaw_connected = false;
                    self.openclaw_commands = None;
                    self.openclaw_updates = None;
                    if self.openclaw_batch_active.is_some() {
                        self.finish_openclaw_batch_task(
                            TaskOutcome::Error,
                            "Gateway disconnected before task completion",
                        );
                    }
                }
            }
        }
    }
    fn tick(&mut self) {
        if !self.paused {
            self.load_external_events();
            self.elapsed += 1;
            self.step_elapsed += 1;
            if self.real_pipeline {
                return;
            }
            if let Some(agent) = self
                .agents
                .iter_mut()
                .find(|agent| agent.status == Status::Running)
            {
                agent.tokens += 7;
                agent.duration_secs += 1;
            }
            if self.elapsed % 2 == 0 {
                if let Some(agent) = self
                    .agents
                    .iter()
                    .find(|agent| agent.status == Status::Running)
                {
                    self.events.push(format!(
                        "14:{:02}:{:02}  {}  heartbeat  tool={} tokens=+7",
                        32 + (self.elapsed / 60),
                        self.elapsed % 60,
                        agent.name,
                        agent.tool
                    ));
                    if self.events.len() > 40 {
                        self.events.remove(0);
                    }
                }
            }
            self.advance_pipeline();
        }
    }

    fn advance_pipeline(&mut self) {
        if self.pipeline != PipelineState::Running {
            return;
        }
        if self.agents[1].status == Status::Running && self.step_elapsed >= 15 {
            self.agents[1].status = Status::Done;
            self.history.push(StepRecord {
                step: "Code Generator".into(),
                duration_secs: self.agents[1].duration_secs,
                tokens: self.agents[1].tokens,
                status: Status::Done,
            });
            self.events
                .push("14:34:10  code-generator  completed  [filesystem + terminal]".into());
            self.agents[2].status = Status::Running;
            self.agents[2].tool = "filesystem.read".into();
            self.step_elapsed = 0;
            self.events
                .push("14:34:10  code-reviewer  started  [cloud:gpt-4o]".into());
        } else if self.agents[2].status == Status::Running && self.step_elapsed >= 8 {
            self.agents[2].status = Status::Done;
            self.agents[2].duration_secs = self.step_elapsed;
            self.agents[2].tokens = 1_126;
            self.history.push(StepRecord {
                step: "Code Reviewer".into(),
                duration_secs: self.agents[2].duration_secs,
                tokens: self.agents[2].tokens,
                status: Status::Done,
            });
            self.events
                .push("14:34:18  code-reviewer  report ready  [0 errors]".into());
            self.agents[3].status = Status::Running;
            self.agents[3].tool = "approval.request".into();
            self.pipeline = PipelineState::AwaitingApproval;
            self.persist_pipeline("AWAITING_APPROVAL", "14:34:18");
            for error in notifications::send(
                &self.config.notifications,
                &format!(
                    "Mission Control task '{}' is waiting for approval",
                    self.task
                ),
            ) {
                self.errors.push(format!("Notification: {error}"));
            }
            self.events
                .push("14:34:18  quality-gate  awaiting human approval".into());
        }
    }

    fn approve(&mut self) {
        if self.pipeline == PipelineState::AwaitingApproval {
            self.real_pipeline = false;
            self.real_stage = None;
            self.agents[3].status = Status::Done;
            self.history.push(StepRecord {
                step: "Human Approval".into(),
                duration_secs: self.step_elapsed,
                tokens: 0,
                status: Status::Done,
            });
            self.events
                .push("14:34:22  human approval  APPROVED  [artifacts committed]".into());
            self.pipeline = PipelineState::Completed;
            self.persist_pipeline("COMPLETED", "14:34:22");
            self.start_next_queued_task();
        }
    }

    fn reject_with_feedback(&mut self, feedback: &str) {
        if self.pipeline == PipelineState::AwaitingApproval {
            let feedback = feedback.trim();
            let feedback = if feedback.is_empty() {
                "revise implementation"
            } else {
                feedback
            };
            self.errors
                .push(format!("Human review rejected the artifact: {feedback}"));
            self.events.push(format!(
                "14:34:22  human approval  REJECTED  [feedback: {feedback}]"
            ));
            self.agents[1].status = Status::Running;
            self.agents[1].tool = "filesystem.write".into();
            self.agents[2].status = Status::Waiting;
            self.agents[3].status = Status::Waiting;
            self.step_elapsed = 0;
            self.pipeline = PipelineState::Running;
            self.persist_pipeline("RUNNING", "14:34:22");
            if self.real_pipeline {
                self.real_stage = Some(1);
                self.agents[1].status = Status::Running;
                self.agents[1].tool = "filesystem.write".into();
                self.agents[2].status = Status::Waiting;
                self.agents[3].status = Status::Waiting;
                self.launch_agent_job(
                    1,
                    format!(
                        "Revise the implementation according to this human feedback. Return every changed file using exactly <file path=\"relative/path\">file contents</file>.\n\nTask: {}\n\nFeedback: {}\n\nPrevious review and generated output:\n{}",
                        self.task, feedback, self.real_context
                    ),
                );
            }
        }
    }

    fn restart(&mut self) {
        let task = self.task.clone();
        self.start_task(task);
    }

    fn enqueue_task(&mut self, task: String) {
        let id = self.next_task_id;
        self.next_task_id = self.next_task_id.saturating_add(1);
        self.task_queue.push_back(QueuedTask {
            id,
            text: task.clone(),
        });
        self.events.push(format!("Task queued  #{id}  task={task}"));
    }

    fn start_next_queued_task(&mut self) {
        if let Some(next) = self.task_queue.pop_front() {
            let task_id = next.id;
            self.start_task(next.text);
            self.events
                .push(format!("Starting queued task  #{task_id}"));
        }
    }

    fn start_task(&mut self, task: String) {
        if self.real_pipeline {
            self.enqueue_task(task);
            return;
        }
        let bindings = self
            .config
            .agents
            .iter()
            .map(|agent| {
                (
                    agent.id.clone(),
                    agent.provider.clone(),
                    agent.model.clone(),
                )
            })
            .collect::<Vec<_>>();
        let queued_tasks = std::mem::take(&mut self.task_queue);
        let next_task_id = self.next_task_id;
        *self = Self::new();
        self.task_queue = queued_tasks;
        self.next_task_id = next_task_id;
        for (id, provider, model) in bindings {
            if let Some(agent) = self.config.agents.iter_mut().find(|agent| agent.id == id) {
                agent.provider = provider;
                agent.model = model.clone();
            }
            if let Some(agent) = self.agents.iter_mut().find(|agent| agent.name == id) {
                agent.model = model;
            }
        }
        self.task = task;
        self.events
            .push(format!("New task accepted  task={}", self.task));
        self.begin_real_pipeline();
    }
    fn select_agent(&mut self, index: usize) {
        if index < self.agents.len() {
            self.selected = index;
        }
    }
    fn handle_mouse(&mut self, mouse: MouseEvent, area: Rect) -> bool {
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return false;
        }
        if self.model_mode {
            let first_model_row = area.height / 5 + 4;
            if mouse.row >= first_model_row {
                let index = usize::from(mouse.row - first_model_row);
                if index < self.config.models.len() {
                    self.model_index = index;
                    self.apply_model_selection();
                }
            }
            return false;
        }
        if self.input_mode || self.help {
            return false;
        }
        if mouse.row >= area.height.saturating_sub(2) && area.width > 0 {
            let row = usize::from(mouse.row.saturating_sub(area.height.saturating_sub(2))).min(1);
            let column = (usize::from(mouse.column) * 8 / usize::from(area.width)).min(7);
            let code = [
                [
                    KeyCode::Char('q'),
                    KeyCode::Char('t'),
                    KeyCode::Char('b'),
                    KeyCode::Char('g'),
                    KeyCode::Char('m'),
                    KeyCode::Char('f'),
                    KeyCode::Char('o'),
                    KeyCode::Char('c'),
                ],
                [
                    KeyCode::Char('w'),
                    KeyCode::Char('a'),
                    KeyCode::Char('r'),
                    KeyCode::Char('n'),
                    KeyCode::Char(' '),
                    KeyCode::Char('?'),
                    KeyCode::Char('x'),
                    KeyCode::Null,
                ],
            ][row][column];
            return self.handle_key(KeyEvent::new(code, crossterm::event::KeyModifiers::NONE));
        }
        let body_width = area.width;
        let agent_panel_width = body_width.saturating_mul(38) / 100;
        if mouse.column < agent_panel_width && (5..13).contains(&mouse.row) {
            self.select_agent(((mouse.row - 5) / 2) as usize);
            return false;
        }
        if mouse.column >= agent_panel_width && (3..6).contains(&mouse.row) {
            let detail_width = body_width.saturating_sub(agent_panel_width).max(1);
            let relative = mouse.column.saturating_sub(agent_panel_width);
            self.tab = usize::from(relative)
                .saturating_mul(6)
                .checked_div(usize::from(detail_width))
                .unwrap_or(0)
                .min(6);
        }
        false
    }
    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL)
            && key.code == KeyCode::Char('c')
        {
            return true;
        }
        if self.model_mode {
            match key.code {
                KeyCode::Esc => self.model_mode = false,
                KeyCode::Enter => self.apply_model_selection(),
                KeyCode::Down | KeyCode::Char('j') => {
                    self.model_index = (self.model_index + 1) % self.config.models.len();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.model_index = self
                        .model_index
                        .checked_sub(1)
                        .unwrap_or(self.config.models.len() - 1);
                }
                _ => {}
            }
            return false;
        }
        if self.input_mode {
            match key.code {
                KeyCode::Esc => {
                    self.input_mode = false;
                    self.feedback_mode = false;
                    self.openclaw_input_mode = false;
                    self.input_buffer.clear();
                }
                KeyCode::Enter => {
                    if !self.input_buffer.trim().is_empty() {
                        let value = self.input_buffer.trim().to_string();
                        if self.feedback_mode {
                            self.reject_with_feedback(&value);
                        } else if self.openclaw_input_mode {
                            self.submit_openclaw_task(value);
                        } else {
                            self.start_task(value);
                        }
                        self.input_buffer.clear();
                    }
                    self.input_mode = false;
                    self.feedback_mode = false;
                    self.openclaw_input_mode = false;
                }
                KeyCode::Backspace => {
                    self.input_buffer.pop();
                }
                KeyCode::Char(character) => self.input_buffer.push(character),
                _ => {}
            }
            return false;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return true,
            KeyCode::Char(' ') => self.paused = !self.paused,
            KeyCode::Char('?') | KeyCode::F(1) => self.help = !self.help,
            KeyCode::Char('a') => self.approve(),
            KeyCode::Char('r') => {
                if self.pipeline == PipelineState::AwaitingApproval {
                    self.input_mode = true;
                    self.feedback_mode = true;
                    self.input_buffer.clear();
                }
            }
            KeyCode::Char('n') => self.restart(),
            KeyCode::Char('t') => {
                self.input_mode = true;
                self.feedback_mode = false;
                self.openclaw_input_mode = false;
                self.input_buffer.clear();
            }
            KeyCode::Char('g') => self.run_selected_agent(),
            KeyCode::Char('m') => self.open_model_selector(),
            KeyCode::Char('c') => self.theme = self.theme.next(),
            KeyCode::Char('f') => self.inspect_workspace(),
            KeyCode::Char('v') => self.inspect_git(),
            KeyCode::Char('x') => self.abort_openclaw_session(),
            KeyCode::Char('o') => self.inspect_openclaw(),
            KeyCode::Char('b') => self.load_openclaw_batch(),
            KeyCode::Char('w') => self.cycle_workspace(),
            KeyCode::Char('l') => self.tab = 6,
            KeyCode::Char('1') => self.select_agent(0),
            KeyCode::Char('2') => self.select_agent(1),
            KeyCode::Char('3') => self.select_agent(2),
            KeyCode::Char('4') => self.select_agent(3),
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1) % self.agents.len()
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self
                    .selected
                    .checked_sub(1)
                    .unwrap_or(self.agents.len() - 1)
            }
            KeyCode::Right | KeyCode::Tab => self.tab = (self.tab + 1) % 7,
            KeyCode::Left => self.tab = self.tab.checked_sub(1).unwrap_or(6),
            _ => {}
        }
        false
    }

    /// Accept terminals that report only key releases, while preventing a
    /// normal press+release pair from executing an action twice. Each pressed
    /// key is tracked independently because release events can arrive after a
    /// different key has already been pressed.
    fn handle_key_event(&mut self, key: KeyEvent) -> bool {
        match key.kind {
            KeyEventKind::Press => {
                self.pressed_keys.insert(key.code.clone());
                self.handle_key(key)
            }
            KeyEventKind::Repeat => self.handle_key(key),
            KeyEventKind::Release => {
                if self.pressed_keys.remove(&key.code) {
                    false
                } else {
                    self.handle_key(key)
                }
            }
        }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run(&mut terminal);
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    result
}
fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new();
    let mut last_tick = Instant::now();
    loop {
        app.poll_llm();
        app.poll_openclaw();
        terminal.draw(|frame| draw(frame, &app))?;
        let timeout = input_poll_timeout(last_tick.elapsed());
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    if app.handle_key_event(key) {
                        break;
                    }
                }
                Event::Mouse(mouse) => {
                    let size = terminal.size()?;
                    if app.handle_mouse(mouse, Rect::new(0, 0, size.width, size.height)) {
                        break;
                    }
                }
                _ => {}
            }
        }
        if last_tick.elapsed() >= Duration::from_secs(1) {
            app.tick();
            last_tick = Instant::now();
        }
    }
    Ok(())
}

fn input_poll_timeout(elapsed_since_tick: Duration) -> Duration {
    Duration::from_secs(1).saturating_sub(elapsed_since_tick)
}
fn draw(frame: &mut ratatui::Frame, app: &App) {
    frame.render_widget(
        Block::default().style(
            Style::default()
                .bg(app.theme.background())
                .fg(app.theme.foreground()),
        ),
        frame.area(),
    );
    let workspace_name = app
        .active_workspace()
        .map(|workspace| workspace.name.as_str())
        .unwrap_or(app.config.workspace.as_str());
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(frame.area());
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " MISSION CONTROL ",
            Style::default()
                .fg(Color::Black)
                .bg(app.theme.accent())
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "  AI Agent Observatory  •  workspace: {}  •  agents: {}  •  theme: {}  •  pipeline: #mc-2048  •  {}",
            workspace_name,
            app.config.agents.len(),
            app.theme.label(),
            pipeline_label(app.pipeline)
        )),
    ]))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(header, root[0]);
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(root[1]);
    draw_agents(frame, app, body[0]);
    draw_detail(frame, app, body[1]);
    draw_footer(frame, app, root[2]);
    if app.input_mode {
        draw_task_input(frame, app);
    } else if app.model_mode {
        draw_model_selector(frame, app);
    } else if app.help {
        draw_help(frame);
    }
}

fn draw_model_selector(frame: &mut ratatui::Frame, app: &App) {
    let area = frame.area();
    let popup = Rect {
        x: area.width / 8,
        y: area.height / 5,
        width: area.width * 3 / 4,
        height: (app.config.models.len() as u16 + 8).min(area.height * 3 / 5),
    };
    frame.render_widget(Clear, popup);
    let selected_agent = app.selected_agent();
    let mut lines = vec![
        Line::from(Span::styled(
            format!("SELECT MODEL FOR {}", selected_agent.name),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from("Use ↑/↓ or j/k, Enter to apply, Esc to cancel."),
        Line::from(""),
    ];
    for (index, profile) in app.config.models.iter().enumerate() {
        let marker = if index == app.model_index { "▶" } else { " " };
        let style = if index == app.model_index {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::from(Span::styled(
            format!(
                "{} {}  |  {}/{}",
                marker, profile.name, profile.provider, profile.model
            ),
            style,
        )));
    }
    if let Some(profile) = app.config.models.get(app.model_index) {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("STATUS  ", Style::default().fg(Color::Gray)),
            Span::styled(
                app.provider_status(&profile.provider),
                Style::default().fg(if profile.provider == "local" {
                    Color::Green
                } else {
                    Color::Yellow
                }),
            ),
        ]));
    }
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title(" MODEL / PROVIDER ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .style(Style::default().bg(Color::Rgb(12, 20, 30)))
                .padding(ratatui::widgets::Padding::horizontal(2)),
        ),
        popup,
    );
}

fn draw_footer(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let pause = if app.paused {
        "[Space] Resume"
    } else {
        "[Space] Pause"
    };
    let rows = [
        Row::new([
            "[q] Quit",
            "[t] New task",
            "[b] Batch TXT",
            "[g] Run LLM",
            "[m] Model",
            "[f] Files",
            "[o] OpenClaw",
            "[c] Theme",
        ]),
        Row::new([
            "[w] Workspace",
            "[a] Approve",
            "[r] Reject",
            "[n] Restart",
            pause,
            "[?] Help",
            "[x] Stop OC",
            "",
        ]),
    ];
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Ratio(1, 8),
                Constraint::Ratio(1, 8),
                Constraint::Ratio(1, 8),
                Constraint::Ratio(1, 8),
                Constraint::Ratio(1, 8),
                Constraint::Ratio(1, 8),
                Constraint::Ratio(1, 8),
                Constraint::Ratio(1, 8),
            ],
        )
        .style(Style::default().fg(Color::Black).bg(app.theme.accent()))
        .column_spacing(0),
        area,
    );
}

fn draw_task_input(frame: &mut ratatui::Frame, app: &App) {
    let area = frame.area();
    let popup = Rect {
        x: area.width / 10,
        y: area.height / 3,
        width: area.width * 8 / 10,
        height: 7,
    };
    frame.render_widget(Clear, popup);
    let heading = if app.feedback_mode {
        "REJECT WITH FEEDBACK"
    } else if app.openclaw_input_mode {
        "NEW OPENCLAW TASK"
    } else {
        "NEW PIPELINE TASK"
    };
    let explanation = if app.feedback_mode {
        "Explain what the generator must change, then press Enter."
    } else if app.openclaw_input_mode {
        "Create a real OpenClaw session and stream its agent/tool events."
    } else {
        "Describe what you want the agents to do, then press Enter."
    };
    let title = if app.feedback_mode {
        " REVIEW FEEDBACK "
    } else {
        " TASK INPUT "
    };
    let text = vec![
        Line::from(Span::styled(
            heading,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(explanation),
        Line::from(""),
        Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Green)),
            Span::raw(&app.input_buffer),
            Span::styled("▌", Style::default().fg(Color::Cyan)),
        ]),
        Line::from("Esc cancels  •  Enter submits the task"),
    ];
    frame.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: true }).block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .style(Style::default().bg(Color::Rgb(12, 20, 30)))
                .padding(ratatui::widgets::Padding::horizontal(2)),
        ),
        popup,
    );
}

fn draw_help(frame: &mut ratatui::Frame) {
    let area = frame.area();
    let popup = Rect {
        x: area.width / 10,
        y: area.height / 10,
        width: area.width * 8 / 10,
        height: area.height * 8 / 10,
    };
    frame.render_widget(Clear, popup);
    let text = vec![
        Line::from(Span::styled(
            "MISSION CONTROL HELP",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("Mission Control sleduje práci AI agentů v pipeline."),
        Line::from("Každý agent má vlastní roli, model a povolené nástroje."),
        Line::from(""),
        Line::from(vec![
            Span::styled("1–4  ", Style::default().fg(Color::Yellow)),
            Span::raw("vybrat konkrétního agenta"),
        ]),
        Line::from(vec![
            Span::styled("j/k  ", Style::default().fg(Color::Yellow)),
            Span::raw("pohyb mezi agenty"),
        ]),
        Line::from(vec![
            Span::styled("Tab  ", Style::default().fg(Color::Yellow)),
            Span::raw("Live, History, Errors, Soul, Artifacts, LLM Output, Task Queue"),
        ]),
        Line::from(vec![
            Span::styled("a    ", Style::default().fg(Color::Green)),
            Span::raw("schválit výsledek při Human Review"),
        ]),
        Line::from(vec![
            Span::styled("r    ", Style::default().fg(Color::Red)),
            Span::raw("odmítnout výsledek a vrátit ho ke Generatorovi"),
        ]),
        Line::from(vec![
            Span::styled("n    ", Style::default().fg(Color::Magenta)),
            Span::raw("restartovat pipeline"),
        ]),
        Line::from(vec![
            Span::styled("g    ", Style::default().fg(Color::Green)),
            Span::raw("spustit vybraného agenta přes jeho LLM provider"),
        ]),
        Line::from(vec![
            Span::styled("t / l", Style::default().fg(Color::Yellow)),
            Span::raw(" přidat úkol / otevřít frontu úkolů; čekající úkoly se spustí po schválení"),
        ]),
        Line::from(vec![
            Span::styled("m    ", Style::default().fg(Color::Cyan)),
            Span::raw("vybrat model a provider pro označeného agenta"),
        ]),
        Line::from(vec![
            Span::styled("f    ", Style::default().fg(Color::Blue)),
            Span::raw("vypsat soubory v izolovaném workspace"),
        ]),
        Line::from(vec![
            Span::styled("v    ", Style::default().fg(Color::Yellow)),
            Span::raw("spustit bezpečný git status, pokud je terminál povolen"),
        ]),
        Line::from(vec![
            Span::styled("o    ", Style::default().fg(Color::Magenta)),
            Span::raw("připojit OpenClaw; po připojení vytvořit skutečný úkol/session"),
        ]),
        Line::from(vec![
            Span::styled("b    ", Style::default().fg(Color::Green)),
            Span::raw("načíst tasks.txt a postupně spustit dávku přes OpenClaw"),
        ]),
        Line::from(vec![
            Span::styled("x    ", Style::default().fg(Color::Red)),
            Span::raw("zastavit aktivní OpenClaw session"),
        ]),
        Line::from(vec![
            Span::styled("w / c", Style::default().fg(Color::Magenta)),
            Span::raw(" změnit workspace / barevné téma"),
        ]),
        Line::from(vec![
            Span::styled("Space", Style::default().fg(Color::Yellow)),
            Span::raw(" pozastavit nebo obnovit běh"),
        ]),
        Line::from(vec![
            Span::styled("? / F1", Style::default().fg(Color::Cyan)),
            Span::raw(" zobrazit nebo zavřít tuto nápovědu"),
        ]),
        Line::from(vec![
            Span::styled("q / Esc", Style::default().fg(Color::Cyan)),
            Span::raw(" ukončit aplikaci"),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: true }).block(
            Block::default()
                .title(" USER GUIDE ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .style(Style::default().bg(Color::Rgb(12, 20, 30)))
                .padding(ratatui::widgets::Padding::uniform(2)),
        ),
        popup,
    );
}

fn pipeline_label(state: PipelineState) -> &'static str {
    match state {
        PipelineState::Running => "RUNNING",
        PipelineState::AwaitingApproval => "HUMAN REVIEW",
        PipelineState::Completed => "COMPLETED",
    }
}

fn next_action(state: PipelineState) -> &'static str {
    match state {
        PipelineState::Running => "Agent is working; monitor tools and token usage",
        PipelineState::AwaitingApproval => {
            "Review the result, then press a to approve or r to reject"
        }
        PipelineState::Completed => "Pipeline completed; press n to start a new run",
    }
}

fn format_duration(seconds: u64) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn compact_json(value: &serde_json::Value, limit: usize) -> String {
    let raw = value.to_string();
    if raw.chars().count() > limit {
        format!(
            "{}…",
            raw.chars()
                .take(limit.saturating_sub(1))
                .collect::<String>()
        )
    } else {
        raw
    }
}

fn project_path(configured: &str) -> PathBuf {
    let path = PathBuf::from(configured);
    if path.is_absolute() {
        path
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
    }
}

fn openclaw_event_outcome(
    name: &str,
    payload: &serde_json::Value,
) -> Option<(TaskOutcome, String)> {
    if name != "agent"
        || payload.get("stream").and_then(serde_json::Value::as_str) != Some("lifecycle")
    {
        return None;
    }
    let data = payload.get("data")?;
    match data.get("phase").and_then(serde_json::Value::as_str)? {
        "end" if data.get("aborted").and_then(serde_json::Value::as_bool) != Some(true) => {
            Some((TaskOutcome::Success, "OpenClaw lifecycle completed".into()))
        }
        "end" => Some((TaskOutcome::Error, "OpenClaw task was aborted".into())),
        "error" => Some((
            TaskOutcome::Error,
            data.get("error")
                .or_else(|| data.get("message"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("OpenClaw lifecycle error")
                .to_string(),
        )),
        _ => None,
    }
}

fn extract_openclaw_sessions(payload: &serde_json::Value) -> Vec<String> {
    let rows = payload
        .pointer("/list/sessions")
        .or_else(|| payload.pointer("/list/rows"))
        .or_else(|| payload.get("sessions"))
        .or_else(|| payload.get("list"))
        .and_then(serde_json::Value::as_array);
    rows.into_iter()
        .flatten()
        .filter_map(|row| {
            let key = row
                .get("sessionKey")
                .or_else(|| row.get("key"))
                .and_then(serde_json::Value::as_str)?;
            let label = row
                .get("title")
                .or_else(|| row.get("label"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("untitled");
            Some(format!("{key} — {label}"))
        })
        .collect()
}

fn file_diff(path: &str, previous: Option<&str>, current: &str) -> String {
    let old = previous.unwrap_or("");
    let mut output = format!("--- {path}\n+++ {path}\n");
    let old_lines = old.lines().collect::<Vec<_>>();
    let new_lines = current.lines().collect::<Vec<_>>();
    let shared = old_lines.len().min(new_lines.len());
    for index in 0..shared {
        if old_lines[index] == new_lines[index] {
            output.push_str(&format!("  {}\n", old_lines[index]));
        } else {
            output.push_str(&format!("- {}\n+ {}\n", old_lines[index], new_lines[index]));
        }
    }
    for line in old_lines.iter().skip(shared) {
        output.push_str(&format!("- {}\n", line));
    }
    for line in new_lines.iter().skip(shared) {
        output.push_str(&format!("+ {}\n", line));
    }
    output.push('\n');
    output
}

fn draw_agents(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let header = Row::new(vec![Cell::from("AGENTS"), Cell::from("STATUS")]).style(
        Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::BOLD),
    );
    let rows = app.agents.iter().enumerate().map(|(idx, agent)| {
        let marker = if idx == app.selected { "▶ " } else { "  " };
        let row = Row::new(vec![
            Cell::from(format!("{}{}\n  {}", marker, agent.name, agent.role)),
            Cell::from(agent.status.label()).style(Style::default().fg(agent.status.color())),
        ])
        .height(2);
        if idx == app.selected {
            row.style(Style::default().bg(Color::Rgb(20, 45, 70)))
        } else {
            row
        }
    });
    let table = Table::new(
        rows,
        [Constraint::Percentage(68), Constraint::Percentage(32)],
    )
    .header(header)
    .block(
        Block::default()
            .title(" PIPELINE NODES ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue)),
    );
    frame.render_widget(table, area);
}
fn draw_detail(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let selected = app.selected_agent();
    let workspace_info = app
        .active_workspace()
        .map(|workspace| {
            format!(
                "{} — {} ({} agents)",
                workspace.name,
                workspace.description,
                workspace.agents.len()
            )
        })
        .unwrap_or_else(|| "No workspace configured".into());
    // Keep the active tab useful even in a conventional 80x24 terminal. On
    // taller screens the complete metadata panel is still shown.
    let info_height = area.height.saturating_sub(10).clamp(8, 14);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(info_height),
            Constraint::Min(0),
        ])
        .split(area);
    let tabs = Tabs::new([
        " LIVE OVERVIEW ",
        " STEP HISTORY ",
        " ERRORS ",
        " SOUL ",
        " ARTIFACTS ",
        " LLM OUTPUT ",
        " TASK QUEUE ",
    ])
    .select(app.tab)
    .highlight_style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )
    .block(
        Block::default()
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .title(" OBSERVABILITY "),
    );
    frame.render_widget(tabs, chunks[0]);
    let info = vec![
        Line::from(format!("TASK   {}", app.task)),
        Line::from(format!("SPACE  {}", workspace_info)),
        Line::from(vec![
            Span::styled("AGENT  ", Style::default().fg(Color::Gray)),
            Span::styled(
                selected.name.as_str(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("    "),
            Span::styled(
                selected.status.label(),
                Style::default().fg(selected.status.color()),
            ),
        ]),
        Line::from(format!("ROLE   {}", selected.role)),
        Line::from(format!("PROVIDER  {}", app.selected_provider())),
        Line::from(format!("MODEL  {}", selected.model)),
        Line::from(format!(
            "LLM    {}",
            app.llm_running
                .as_deref()
                .map(|agent| format!("RUNNING for {agent}"))
                .unwrap_or_else(|| "idle — press g to run".into())
        )),
        Line::from(format!(
            "OPENCLAW  {}",
            if app.openclaw_connected {
                "CONNECTED — press o for a real task"
            } else if app.openclaw_updates.is_some() {
                "CONNECTING"
            } else {
                "DISCONNECTED — press o to connect"
            }
        )),
        Line::from(format!("TOOL   {}", selected.tool)),
        Line::from(format!(
            "FILES  {}{}",
            app.sandbox
                .as_ref()
                .map(|sandbox| sandbox.root().display().to_string())
                .unwrap_or_else(|| "sandbox unavailable".into()),
            match (
                app.config.security.read_only,
                app.config.security.allow_terminal
            ) {
                (true, true) => " (read-only, terminal enabled)",
                (true, false) => " (read-only, terminal disabled)",
                (false, true) => " (read/write, terminal enabled)",
                (false, false) => " (read/write, terminal disabled)",
            }
        )),
        Line::from(format!(
            "SOUL   {}",
            app.config
                .agents
                .iter()
                .find(|agent| agent.id == selected.name)
                .map(|agent| agent.soul_file.as_str())
                .unwrap_or("not configured")
        )),
        Line::from(format!(
            "       {}",
            app.config
                .agents
                .iter()
                .find(|agent| agent.id == selected.name)
                .map(|agent| soul::preview(&agent.soul_file, 54))
                .unwrap_or_else(|| "SOUL.md not configured".into())
        )),
        Line::from(format!(
            "TIME   {}   •   TOKENS  {:>6}   •   EST. COST  ${:.4}",
            format_duration(selected.duration_secs),
            selected.tokens,
            selected.tokens as f32 * 0.000012
        )),
        Line::from(vec![
            Span::styled("NEXT   ", Style::default().fg(Color::Gray)),
            Span::styled(
                next_action(app.pipeline),
                Style::default().fg(Color::Yellow),
            ),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(info).block(
            Block::default()
                .borders(Borders::LEFT | Borders::RIGHT)
                .padding(ratatui::widgets::Padding::horizontal(2)),
        ),
        chunks[1],
    );
    match app.tab {
        0 => draw_live(frame, app, chunks[2]),
        1 => draw_history(frame, app, chunks[2]),
        2 => draw_errors(frame, app, chunks[2]),
        3 => draw_soul(frame, app, chunks[2]),
        4 => draw_artifacts(frame, app, chunks[2]),
        5 => draw_llm_output(frame, app, chunks[2]),
        _ => draw_task_queue(frame, app, chunks[2]),
    }
}
fn draw_live(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(area);
    let logs = app
        .events
        .iter()
        .rev()
        .take(7)
        .rev()
        .cloned()
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(logs.into_iter().map(ListItem::new)).block(
            Block::default()
                .title(" EVENT STREAM ")
                .borders(Borders::ALL),
        ),
        cols[0],
    );
    let progress = if app.pipeline == PipelineState::Completed {
        1.0
    } else {
        ((app.step_elapsed % 15) as f64) / 15.0
    };
    let gauge = Gauge::default()
        .block(
            Block::default()
                .title(" CURRENT STEP ")
                .borders(Borders::ALL),
        )
        .gauge_style(Style::default().fg(Color::Cyan))
        .label(format!("step progress  {:>3}%", (progress * 100.0) as u16))
        .ratio(progress);
    frame.render_widget(gauge, cols[1]);
}
fn draw_history(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let rows = app.history.iter().enumerate().map(|(index, record)| {
        Row::new(
            [
                format!("{:02}", index + 1),
                record.step.to_string(),
                format_duration(record.duration_secs),
                record.tokens.to_string(),
                record.status.label().to_string(),
            ]
            .into_iter()
            .map(Cell::from),
        )
    });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(5),
                Constraint::Percentage(35),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(12),
            ],
        )
        .header(
            Row::new(["STEP", "AGENT", "DURATION", "TOKENS", "STATE"])
                .style(Style::default().fg(Color::Gray)),
        )
        .block(
            Block::default()
                .title(" EXECUTION HISTORY ")
                .borders(Borders::ALL),
        ),
        area,
    );
}
fn draw_errors(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let error = if app.errors.is_empty() {
        vec![
            Line::from(Span::styled(
                "No active errors",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from("The reviewer will report syntax or policy issues here."),
            Line::from("Reject a review to simulate a feedback error."),
        ]
    } else {
        app.errors
            .iter()
            .map(|error| {
                Line::from(Span::styled(
                    format!("✗ {}", error),
                    Style::default().fg(Color::Red),
                ))
            })
            .collect()
    };
    frame.render_widget(
        Paragraph::new(error)
            .wrap(Wrap { trim: true })
            .block(Block::default().title(" ERROR LOG ").borders(Borders::ALL)),
        area,
    );
}

fn draw_soul(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let soul_path = app
        .config
        .agents
        .iter()
        .find(|agent| agent.id == app.selected_agent().name)
        .map(|agent| agent.soul_file.as_str());
    let lines = soul_path
        .and_then(soul::load)
        .map(|content| {
            content
                .lines()
                .map(|line| Line::from(line.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec![Line::from("SOUL.md is not configured for this agent.")]);
    let title = format!(" SOUL.md · {} ", soul_path.unwrap_or("not configured"));
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Magenta)),
        ),
        area,
    );
}

fn draw_artifacts(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let mut lines = match &app.sandbox {
        Some(sandbox) => match sandbox.list_workspace() {
            Ok(entries) if entries.is_empty() => vec![Line::from("Workspace is empty.")],
            Ok(entries) => entries
                .into_iter()
                .map(|entry| Line::from(format!("• {entry}")))
                .collect(),
            Err(error) => vec![Line::from(format!("Cannot list artifacts: {error}"))],
        },
        None => vec![Line::from("Filesystem sandbox is unavailable.")],
    };
    if !app.diff_output.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "DIFF BEFORE APPROVAL",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        lines.extend(app.diff_output.lines().map(|line| {
            let color = if line.starts_with('+') {
                Color::Green
            } else if line.starts_with('-') {
                Color::Red
            } else {
                Color::Gray
            };
            Line::from(Span::styled(line.to_string(), Style::default().fg(color)))
        }));
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(" WORKSPACE ARTIFACTS · press f to refresh, v for git status ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue)),
        ),
        area,
    );
}

fn draw_llm_output(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let agent = &app.selected_agent().name;
    let content = if app.llm_running.as_ref() == Some(agent) {
        format!("Request is running for {agent}. The UI remains responsive.")
    } else {
        app.llm_outputs
            .get(agent)
            .cloned()
            .unwrap_or_else(|| "No response yet. Choose a model with m, then press g.".into())
    };
    frame.render_widget(
        Paragraph::new(content).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(format!(" FULL LLM RESPONSE · {agent} "))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green)),
        ),
        area,
    );
}

fn draw_task_queue(frame: &mut ratatui::Frame, app: &App, area: Rect) {
    let mut lines = vec![Line::from(vec![
        Span::styled("ACTIVE  ", Style::default().fg(Color::Gray)),
        Span::styled(app.task.as_str(), Style::default().fg(Color::White)),
        Span::raw("  ["),
        Span::styled(
            pipeline_label(app.pipeline),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw("]"),
    ])];
    if app.task_queue.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from("Queue is empty. Press t to add another task."));
    } else {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "WAITING TASKS",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        for queued in &app.task_queue {
            lines.push(Line::from(format!(
                "#{}  {}  [QUEUED]",
                queued.id, queued.text
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from("They start automatically after Human Approval."));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!(
            "OPENCLAW BATCH  done={}/{}  errors={}  waiting={}",
            app.openclaw_batch_success + app.openclaw_batch_errors,
            app.openclaw_batch_total,
            app.openclaw_batch_errors,
            app.openclaw_batch_queue.len()
        ),
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    )));
    if let Some(active) = &app.openclaw_batch_active {
        lines.push(Line::from(format!(
            "ACTIVE #{}  {}",
            active.id, active.text
        )));
    } else {
        lines.push(Line::from(
            "Press b to load tasks.txt; results are written to one report file.",
        ));
    }
    for queued in &app.openclaw_batch_queue {
        lines.push(Line::from(format!(
            "WAITING #{}  {}",
            queued.id, queued.text
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("OPENCLAW SESSIONS ({})", app.openclaw_sessions.len()),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    if app.openclaw_sessions.is_empty() {
        lines.push(Line::from("Press o to connect to the Gateway."));
    } else {
        lines.extend(
            app.openclaw_sessions
                .iter()
                .map(|session| Line::from(format!("• {session}"))),
        );
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(Block::default().title(" TASK QUEUE ").borders(Borders::ALL)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::backend::TestBackend;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn rendered_text(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer.cell((x, y)).unwrap().symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn at_human_gate() -> App {
        let mut app = App::new();
        for _ in 0..23 {
            app.tick();
        }
        assert_eq!(app.pipeline, PipelineState::AwaitingApproval);
        app
    }

    #[test]
    fn app_loads_sample_event_and_error_data() {
        let app = App::new();
        assert!(
            app.events
                .iter()
                .any(|event| event.contains("filesystem.write"))
        );
        assert!(
            app.errors
                .iter()
                .any(|error| error.contains("missing fixture"))
        );
    }

    #[test]
    fn keyboard_selection_changes_detail_target() {
        let mut app = App::new();
        assert_eq!(app.selected_agent().name, "code-generator");
        app.handle_key(key(KeyCode::Char('1')));
        assert_eq!(app.selected_agent().name, "prompt-architect");
        app.handle_key(key(KeyCode::Char('4')));
        assert_eq!(app.selected_agent().name, "quality-gate");
        app.handle_key(key(KeyCode::Char('j')));
        assert_eq!(app.selected, 0);
        app.handle_key(key(KeyCode::Char('k')));
        assert_eq!(app.selected, 3);
        app.select_agent(99);
        assert_eq!(app.selected, 3);
    }

    #[test]
    fn mouse_click_selects_agent_row() {
        let mut app = App::new();
        let area = Rect::new(0, 0, 120, 40);
        app.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 10,
                row: 10,
                modifiers: KeyModifiers::NONE,
            },
            area,
        );
        assert_eq!(app.selected, 2);
        app.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Right),
                column: 10,
                row: 8,
                modifiers: KeyModifiers::NONE,
            },
            area,
        );
        assert_eq!(app.selected, 2);
    }

    #[test]
    fn mouse_click_selects_observability_tab() {
        let mut app = App::new();
        let area = Rect::new(0, 0, 120, 40);
        app.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 105,
                row: 4,
                modifiers: KeyModifiers::NONE,
            },
            area,
        );
        assert_eq!(app.tab, 4);
    }

    #[test]
    fn footer_buttons_are_clickable() {
        let area = Rect::new(0, 0, 120, 40);
        let mut app = App::new();
        assert!(!app.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 25,
                row: 38,
                modifiers: KeyModifiers::NONE,
            },
            area,
        ));
        assert!(app.input_mode);

        let mut app = App::new();
        assert!(!app.paused);
        app.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 67,
                row: 39,
                modifiers: KeyModifiers::NONE,
            },
            area,
        );
        assert!(app.paused);

        let mut app = App::new();
        assert!(app.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 5,
                row: 38,
                modifiers: KeyModifiers::NONE,
            },
            area,
        ));
    }

    #[test]
    fn keyboard_navigation_and_pause_work() {
        let mut app = App::new();
        assert_eq!(app.tab, 0);
        app.handle_key(key(KeyCode::Tab));
        assert_eq!(app.tab, 1);
        app.handle_key(key(KeyCode::Right));
        assert_eq!(app.tab, 2);
        app.handle_key(key(KeyCode::Left));
        assert_eq!(app.tab, 1);
        app.handle_key(key(KeyCode::Char(' ')));
        let elapsed = app.elapsed;
        app.tick();
        assert_eq!(app.elapsed, elapsed);
        app.handle_key(key(KeyCode::Char(' ')));
        app.tick();
        assert_eq!(app.elapsed, elapsed + 1);
    }

    #[test]
    fn control_c_always_quits_even_inside_input_dialog() {
        let mut app = App::new();
        app.handle_key(key(KeyCode::Char('t')));
        assert!(app.input_mode);

        assert!(app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL,)));
    }

    #[test]
    fn release_only_terminal_events_are_supported_without_double_actions() {
        let mut app = App::new();
        let press = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        let release = KeyEvent::new_with_kind(
            KeyCode::Char('j'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        app.handle_key_event(press);
        assert_eq!(app.selected, 2);
        app.handle_key_event(release);
        assert_eq!(app.selected, 2);

        let mut release_only = App::new();
        release_only.handle_key_event(KeyEvent::new_with_kind(
            KeyCode::Char('1'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        assert_eq!(release_only.selected_agent().name, "prompt-architect");
    }

    #[test]
    fn interleaved_press_and_release_events_do_not_repeat_old_keys() {
        let mut app = App::new();
        assert_eq!(app.selected, 1);

        app.handle_key_event(KeyEvent::new_with_kind(
            KeyCode::Char('j'),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        ));
        app.handle_key_event(KeyEvent::new_with_kind(
            KeyCode::Char('k'),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        ));
        assert_eq!(app.selected, 1);

        app.handle_key_event(KeyEvent::new_with_kind(
            KeyCode::Char('j'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));
        app.handle_key_event(KeyEvent::new_with_kind(
            KeyCode::Char('k'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        ));

        assert_eq!(app.selected, 1);
        assert!(app.pressed_keys.is_empty());
    }

    #[test]
    fn event_loop_waits_for_input_instead_of_busy_redrawing() {
        assert_eq!(
            input_poll_timeout(Duration::from_millis(250)),
            Duration::from_millis(750)
        );
        assert_eq!(
            input_poll_timeout(Duration::from_millis(999)),
            Duration::from_millis(1)
        );
        assert_eq!(input_poll_timeout(Duration::from_secs(1)), Duration::ZERO);
    }

    #[test]
    fn task_input_creates_new_pipeline() {
        let mut app = App::new();
        app.handle_key(key(KeyCode::Char('t')));
        assert!(app.input_mode);
        for character in "Build a REST API".chars() {
            app.handle_key(key(KeyCode::Char(character)));
        }
        app.handle_key(key(KeyCode::Backspace));
        app.handle_key(key(KeyCode::Char('I')));
        app.handle_key(key(KeyCode::Enter));
        assert!(!app.input_mode);
        assert_eq!(app.task, "Build a REST API");
        assert_eq!(app.pipeline, PipelineState::Running);
    }

    #[test]
    fn task_input_escape_does_not_change_current_task() {
        let mut app = App::new();
        let original = app.task.clone();
        app.handle_key(key(KeyCode::Char('t')));
        app.handle_key(key(KeyCode::Char('x')));
        app.handle_key(key(KeyCode::Esc));
        assert!(!app.input_mode);
        assert_eq!(app.task, original);
    }

    #[test]
    fn new_task_during_active_real_pipeline_is_added_to_queue() {
        let mut app = App::new();
        app.real_pipeline = true;
        app.pipeline = PipelineState::Running;
        let active = app.task.clone();
        app.start_task("Second task prepared in advance".into());
        assert_eq!(app.task, active);
        assert_eq!(app.task_queue.len(), 1);
        assert_eq!(app.task_queue[0].text, "Second task prepared in advance");
        assert!(app.events.iter().any(|event| event.contains("Task queued")));
    }

    #[test]
    fn approved_task_starts_the_next_queued_task() {
        let mut app = App::new();
        for agent in &mut app.config.agents {
            agent.provider = "local".into();
            agent.model = "demo-model".into();
        }
        app.real_pipeline = true;
        app.pipeline = PipelineState::AwaitingApproval;
        app.agents[3].status = Status::Running;
        app.enqueue_task("Run the second task".into());
        app.approve();
        assert_eq!(app.task, "Run the second task");
        assert!(app.task_queue.is_empty());
        assert!(
            app.events
                .iter()
                .any(|event| event.contains("Starting queued task"))
        );
    }

    #[test]
    fn task_queue_tab_is_reachable_and_shows_waiting_work() {
        let mut app = App::new();
        app.real_pipeline = true;
        app.enqueue_task("Queued visible task".into());
        app.handle_key(key(KeyCode::Char('l')));
        assert_eq!(app.tab, 6);
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let rendered = rendered_text(&terminal);
        assert!(rendered.contains("TASK QUEUE"));
        assert!(rendered.contains("Queued visible task"));
    }

    #[test]
    fn reject_key_collects_feedback_and_returns_pipeline_to_generator() {
        let mut app = at_human_gate();
        app.handle_key(key(KeyCode::Char('r')));
        assert!(app.input_mode);
        assert!(app.feedback_mode);
        for character in "Improve error handling".chars() {
            app.handle_key(key(KeyCode::Char(character)));
        }
        app.handle_key(key(KeyCode::Enter));
        assert!(!app.input_mode);
        assert_eq!(app.pipeline, PipelineState::Running);
        assert_eq!(app.agents[1].status, Status::Running);
        assert!(
            app.errors
                .iter()
                .any(|error| error.contains("Improve error handling"))
        );
    }

    #[test]
    fn escape_cancels_reject_feedback_without_changing_pipeline() {
        let mut app = at_human_gate();
        app.handle_key(key(KeyCode::Char('r')));
        app.handle_key(key(KeyCode::Char('x')));
        app.handle_key(key(KeyCode::Esc));
        assert!(!app.input_mode);
        assert!(!app.feedback_mode);
        assert_eq!(app.pipeline, PipelineState::AwaitingApproval);
    }

    #[test]
    fn reject_feedback_dialog_is_visible_to_user() {
        let mut app = at_human_gate();
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        app.handle_key(key(KeyCode::Char('r')));
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let rendered = rendered_text(&terminal);
        assert!(rendered.contains("REVIEW FEEDBACK"));
        assert!(rendered.contains("REJECT WITH FEEDBACK"));
    }

    #[test]
    fn task_input_is_visible_in_real_tui_rendering() {
        let mut app = App::new();
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        app.handle_key(key(KeyCode::Char('t')));
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        assert!(rendered_text(&terminal).contains("TASK INPUT"));
        for character in "Build API".chars() {
            app.handle_key(key(KeyCode::Char(character)));
        }
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        assert!(rendered_text(&terminal).contains("Build API"));
        app.handle_key(key(KeyCode::Enter));
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        assert!(!rendered_text(&terminal).contains("TASK INPUT"));
        assert_eq!(app.task, "Build API");
    }

    #[test]
    fn user_can_run_selected_agent_with_offline_demo_provider() {
        let mut app = App::new();
        app.select_agent(3);
        let tokens = app.agents[3].tokens;
        app.run_selected_agent();
        assert!(app.agents[3].tokens > tokens);
        assert!(
            app.events
                .iter()
                .any(|event| event.contains("LLM response") && event.contains("quality-gate"))
        );
        assert!(app.llm_outputs["quality-gate"].contains("Create a backend module in Python"));
    }

    #[test]
    fn full_llm_output_tab_shows_untruncated_response() {
        let mut app = App::new();
        app.select_agent(3);
        app.run_selected_agent();
        app.tab = 5;
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let rendered = rendered_text(&terminal);
        assert!(rendered.contains("FULL LLM RESPONSE"));
        assert!(rendered.contains("Demo response for"));
    }

    #[test]
    fn full_llm_output_remains_visible_in_standard_80x24_terminal() {
        let mut app = App::new();
        app.select_agent(3);
        app.run_selected_agent();
        app.tab = 5;
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let rendered = rendered_text(&terminal);
        assert!(rendered.contains("FULL LLM RESPONSE"));
        assert!(rendered.contains("Demo response for"));
    }

    #[test]
    fn failed_llm_request_is_visible_in_errors_and_full_output() {
        let mut app = App::new();
        app.finish_llm_job(LlmJobResult {
            agent_id: "code-generator".into(),
            provider_id: "ollama".into(),
            result: Err("connection refused at localhost:11434".into()),
        });
        assert!(app.errors.iter().any(|error| {
            error.contains("code-generator") && error.contains("connection refused")
        }));
        assert_eq!(
            app.llm_outputs.get("code-generator").map(String::as_str),
            Some("ERROR: connection refused at localhost:11434")
        );

        app.tab = 5;
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let rendered = rendered_text(&terminal);
        assert!(rendered.contains("ERROR: connection refused"));
    }

    #[test]
    fn user_can_choose_model_for_selected_agent() {
        let mut app = App::new();
        app.select_agent(1);
        app.handle_key(key(KeyCode::Char('m')));
        assert!(app.model_mode);
        assert_eq!(app.config.models[app.model_index].provider, "ollama");
        app.handle_key(key(KeyCode::Down));
        app.handle_key(key(KeyCode::Enter));
        assert!(!app.model_mode);
        assert_eq!(app.selected_provider(), "openai");
        assert_eq!(app.selected_agent().model, "gpt-4o");
        assert!(app.events.iter().any(|event| {
            event.contains("Model binding changed") && event.contains("provider=openai")
        }));
    }

    #[test]
    fn selected_model_bindings_survive_new_task_start() {
        let mut app = App::new();
        app.select_agent(0);
        app.open_model_selector();
        app.model_index = 1;
        app.apply_model_selection();
        assert_eq!(app.selected_provider(), "ollama");
        app.start_task("Use the selected local provider".into());
        assert_eq!(app.selected_provider(), "ollama");
        assert_eq!(app.config.agents[0].model, "qwen2.5-coder:14b");
        assert_eq!(app.agents[0].model, "qwen2.5-coder:14b");
    }

    #[test]
    fn model_selector_is_visible_and_escape_cancels_changes() {
        let mut app = App::new();
        let original = app.selected_agent().model.clone();
        app.handle_key(key(KeyCode::Char('m')));
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let rendered = rendered_text(&terminal);
        assert!(rendered.contains("MODEL / PROVIDER"));
        assert!(rendered.contains("Offline Demo"));
        assert!(rendered.contains("Ollama"));
        assert!(rendered.contains("requires service at http://localhost:11434"));
        app.handle_key(key(KeyCode::Down));
        app.handle_key(key(KeyCode::Esc));
        assert!(!app.model_mode);
        assert_eq!(app.selected_agent().model, original);
    }

    #[test]
    fn provider_requirements_are_explained_before_running_a_model() {
        let app = App::new();
        assert_eq!(app.provider_status("local"), "READY (offline)");
        assert_eq!(
            app.provider_status("ollama"),
            "requires service at http://localhost:11434"
        );
        assert_eq!(app.provider_status("missing"), "NOT CONFIGURED");
    }

    #[test]
    fn model_profile_can_be_selected_with_mouse() {
        let mut app = App::new();
        app.select_agent(1);
        app.open_model_selector();
        let area = Rect::new(0, 0, 120, 40);
        let first_model_row = area.height / 5 + 4;
        app.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 20,
                row: first_model_row,
                modifiers: KeyModifiers::NONE,
            },
            area,
        );
        assert!(!app.model_mode);
        assert_eq!(app.selected_provider(), "local");
        assert_eq!(app.selected_agent().model, "demo-model");
    }

    #[test]
    fn real_provider_request_returns_control_without_blocking_ui() {
        let mut app = App::new();
        app.select_agent(1);
        let started = Instant::now();
        app.run_selected_agent();
        assert!(started.elapsed() < Duration::from_millis(200));
        assert_eq!(app.llm_running.as_deref(), Some("code-generator"));
        assert!(
            app.events
                .iter()
                .any(|event| event.contains("LLM request started"))
        );
    }

    #[test]
    fn user_can_inspect_files_through_workspace_sandbox() {
        let mut app = App::new();
        app.inspect_workspace();
        assert!(
            app.events
                .iter()
                .any(|event| event.contains("MCP filesystem.list") && event.contains("Cargo.toml"))
        );
    }

    #[test]
    fn configured_agents_drive_runtime_names_roles_and_models() {
        let config = Config::load("config.toml");
        let agents = build_agents(&config);
        assert_eq!(agents.len(), config.agents.len());
        assert_eq!(agents[1].name, config.agents[1].id);
        assert_eq!(agents[1].role, config.agents[1].role);
        assert_eq!(agents[1].model, config.agents[1].model);
    }

    #[test]
    fn git_action_explains_that_terminal_is_disabled_by_default() {
        let mut app = App::new();
        app.handle_key(key(KeyCode::Char('v')));
        assert!(
            app.errors
                .iter()
                .any(|error| error.contains("terminal access is disabled"))
        );
    }

    #[test]
    fn openclaw_action_explains_how_to_enable_gateway_integration() {
        let mut app = App::new();
        app.config.openclaw.enabled = false;
        app.handle_key(key(KeyCode::Char('o')));
        assert!(
            app.errors
                .iter()
                .any(|error| error.contains("openclaw.enabled = true"))
        );
    }

    #[test]
    fn connected_openclaw_action_opens_task_dialog_and_submits_session() {
        let mut app = App::new();
        let (commands, receiver) = mpsc::channel();
        app.openclaw_commands = Some(commands);
        app.openclaw_connected = true;
        app.handle_key(key(KeyCode::Char('o')));
        assert!(app.input_mode);
        assert!(app.openclaw_input_mode);
        for character in "Inspect this repository".chars() {
            app.handle_key(key(KeyCode::Char(character)));
        }
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(
            receiver.recv().unwrap(),
            GatewayCommand::CreateTask {
                task: "Inspect this repository".into()
            }
        );
        assert_eq!(app.task, "Inspect this repository");
    }

    #[test]
    fn user_can_abort_the_active_openclaw_session() {
        let mut app = App::new();
        let (commands, receiver) = mpsc::channel();
        app.openclaw_commands = Some(commands);
        app.openclaw_connected = true;
        app.active_openclaw_session = Some("agent:main:mission-control".into());
        app.handle_key(key(KeyCode::Char('x')));
        assert_eq!(
            receiver.recv().unwrap(),
            GatewayCommand::AbortSession {
                session_key: "agent:main:mission-control".into()
            }
        );
        assert!(
            app.events
                .iter()
                .any(|event| event.contains("abort requested"))
        );
    }

    #[test]
    fn openclaw_roster_parser_accepts_subscribe_snapshot() {
        let sessions = extract_openclaw_sessions(&serde_json::json!({
            "subscribed": true,
            "list": {
                "sessions": [
                    {"sessionKey": "agent:main:one", "title": "First task"},
                    {"key": "agent:main:two", "label": "Second task"}
                ]
            }
        }));
        assert_eq!(sessions.len(), 2);
        assert!(sessions[0].contains("First task"));
        assert!(sessions[1].contains("agent:main:two"));
    }

    #[test]
    fn openclaw_stream_updates_are_visible_in_event_and_error_logs() {
        let mut app = App::new();
        let (sender, receiver) = mpsc::channel();
        app.openclaw_updates = Some(receiver);
        sender.send(GatewayUpdate::Connected).unwrap();
        sender
            .send(GatewayUpdate::Event {
                name: "session.tool".into(),
                payload: serde_json::json!({"tool":"terminal.exec","status":"running"}),
            })
            .unwrap();
        sender
            .send(GatewayUpdate::Error("test failure".into()))
            .unwrap();
        app.poll_openclaw();
        assert!(app.openclaw_connected);
        assert!(
            app.events
                .iter()
                .any(|event| event.contains("session.tool"))
        );
        assert!(
            app.errors
                .iter()
                .any(|error| error.contains("test failure"))
        );
        assert!(
            app.events
                .iter()
                .any(|event| event.contains("OpenClaw ERROR") && event.contains("test failure"))
        );
    }

    #[test]
    fn openclaw_txt_batch_runs_sequentially_and_writes_one_final_report() {
        let unique = format!(
            "mission-control-ui-batch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let input = std::env::temp_dir().join(format!("{unique}.txt"));
        let report = std::env::temp_dir().join(format!("{unique}.log"));
        std::fs::write(
            &input,
            "Create first file\nCreate second file\nCheck README as the final task\n",
        )
        .unwrap();

        let mut app = App::new();
        app.config.openclaw.task_input_file = input.to_string_lossy().into_owned();
        app.config.openclaw.task_report_file = report.to_string_lossy().into_owned();
        app.openclaw_connected = true;
        let (command_sender, command_receiver) = mpsc::channel();
        app.openclaw_commands = Some(command_sender);
        let (update_sender, update_receiver) = mpsc::channel();
        app.openclaw_updates = Some(update_receiver);

        app.handle_key(key(KeyCode::Char('b')));
        assert_eq!(
            command_receiver.recv().unwrap(),
            GatewayCommand::CreateTask {
                task: "Create first file".into()
            }
        );
        assert_eq!(app.openclaw_batch_queue.len(), 2);

        update_sender
            .send(GatewayUpdate::Response {
                method: "create-2".into(),
                payload: serde_json::json!({"sessionKey":"agent:main:first"}),
            })
            .unwrap();
        update_sender
            .send(GatewayUpdate::Event {
                name: "agent".into(),
                payload: serde_json::json!({
                    "sessionKey":"agent:main:stale-session",
                    "stream":"lifecycle",
                    "data":{"phase":"end"}
                }),
            })
            .unwrap();
        app.poll_openclaw();
        assert_eq!(app.openclaw_batch_active.as_ref().unwrap().id, 1);
        assert!(matches!(
            command_receiver.try_recv(),
            Err(TryRecvError::Empty)
        ));

        update_sender
            .send(GatewayUpdate::Event {
                name: "agent".into(),
                payload: serde_json::json!({
                    "sessionKey":"agent:main:first",
                    "stream":"lifecycle",
                    "data":{"phase":"end"}
                }),
            })
            .unwrap();
        app.poll_openclaw();
        assert_eq!(
            command_receiver.recv().unwrap(),
            GatewayCommand::CreateTask {
                task: "Create second file".into()
            }
        );

        update_sender
            .send(GatewayUpdate::Response {
                method: "create-3".into(),
                payload: serde_json::json!({"sessionKey":"agent:main:second"}),
            })
            .unwrap();
        update_sender
            .send(GatewayUpdate::Event {
                name: "agent".into(),
                payload: serde_json::json!({
                    "sessionKey":"agent:main:second",
                    "stream":"lifecycle",
                    "data":{"phase":"end"}
                }),
            })
            .unwrap();
        app.poll_openclaw();
        assert_eq!(
            command_receiver.recv().unwrap(),
            GatewayCommand::CreateTask {
                task: "Check README as the final task".into()
            }
        );
        assert!(
            app.events
                .iter()
                .any(|event| event.contains("Check README as the final task"))
        );

        update_sender
            .send(GatewayUpdate::Response {
                method: "create-4".into(),
                payload: serde_json::json!({"sessionKey":"agent:main:third"}),
            })
            .unwrap();
        update_sender
            .send(GatewayUpdate::Event {
                name: "agent".into(),
                payload: serde_json::json!({
                    "sessionKey":"agent:main:third",
                    "stream":"lifecycle",
                    "data":{"phase":"end"}
                }),
            })
            .unwrap();
        app.poll_openclaw();

        assert!(app.openclaw_batch_active.is_none());
        assert!(app.openclaw_batch_queue.is_empty());
        let contents = std::fs::read_to_string(&report).unwrap();
        assert_eq!(contents.matches("status=SUCCESS").count(), 3);
        assert!(contents.contains("task=Check README as the final task"));
        assert!(contents.contains("SUMMARY total=3 success=3 errors=0"));
        std::fs::remove_file(input).unwrap();
        std::fs::remove_file(report).unwrap();
    }

    #[test]
    fn openclaw_lifecycle_abortion_is_an_error_not_success() {
        let outcome = openclaw_event_outcome(
            "agent",
            &serde_json::json!({
                "stream":"lifecycle",
                "data":{"phase":"end","aborted":true}
            }),
        );
        assert_eq!(
            outcome,
            Some((TaskOutcome::Error, "OpenClaw task was aborted".into()))
        );
    }

    #[test]
    fn workspace_switching_changes_active_context() {
        let mut app = App::new();
        assert_eq!(app.active_workspace().unwrap().id, "coding-space");
        app.handle_key(key(KeyCode::Char('w')));
        assert_eq!(app.active_workspace().unwrap().id, "content-space");
        app.handle_key(key(KeyCode::Char('w')));
        assert_eq!(app.active_workspace().unwrap().id, "graphics-space");
    }

    #[test]
    fn user_can_cycle_all_visual_themes() {
        let mut app = App::new();
        assert_eq!(app.theme, Theme::Dark);
        app.handle_key(key(KeyCode::Char('c')));
        assert_eq!(app.theme, Theme::Light);
        app.handle_key(key(KeyCode::Char('c')));
        assert_eq!(app.theme, Theme::Starfall);
        app.handle_key(key(KeyCode::Char('c')));
        assert_eq!(app.theme, Theme::Dark);
    }

    #[test]
    fn soul_tab_is_reachable_for_selected_agent() {
        let mut app = App::new();
        for _ in 0..3 {
            app.handle_key(key(KeyCode::Tab));
        }
        assert_eq!(app.tab, 3);
        let path = app
            .config
            .agents
            .iter()
            .find(|agent| agent.id == app.selected_agent().name)
            .unwrap()
            .soul_file
            .clone();
        assert!(soul::load(path).unwrap().contains("approved specification"));
    }

    #[test]
    fn artifacts_tab_shows_workspace_files() {
        let mut app = App::new();
        app.tab = 4;
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
        let rendered = rendered_text(&terminal);
        assert!(rendered.contains("WORKSPACE ARTIFACTS"));
        assert!(rendered.contains("Cargo.toml"));
    }

    #[test]
    fn invalid_actions_do_not_change_pipeline() {
        let mut app = App::new();
        app.approve();
        app.reject_with_feedback("revise implementation");
        assert_eq!(app.pipeline, PipelineState::Running);
        assert_eq!(app.agents[1].status, Status::Running);
    }

    #[test]
    fn pipeline_reaches_human_review() {
        let mut app = App::new();
        for _ in 0..15 {
            app.tick();
        }
        assert_eq!(app.agents[1].status, Status::Done);
        assert_eq!(app.agents[2].status, Status::Running);
        for _ in 0..8 {
            app.tick();
        }
        assert_eq!(app.pipeline, PipelineState::AwaitingApproval);
        assert_eq!(app.agents[3].status, Status::Running);
    }

    #[test]
    fn approval_completes_pipeline() {
        let mut app = at_human_gate();
        app.approve();
        assert_eq!(app.pipeline, PipelineState::Completed);
        assert_eq!(app.agents[3].status, Status::Done);
    }

    #[test]
    fn rejection_restarts_code_generation() {
        let mut app = at_human_gate();
        app.reject_with_feedback("revise implementation");
        assert_eq!(app.pipeline, PipelineState::Running);
        assert_eq!(app.agents[1].status, Status::Running);
        assert_eq!(app.agents[2].status, Status::Waiting);
    }

    #[test]
    fn rejected_pipeline_can_reach_human_gate_again() {
        let mut app = at_human_gate();
        app.reject_with_feedback("revise implementation");
        for _ in 0..23 {
            app.tick();
        }
        assert_eq!(app.pipeline, PipelineState::AwaitingApproval);
        assert_eq!(app.agents[3].status, Status::Running);
    }

    #[test]
    fn approve_and_reject_are_idempotent_outside_gate() {
        let mut app = at_human_gate();
        app.approve();
        let history_len = app.history.len();
        app.approve();
        app.reject_with_feedback("revise implementation");
        assert_eq!(app.pipeline, PipelineState::Completed);
        assert_eq!(app.history.len(), history_len);
    }

    #[test]
    fn external_event_updates_tokens_duration_and_error_log() {
        let mut app = App::new();
        let before = app.agents[1].tokens;
        app.ingest_event(AgentEvent {
            timestamp: "14:40:00".into(),
            agent: "code-generator".into(),
            kind: "error".into(),
            message: "terminal exited with code 1".into(),
            tokens: 11,
            duration_ms: 2200,
        });
        assert_eq!(app.agents[1].tokens, before + 11);
        assert_eq!(app.agents[1].duration_secs, 2);
        assert!(
            app.errors
                .iter()
                .any(|error| error.contains("terminal exited with code 1"))
        );
    }

    #[test]
    fn unknown_agent_event_is_logged_without_mutating_agents() {
        let mut app = App::new();
        let tokens = app
            .agents
            .iter()
            .map(|agent| agent.tokens)
            .collect::<Vec<_>>();
        app.ingest_event(AgentEvent {
            timestamp: "14:40:01".into(),
            agent: "unknown".into(),
            kind: "tool".into(),
            message: "mystery.call".into(),
            tokens: 99,
            duration_ms: 10,
        });
        assert!(
            app.events
                .iter()
                .any(|event| event.contains("mystery.call"))
        );
        assert_eq!(
            app.agents
                .iter()
                .map(|agent| agent.tokens)
                .collect::<Vec<_>>(),
            tokens
        );
    }

    #[test]
    fn restart_returns_to_initial_running_state() {
        let mut app = at_human_gate();
        app.approve();
        app.restart();
        assert_eq!(app.pipeline, PipelineState::Running);
        assert_eq!(app.selected, 1);
        assert_eq!(app.agents[0].status, Status::Running);
        assert!(
            app.events
                .iter()
                .any(|event| event.contains("Real pipeline started"))
        );
    }

    #[test]
    fn real_pipeline_runs_all_agents_and_writes_an_artifact_offline() {
        let root = std::env::temp_dir().join(format!(
            "mission-control-real-pipeline-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut app = App::new();
        app.sandbox = Some(FileSandbox::new(&root, false).unwrap());
        for agent in &mut app.config.agents {
            agent.provider = "local".into();
            agent.model = "demo-model".into();
        }
        for agent in &mut app.agents {
            agent.model = "demo-model".into();
        }
        app.task = "Create a tiny Python service".into();
        app.begin_real_pipeline();
        assert_eq!(app.pipeline, PipelineState::AwaitingApproval);
        assert_eq!(app.agents[0].status, Status::Done);
        assert_eq!(app.agents[1].status, Status::Done);
        assert_eq!(app.agents[2].status, Status::Done);
        assert_eq!(app.agents[3].status, Status::Running);
        assert!(root.join("demo_output.txt").exists());
        assert!(app.diff_output.contains("+++ demo_output.txt"));
        assert!(app.diff_output.contains("+ Generated"));
        assert!(
            app.events
                .iter()
                .any(|event| event.contains("Validation skipped"))
        );
        assert!(
            app.events
                .iter()
                .any(|event| event.contains("Reviewer report ready"))
        );
        app.reject_with_feedback("Add a regression test");
        assert_eq!(app.pipeline, PipelineState::AwaitingApproval);
        assert!(
            app.errors
                .iter()
                .any(|error| error.contains("Add a regression test"))
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn error_events_are_visible_in_error_log() {
        let mut app = App::new();
        app.ingest_event(AgentEvent {
            timestamp: "14:40:00".into(),
            agent: "code-generator".into(),
            kind: "error".into(),
            message: "terminal exited with code 1".into(),
            tokens: 0,
            duration_ms: 0,
        });
        assert!(
            app.errors
                .iter()
                .any(|error| error.contains("terminal exited with code 1"))
        );
    }
}
