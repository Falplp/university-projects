use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskOutcome {
    Success,
    Error,
}

impl TaskOutcome {
    fn label(self) -> &'static str {
        match self {
            Self::Success => "SUCCESS",
            Self::Error => "ERROR",
        }
    }
}

pub fn load_tasks(path: impl AsRef<Path>) -> io::Result<Vec<String>> {
    let raw = fs::read_to_string(path)?;
    Ok(parse_tasks(&raw))
}

pub fn parse_tasks(raw: &str) -> Vec<String> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(ToOwned::to_owned)
        .collect()
}

pub fn begin_report(path: impl AsRef<Path>, total: usize) -> io::Result<()> {
    ensure_parent(path.as_ref())?;
    fs::write(
        path,
        format!(
            "Mission Control OpenClaw batch report\nstarted_unix={}\ntotal_tasks={total}\n\n",
            unix_timestamp()
        ),
    )
}

pub fn append_result(
    path: impl AsRef<Path>,
    id: u64,
    task: &str,
    outcome: TaskOutcome,
    session: Option<&str>,
    detail: &str,
) -> io::Result<()> {
    ensure_parent(path.as_ref())?;
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(
        file,
        "[{}] task_id={id} status={} session={}\ntask={}\ndetail={}\n",
        unix_timestamp(),
        outcome.label(),
        sanitize(session.unwrap_or("not-created")),
        sanitize(task),
        sanitize(detail)
    )
}

pub fn finish_report(
    path: impl AsRef<Path>,
    total: usize,
    success: usize,
    errors: usize,
) -> io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(
        file,
        "SUMMARY total={total} success={success} errors={errors} finished_unix={}",
        unix_timestamp()
    )
}

fn sanitize(value: &str) -> String {
    value.replace(['\n', '\r'], " ").trim().to_string()
}

fn ensure_parent(path: &Path) -> io::Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_ignores_empty_lines_and_comments() {
        let tasks = parse_tasks("# planned work\n\n First task \n  # disabled\nSecond task\n");
        assert_eq!(tasks, ["First task", "Second task"]);
    }

    #[test]
    fn report_contains_success_error_and_final_summary() {
        let path = std::env::temp_dir().join(format!(
            "mission-control-batch-{}-{}.log",
            std::process::id(),
            unix_timestamp()
        ));
        begin_report(&path, 2).unwrap();
        append_result(
            &path,
            1,
            "Create one file",
            TaskOutcome::Success,
            Some("agent:main:one"),
            "completed",
        )
        .unwrap();
        append_result(
            &path,
            2,
            "Fail safely",
            TaskOutcome::Error,
            None,
            "RPC rejected",
        )
        .unwrap();
        finish_report(&path, 2, 1, 1).unwrap();

        let report = fs::read_to_string(&path).unwrap();
        assert!(report.contains("task_id=1 status=SUCCESS"));
        assert!(report.contains("task_id=2 status=ERROR"));
        assert!(report.contains("SUMMARY total=2 success=1 errors=1"));
        fs::remove_file(path).unwrap();
    }
}
