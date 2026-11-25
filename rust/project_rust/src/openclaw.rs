use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io,
    net::TcpStream,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread,
    time::Duration,
};
use tungstenite::{Error as WsError, Message, WebSocket, connect, stream::MaybeTlsStream};

const PROTOCOL_VERSION: u64 = 4;

#[derive(Clone, Debug)]
pub struct GatewayConfig {
    pub url: String,
    pub token: String,
    pub agent_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GatewayCommand {
    CreateTask { task: String },
    AbortSession { session_key: String },
}

#[derive(Clone, Debug, PartialEq)]
pub enum GatewayUpdate {
    Connected,
    Sessions(Value),
    Event { name: String, payload: Value },
    Response { method: String, payload: Value },
    Error(String),
    Disconnected,
}

pub struct GatewayHandle {
    pub commands: Sender<GatewayCommand>,
    pub updates: Receiver<GatewayUpdate>,
}

#[derive(Default)]
struct TaskDispatch {
    pending_creates: HashMap<String, String>,
    pending_observes: HashMap<String, (String, String)>,
}

impl TaskDispatch {
    fn create_request(&mut self, id: &str, agent_id: &str, task: String) -> Value {
        self.pending_creates.insert(id.to_string(), task);
        create_task_request(id, agent_id)
    }

    fn follow_up(&mut self, frame: &Value, agent_id: &str, request_id: &mut u64) -> Option<Value> {
        if frame.get("type").and_then(Value::as_str) != Some("res") {
            return None;
        }
        let id = frame.get("id").and_then(Value::as_str)?;
        if frame.get("ok").and_then(Value::as_bool) != Some(true) {
            self.pending_creates.remove(id);
            self.pending_observes.remove(id);
            return None;
        }
        if let Some(task) = self.pending_creates.remove(id) {
            let key = frame
                .pointer("/payload/sessionKey")
                .or_else(|| frame.pointer("/payload/key"))
                .and_then(Value::as_str)?
                .to_string();
            let observe_id = next_request_id("observe", request_id);
            self.pending_observes
                .insert(observe_id.clone(), (key.clone(), task));
            return Some(observe_session_request(&observe_id, &key, id));
        }
        if let Some((key, task)) = self.pending_observes.remove(id) {
            let send_id = next_request_id("send", request_id);
            return Some(send_task_request(&send_id, agent_id, &key, &task));
        }
        None
    }
}

pub fn spawn(config: GatewayConfig) -> GatewayHandle {
    let (command_tx, command_rx) = mpsc::channel();
    let (update_tx, update_rx) = mpsc::channel();
    thread::spawn(move || {
        if let Err(error) = run(config, command_rx, &update_tx) {
            let _ = update_tx.send(GatewayUpdate::Error(error));
        }
        let _ = update_tx.send(GatewayUpdate::Disconnected);
    });
    GatewayHandle {
        commands: command_tx,
        updates: update_rx,
    }
}

fn run(
    config: GatewayConfig,
    commands: Receiver<GatewayCommand>,
    updates: &Sender<GatewayUpdate>,
) -> Result<(), String> {
    let (mut socket, _) = connect(config.url.as_str()).map_err(|error| error.to_string())?;
    set_read_timeout(socket.get_mut(), Duration::from_millis(150))
        .map_err(|error| error.to_string())?;

    let challenge = read_json(&mut socket)?;
    let nonce = challenge
        .get("event")
        .filter(|event| event.as_str() == Some("connect.challenge"))
        .and_then(|_| challenge.pointer("/payload/nonce"))
        .and_then(Value::as_str)
        .ok_or_else(|| "Gateway did not send a valid connect.challenge".to_string())?;
    socket
        .send(Message::text(connect_request(&config, nonce).to_string()))
        .map_err(|error| error.to_string())?;
    let hello = read_json(&mut socket)?;
    ensure_success(&hello, "connect-1")?;
    updates
        .send(GatewayUpdate::Connected)
        .map_err(|error| error.to_string())?;

    socket
        .send(Message::text(session_subscribe_request().to_string()))
        .map_err(|error| error.to_string())?;

    let mut request_id = 2_u64;
    let mut task_dispatch = TaskDispatch::default();
    loop {
        loop {
            match commands.try_recv() {
                Ok(GatewayCommand::CreateTask { task }) => {
                    let id = next_request_id("create", &mut request_id);
                    socket
                        .send(Message::text(
                            task_dispatch
                                .create_request(&id, &config.agent_id, task)
                                .to_string(),
                        ))
                        .map_err(|error| error.to_string())?;
                }
                Ok(GatewayCommand::AbortSession { session_key }) => {
                    let id = next_request_id("abort", &mut request_id);
                    socket
                        .send(Message::text(
                            request(
                                &id,
                                "sessions.abort",
                                json!({ "key": session_key, "clearQueued": true }),
                            )
                            .to_string(),
                        ))
                        .map_err(|error| error.to_string())?;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return Ok(()),
            }
        }

        match socket.read() {
            Ok(Message::Text(text)) => {
                let frame: Value =
                    serde_json::from_str(&text).map_err(|error| error.to_string())?;
                if let Some(follow_up) =
                    task_dispatch.follow_up(&frame, &config.agent_id, &mut request_id)
                {
                    socket
                        .send(Message::text(follow_up.to_string()))
                        .map_err(|error| error.to_string())?;
                }
                dispatch_frame(frame, updates)?;
            }
            Ok(Message::Ping(payload)) => socket
                .send(Message::Pong(payload))
                .map_err(|error| error.to_string())?,
            Ok(Message::Close(_)) => return Ok(()),
            Ok(_) => {}
            Err(WsError::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(WsError::ConnectionClosed | WsError::AlreadyClosed) => return Ok(()),
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn connect_request(config: &GatewayConfig, _nonce: &str) -> Value {
    request(
        "connect-1",
        "connect",
        json!({
            "minProtocol": PROTOCOL_VERSION,
            "maxProtocol": PROTOCOL_VERSION,
            "client": {
                "id": "gateway-client",
                "version": env!("CARGO_PKG_VERSION"),
                "platform": std::env::consts::OS,
                "mode": "backend"
            },
            "role": "operator",
            "scopes": ["operator.read", "operator.write", "operator.approvals"],
            "caps": [],
            "commands": [],
            "permissions": {},
            "auth": { "token": config.token },
            "locale": "en-US",
            "userAgent": concat!("mission-control/", env!("CARGO_PKG_VERSION"))
        }),
    )
}

fn request(id: &str, method: &str, params: Value) -> Value {
    json!({ "type": "req", "id": id, "method": method, "params": params })
}

fn next_request_id(prefix: &str, request_id: &mut u64) -> String {
    let id = format!("{prefix}-{request_id}");
    *request_id += 1;
    id
}

fn session_subscribe_request() -> Value {
    request(
        "sessions-1",
        "sessions.subscribe",
        json!({
            "limit": 60,
            "ownerFirst": true
        }),
    )
}

fn create_task_request(id: &str, agent_id: &str) -> Value {
    request(
        id,
        "sessions.create",
        json!({
            "agentId": agent_id
        }),
    )
}

fn observe_session_request(id: &str, session_key: &str, subscription_id: &str) -> Value {
    request(
        id,
        "sessions.messages.subscribe",
        json!({
            "key": session_key,
            "subscriptionId": format!("mission-control-{subscription_id}")
        }),
    )
}

fn send_task_request(id: &str, agent_id: &str, session_key: &str, task: &str) -> Value {
    request(
        id,
        "sessions.send",
        json!({
            "key": session_key,
            "agentId": agent_id,
            "message": task
        }),
    )
}

fn read_json(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>) -> Result<Value, String> {
    loop {
        match socket.read().map_err(|error| error.to_string())? {
            Message::Text(text) => {
                return serde_json::from_str(&text).map_err(|error| error.to_string());
            }
            Message::Ping(payload) => socket
                .send(Message::Pong(payload))
                .map_err(|error| error.to_string())?,
            Message::Close(_) => return Err("Gateway closed during handshake".into()),
            _ => {}
        }
    }
}

fn ensure_success(frame: &Value, expected_id: &str) -> Result<(), String> {
    if frame.get("type").and_then(Value::as_str) != Some("res")
        || frame.get("id").and_then(Value::as_str) != Some(expected_id)
    {
        return Err(format!("Unexpected Gateway handshake frame: {frame}"));
    }
    if frame.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(frame
            .get("error")
            .map(Value::to_string)
            .unwrap_or_else(|| "Gateway rejected the connection".into()))
    }
}

#[cfg(test)]
fn dispatch(text: &str, updates: &Sender<GatewayUpdate>) -> Result<(), String> {
    let frame: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    dispatch_frame(frame, updates)
}

fn dispatch_frame(frame: Value, updates: &Sender<GatewayUpdate>) -> Result<(), String> {
    match frame.get("type").and_then(Value::as_str) {
        Some("event") => updates
            .send(GatewayUpdate::Event {
                name: frame
                    .get("event")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_string(),
                payload: frame.get("payload").cloned().unwrap_or(Value::Null),
            })
            .map_err(|error| error.to_string()),
        Some("res") if frame.get("ok").and_then(Value::as_bool) == Some(true) => {
            let id = frame.get("id").and_then(Value::as_str).unwrap_or_default();
            let payload = frame.get("payload").cloned().unwrap_or(Value::Null);
            if id == "sessions-1" {
                updates
                    .send(GatewayUpdate::Sessions(payload))
                    .map_err(|error| error.to_string())
            } else {
                updates
                    .send(GatewayUpdate::Response {
                        method: id.to_string(),
                        payload,
                    })
                    .map_err(|error| error.to_string())
            }
        }
        Some("res") => {
            let id = frame.get("id").and_then(Value::as_str).unwrap_or("unknown");
            let detail = frame
                .get("error")
                .map(Value::to_string)
                .unwrap_or_else(|| "OpenClaw RPC failed".into());
            updates
                .send(GatewayUpdate::Error(format!("RPC {id}: {detail}")))
                .map_err(|error| error.to_string())
        }
        _ => Ok(()),
    }
}

fn set_read_timeout(stream: &mut MaybeTlsStream<TcpStream>, timeout: Duration) -> io::Result<()> {
    match stream {
        MaybeTlsStream::Plain(stream) => stream.set_read_timeout(Some(timeout)),
        MaybeTlsStream::Rustls(stream) => stream.sock.set_read_timeout(Some(timeout)),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> GatewayConfig {
        GatewayConfig {
            url: "ws://127.0.0.1:18789".into(),
            token: "secret-value".into(),
            agent_id: "main".into(),
        }
    }

    #[test]
    fn connect_frame_uses_protocol_v4_backend_identity_and_required_scopes() {
        let frame = connect_request(&config(), "challenge-nonce");
        assert_eq!(frame["params"]["minProtocol"], 4);
        assert_eq!(frame["params"]["maxProtocol"], 4);
        assert_eq!(frame["params"]["client"]["id"], "gateway-client");
        assert_eq!(frame["params"]["client"]["mode"], "backend");
        assert_eq!(frame["params"]["auth"]["token"], "secret-value");
        assert!(
            frame["params"]["scopes"]
                .as_array()
                .unwrap()
                .contains(&json!("operator.write"))
        );
    }

    #[test]
    fn session_subscription_matches_gateway_schema() {
        let frame = session_subscribe_request();
        assert_eq!(frame["method"], "sessions.subscribe");
        assert_eq!(frame["params"]["limit"], 60);
        assert_eq!(frame["params"]["ownerFirst"], true);
        assert!(frame["params"].get("includeMessageBodies").is_none());
    }

    #[test]
    fn task_creation_is_empty_until_stream_subscription_is_ready() {
        let frame = create_task_request("create-2", "main");
        assert_eq!(frame["method"], "sessions.create");
        assert_eq!(frame["params"]["agentId"], "main");
        assert!(frame["params"].get("message").is_none());
        assert!(frame["params"].get("idempotencyKey").is_none());
    }

    #[test]
    fn task_dispatch_waits_for_subscription_ack_before_sending_prompt() {
        let mut dispatch = TaskDispatch::default();
        let create = dispatch.create_request("create-2", "main", "Third task".into());
        assert_eq!(create["method"], "sessions.create");
        assert!(create["params"].get("message").is_none());

        let mut request_id = 3;
        let observe = dispatch
            .follow_up(
                &json!({
                    "type":"res",
                    "id":"create-2",
                    "ok":true,
                    "payload":{"key":"agent:main:third"}
                }),
                "main",
                &mut request_id,
            )
            .unwrap();
        assert_eq!(observe["method"], "sessions.messages.subscribe");
        assert_eq!(observe["params"]["key"], "agent:main:third");
        assert_eq!(
            observe["params"]["subscriptionId"],
            "mission-control-create-2"
        );

        let send = dispatch
            .follow_up(
                &json!({
                    "type":"res",
                    "id":"observe-3",
                    "ok":true,
                    "payload":{"subscribed":true}
                }),
                "main",
                &mut request_id,
            )
            .unwrap();
        assert_eq!(send["method"], "sessions.send");
        assert_eq!(send["params"]["key"], "agent:main:third");
        assert_eq!(send["params"]["message"], "Third task");
    }

    #[test]
    fn event_frames_are_forwarded_without_losing_payload() {
        let (sender, receiver) = mpsc::channel();
        dispatch(
            r#"{"type":"event","event":"session.tool","payload":{"tool":"terminal","tokens":12}}"#,
            &sender,
        )
        .unwrap();
        assert_eq!(
            receiver.recv().unwrap(),
            GatewayUpdate::Event {
                name: "session.tool".into(),
                payload: json!({"tool":"terminal","tokens":12})
            }
        );
    }

    #[test]
    fn failed_rpc_becomes_visible_error_update() {
        let (sender, receiver) = mpsc::channel();
        dispatch(
            r#"{"type":"res","id":"send-2","ok":false,"error":{"code":"DENIED","message":"scope missing"}}"#,
            &sender,
        )
        .unwrap();
        let GatewayUpdate::Error(error) = receiver.recv().unwrap() else {
            panic!("expected error update");
        };
        assert!(error.contains("send-2"));
        assert!(error.contains("scope missing"));
    }
}
