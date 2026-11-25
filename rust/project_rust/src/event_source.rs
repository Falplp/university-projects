use serde::Deserialize;
use std::{fs, io, path::Path};

#[derive(Debug, Clone, Deserialize)]
pub struct AgentEvent {
    pub timestamp: String,
    pub agent: String,
    pub kind: String,
    pub message: String,
    #[serde(default)]
    pub tokens: u32,
    #[serde(default)]
    pub duration_ms: u64,
}

pub fn parse_json_line(line: &str) -> Result<AgentEvent, serde_json::Error> {
    serde_json::from_str(line)
}

#[derive(Debug, Default)]
pub struct EventBatch {
    pub events: Vec<AgentEvent>,
    pub malformed: Vec<String>,
}

pub fn read_new_events(path: impl AsRef<Path>, cursor: &mut usize) -> io::Result<EventBatch> {
    let raw = fs::read_to_string(path)?;
    let lines = raw.lines().collect::<Vec<_>>();
    if *cursor > lines.len() {
        *cursor = 0;
    }
    let mut batch = EventBatch::default();
    for (index, line) in lines.iter().enumerate().skip(*cursor) {
        if line.trim().is_empty() {
            continue;
        }
        match parse_json_line(line) {
            Ok(event) => batch.events.push(event),
            Err(error) => batch
                .malformed
                .push(format!("JSONL line {}: {error}", index + 1)),
        }
    }
    *cursor = lines.len();
    Ok(batch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn parses_mcp_event() {
        let event = parse_json_line(r#"{"timestamp":"14:35:01","agent":"code-generator","kind":"tool","message":"filesystem.write src/main.py","tokens":12,"duration_ms":240}"#).unwrap();
        assert_eq!(event.agent, "code-generator");
        assert_eq!(event.tokens, 12);
        assert_eq!(event.kind, "tool");
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(parse_json_line("{not-json}").is_err());
    }

    #[test]
    fn defaults_optional_metrics_to_zero() {
        let event = parse_json_line(
            r#"{"timestamp":"14:35:01","agent":"reviewer","kind":"llm","message":"started"}"#,
        )
        .unwrap();
        assert_eq!(event.tokens, 0);
        assert_eq!(event.duration_ms, 0);
    }

    #[test]
    fn live_reader_only_returns_lines_appended_since_last_poll() {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("mission-control-events-{id}.jsonl"));
        fs::write(
            &path,
            r#"{"timestamp":"10:00:00","agent":"a","kind":"llm","message":"first"}
"#,
        )
        .unwrap();
        let mut cursor = 0;
        let first = read_new_events(&path, &mut cursor).unwrap();
        assert_eq!(first.events.len(), 1);
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"10:00:01","agent":"b","kind":"tool","message":"second"}}"#
        )
        .unwrap();
        let second = read_new_events(&path, &mut cursor).unwrap();
        assert_eq!(second.events.len(), 1);
        assert_eq!(second.events[0].message, "second");
        assert!(
            read_new_events(&path, &mut cursor)
                .unwrap()
                .events
                .is_empty()
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn live_reader_reports_malformed_lines_without_losing_later_events() {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("mission-control-bad-events-{id}.jsonl"));
        fs::write(&path, "bad-json\n{\"timestamp\":\"10:00:01\",\"agent\":\"a\",\"kind\":\"llm\",\"message\":\"ok\"}\n").unwrap();
        let mut cursor = 0;
        let batch = read_new_events(&path, &mut cursor).unwrap();
        assert_eq!(batch.malformed.len(), 1);
        assert_eq!(batch.events.len(), 1);
        fs::remove_file(path).unwrap();
    }
}
