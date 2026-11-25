use clap::Parser;
use serde::Serialize;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Parser)]
#[command(about = "A parallel grep implementation")]
struct Args {
    #[arg(help = "Directory to search recursively")]
    directory: PathBuf,
    #[arg(help = "Text to search for")]
    pattern: String,
    #[arg(short = 'j', long, default_value_t = 4)]
    threads: usize,
    #[arg(short, long, default_value = "matches.txt")]
    output: PathBuf,
    #[arg(long, default_value = "matches.json")]
    json_output: PathBuf,
}

#[derive(Debug, Serialize)]
struct Match {
    path: PathBuf,
    line_number: usize,
    line: String,
}

enum Event {
    FilesDiscovered(usize),
    FileScanned { matches: Vec<Match> },
    WorkerFinished,
    CrawlFailed(String),
}

fn crawl(directory: &Path, files: &Sender<PathBuf>, events: &Sender<Event>) -> io::Result<usize> {
    let mut count = 0;
    for entry in fs::read_dir(directory)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(kind) => kind,
            Err(_) => continue,
        };
        if file_type.is_dir() {
            count += crawl(&path, files, events)?;
        } else if file_type.is_file() {
            files
                .send(path)
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "worker channel closed"))?;
            count += 1;
            if count % 1_000 == 0 {
                let _ = events.send(Event::FilesDiscovered(count));
            }
        }
    }
    Ok(count)
}

fn scan_file(path: &Path, pattern: &str) -> Vec<Match> {
    let mut bytes = Vec::new();
    if File::open(path)
        .and_then(|mut file| file.read_to_end(&mut bytes))
        .is_err()
    {
        return Vec::new();
    }
    String::from_utf8_lossy(&bytes)
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains(pattern))
        .map(|(index, line)| Match {
            path: path.to_path_buf(),
            line_number: index + 1,
            line: line.to_owned(),
        })
        .collect()
}

fn worker(paths: Arc<Mutex<Receiver<PathBuf>>>, events: Sender<Event>, pattern: String) {
    loop {
        let path = { paths.lock().expect("path receiver mutex poisoned").recv() };
        let Ok(path) = path else { break };
        let found = scan_file(&path, &pattern);
        if events.send(Event::FileScanned { matches: found }).is_err() {
            return;
        }
    }
    let _ = events.send(Event::WorkerFinished);
}

fn print_progress(scanned: usize, discovered: usize, matches: usize) {
    const WIDTH: usize = 30;
    let filled = if discovered == 0 {
        0
    } else {
        scanned.saturating_mul(WIDTH) / discovered
    };
    let bar = format!("{}{}", "#".repeat(filled), "-".repeat(WIDTH - filled));
    if discovered == 0 {
        print!("\r[{bar}] scanned {scanned} files, {matches} matches");
    } else {
        print!("\r[{bar}] scanned {scanned}/{discovered} files, {matches} matches");
    }
    let _ = io::stdout().flush();
}

fn write_text(path: &Path, matches: &[Match]) -> io::Result<()> {
    let mut output = BufWriter::new(File::create(path)?);
    for item in matches {
        writeln!(
            output,
            "{}:{}:{}",
            item.path.display(),
            item.line_number,
            item.line
        )?;
    }
    output.flush()
}

fn write_json(path: &Path, matches: &[Match]) -> io::Result<()> {
    let output = BufWriter::new(File::create(path)?);
    serde_json::to_writer_pretty(output, matches).map_err(io::Error::other)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if args.threads == 0 {
        return Err("--threads must be greater than zero".into());
    }
    if !args.directory.is_dir() {
        return Err(format!("not a directory: {}", args.directory.display()).into());
    }
    let started = Instant::now();
    let (paths_tx, paths_rx) = mpsc::channel();
    let paths_rx = Arc::new(Mutex::new(paths_rx));
    let (events_tx, events_rx) = mpsc::channel();

    let crawler_events = events_tx.clone();
    let crawler_directory = args.directory.clone();
    let crawler =
        thread::spawn(
            move || match crawl(&crawler_directory, &paths_tx, &crawler_events) {
                Ok(total) => {
                    let _ = crawler_events.send(Event::FilesDiscovered(total));
                }
                Err(error) => {
                    let _ = crawler_events.send(Event::CrawlFailed(error.to_string()));
                }
            },
        );

    let mut workers = Vec::with_capacity(args.threads);
    for _ in 0..args.threads {
        let worker_paths = Arc::clone(&paths_rx);
        let worker_events = events_tx.clone();
        let pattern = args.pattern.clone();
        workers.push(thread::spawn(move || {
            worker(worker_paths, worker_events, pattern)
        }));
    }
    drop(paths_rx);
    drop(events_tx);

    let mut discovered = 0;
    let mut scanned = 0;
    let mut matches = Vec::new();
    let mut finished_workers = 0;
    let mut crawl_error = None;
    let mut last_progress = Instant::now();
    while finished_workers < args.threads {
        match events_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Event::FilesDiscovered(total)) => discovered = discovered.max(total),
            Ok(Event::FileScanned { matches: found }) => {
                scanned += 1;
                matches.extend(found);
            }
            Ok(Event::WorkerFinished) => finished_workers += 1,
            Ok(Event::CrawlFailed(error)) => crawl_error = Some(error),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if last_progress.elapsed() >= Duration::from_millis(250) {
            print_progress(scanned, discovered, matches.len());
            last_progress = Instant::now();
        }
    }
    crawler.join().map_err(|_| "crawler thread panicked")?;
    for worker in workers {
        worker.join().map_err(|_| "worker thread panicked")?;
    }
    if let Some(error) = crawl_error {
        eprintln!("crawl warning: {error}");
    }
    matches.sort_unstable_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.line_number.cmp(&right.line_number))
    });
    print_progress(scanned, discovered, matches.len());
    println!();
    write_text(&args.output, &matches)?;
    write_json(&args.json_output, &matches)?;
    println!("scanned files: {scanned}");
    println!("matched lines: {}", matches.len());
    println!("elapsed: {:.3}s", started.elapsed().as_secs_f64());
    println!("text output: {}", args.output.display());
    println!("JSON output: {}", args.json_output.display());
    Ok(())
}
