use anyhow::Result;
use chrono::{Local, NaiveDate, NaiveTime};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek};
use std::path::PathBuf;
use tabled::{Table, Tabled};

use super::printing::print_warning;
use colored::Colorize;

#[derive(Parser)]
pub struct LogArgs {
    /// Date to show (YYYY-MM-DD), defaults to today
    #[arg(long)]
    pub date: Option<String>,

    /// Filter by time range (e.g., 12:00-16:00)
    #[arg(short, long)]
    pub time_range: Option<String>,

    /// Filter by log level (supports * wildcard)
    #[arg(short, long)]
    pub level: Option<String>,

    /// Number of log lines to display
    #[arg(long, default_value = "30")]
    pub lines: usize,

    /// Follow new logs continuously
    #[arg(long)]
    pub tail: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LogRecord {
    pub timestamp: String,
    pub level: String,
    pub fields: serde_json::Value,
    #[serde(default)]
    pub span: Option<SpanInfo>,
    #[serde(default)]
    pub spans: Vec<SpanInfo>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SpanInfo {
    pub request_id: String,
    pub name: String,
}

impl LogRecord {
    fn get_request_id(&self) -> Option<&str> {
        self.span
            .as_ref()
            .map(|s| s.request_id.as_str())
            .or_else(|| self.spans.first().map(|s| s.request_id.as_str()))
    }
}

fn get_log_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".bifrost")
        .join("logs")
}

fn parse_time_range(s: &str) -> Option<(NaiveTime, NaiveTime)> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 2 {
        return None;
    }
    let start = NaiveTime::parse_from_str(parts[0], "%H:%M").ok()?;
    let end = NaiveTime::parse_from_str(parts[1], "%H:%M").ok()?;
    Some((start, end))
}

fn matches_wildcard(text: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let text_lower = text.to_lowercase();
    let pattern_lower = pattern.to_lowercase();
    let pattern_parts: Vec<&str> = pattern_lower.split('*').collect();
    if pattern_parts.is_empty() {
        return true;
    }

    let mut pos = 0;
    for (i, part) in pattern_parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !text_lower.starts_with(part) {
                return false;
            }
            pos = part.len();
        } else if let Some(idx) = text_lower[pos..].find(part) {
            pos += idx + part.len();
        } else {
            return false;
        }
    }
    true
}

fn matches_filters(
    record: &LogRecord,
    level: &Option<String>,
    time_range: &Option<(NaiveTime, NaiveTime)>,
) -> bool {
    if let Some(l) = level
        && !matches_wildcard(&record.level, l)
    {
        return false;
    }

    if let Some((start, end)) = time_range
        && let Ok(time) = NaiveTime::parse_from_str(&record.timestamp[11..19], "%H:%M:%S")
        && (time < *start || time > *end)
    {
        return false;
    }
    true
}

fn read_logs_for_date(date: &NaiveDate) -> Result<Vec<LogRecord>> {
    let dir = get_log_dir();
    let date_str = date.format("%Y-%m-%d").to_string();
    let path = dir.join(format!("{}.log", date_str));

    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = File::open(&path)?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();

    for line in reader.lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if !trimmed.starts_with('{') {
            continue;
        }
        if let Ok(record) = serde_json::from_str::<LogRecord>(&line) {
            records.push(record);
        }
    }

    Ok(records)
}

fn read_logs_from_path(path: &PathBuf, start_pos: usize) -> Result<(Vec<LogRecord>, usize)> {
    let metadata = std::fs::metadata(path)?;
    let file_len = metadata.len() as usize;

    if start_pos >= file_len {
        return Ok((Vec::new(), start_pos));
    }

    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    reader.seek(std::io::SeekFrom::Start(start_pos as u64))?;

    let mut records = Vec::new();
    for line in reader.lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if !trimmed.starts_with('{') {
            continue;
        }
        if let Ok(record) = serde_json::from_str::<LogRecord>(trimmed) {
            records.push(record);
        }
    }

    Ok((records, file_len))
}

#[derive(Tabled, Clone)]
struct LogRow {
    time: String,
    level: String,
    message: String,
}

struct SpanInfo2 {
    start: usize,
    span: usize,
}

fn format_message(level: &str, fields: &serde_json::Value) -> String {
    if let Some(msg) = fields.get("message").and_then(|v| v.as_str()) {
        return msg.to_string();
    }
    if let Some(msg) = fields.get("msg").and_then(|v| v.as_str()) {
        return msg.to_string();
    }
    let log_type = fields.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match log_type {
        "loggger-middleware" => {
            let client_ip = fields
                .get("client_ip")
                .and_then(|v| v.as_str())
                .unwrap_or("-");
            let port = fields.get("port").and_then(|v| v.as_u64()).unwrap_or(0);
            let path = fields.get("path").and_then(|v| v.as_str()).unwrap_or("-");
            let status = fields.get("status").and_then(|v| v.as_str()).unwrap_or("-");
            let duration = fields
                .get("duration_ms")
                .and_then(|v| v.as_str())
                .unwrap_or("-");
            let body = fields.get("body").and_then(|v| v.as_str()).unwrap_or("");
            if level == "ERROR" && !body.is_empty() {
                return format!(
                    "{}:{} {} {} ({}ms) | {}",
                    client_ip, port, path, status, duration, body
                );
            }
            format!(
                "{}:{} {} {} ({}ms)",
                client_ip, port, path, status, duration
            )
        }
        "handler" => {
            let url = fields.get("url").and_then(|v| v.as_str()).unwrap_or("-");
            let model = fields.get("model").and_then(|v| v.as_str()).unwrap_or("");
            if !model.is_empty() {
                return format!("→ {} [{}]", url, model);
            }
            format!("→ {}", url)
        }
        "send-request" => {
            let msg = fields.get("msg").and_then(|v| v.as_str()).unwrap_or("-");
            msg.to_string()
        }
        _ => serde_json::to_string(fields).unwrap_or_default(),
    }
}

pub fn cmd_log(args: LogArgs) -> Result<()> {
    let time_range = args.time_range.as_ref().and_then(|s| parse_time_range(s));

    let date = args
        .date
        .unwrap_or_else(|| Local::now().format("%Y-%m-%d").to_string());

    let date_naive =
        NaiveDate::parse_from_str(&date, "%Y-%m-%d").unwrap_or_else(|_| Local::now().date_naive());

    let records = read_logs_for_date(&date_naive)?;

    let filtered: Vec<&LogRecord> = records
        .iter()
        .filter(|r| matches_filters(r, &args.level, &time_range))
        .collect();

    if filtered.is_empty() {
        print_warning("No matching log records found");
        return Ok(());
    }

    let display_count = args.lines.min(filtered.len());
    let mut display_slice: Vec<&LogRecord> = if args.tail {
        filtered
    } else {
        filtered[filtered.len() - display_count..].to_vec()
    };

    if args.tail {
        return cmd_log_tail(date_naive, time_range, args.level.clone());
    }

    display_slice.sort_by(|a, b| {
        match (
            chrono::DateTime::parse_from_rfc3339(&a.timestamp),
            chrono::DateTime::parse_from_rfc3339(&b.timestamp),
        ) {
            (Ok(ta), Ok(tb)) => ta.cmp(&tb),
            _ => a.timestamp.cmp(&b.timestamp),
        }
    });

    let mut request_groups: BTreeMap<String, Vec<&LogRecord>> = BTreeMap::new();
    for record in &display_slice {
        if let Some(req_id) = record.get_request_id() {
            request_groups
                .entry(req_id.to_string())
                .or_default()
                .push(record);
        }
    }

    let mut processed_requests: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    let mut rows: Vec<LogRow> = Vec::new();
    let mut spans: Vec<SpanInfo2> = Vec::new();
    let mut current_pos = 0;

    for record in display_slice {
        let req_id = record.get_request_id();

        if req_id.is_none() {
            rows.push(LogRow {
                time: record.timestamp[11..19].to_string(),
                level: record.level.clone(),
                message: format_message(&record.level, &record.fields),
            });
            spans.push(SpanInfo2 {
                start: current_pos,
                span: 1,
            });
            current_pos += 1;
            continue;
        }

        let req_id_str = req_id.unwrap();
        if processed_requests.contains(req_id_str) {
            continue;
        }
        processed_requests.insert(req_id_str.to_string());

        let records = match request_groups.get(req_id_str) {
            Some(r) => r,
            None => continue,
        };

        let mut by_level: BTreeMap<String, Vec<&LogRecord>> = BTreeMap::new();
        for record in records {
            by_level
                .entry(record.level.clone())
                .or_default()
                .push(record);
        }

        for level_records in by_level.values() {
            let first_time = level_records
                .first()
                .map(|r| r.timestamp[11..19].to_string())
                .unwrap_or_default();
            let span = level_records.len();
            for (i, record) in level_records.iter().enumerate() {
                rows.push(LogRow {
                    time: if i == 0 {
                        first_time.clone()
                    } else {
                        String::new()
                    },
                    level: if i == 0 {
                        record.level.clone()
                    } else {
                        String::new()
                    },
                    message: format_message(&record.level, &record.fields),
                });
            }
            spans.push(SpanInfo2 {
                start: current_pos,
                span,
            });
            current_pos += span;
        }
    }

    if rows.is_empty() {
        print_warning("No matching log records found");
        return Ok(());
    }

    println!(
        "\n{}",
        format!("Log Records - {}", date).bold().white().on_green()
    );

    if !args.tail {
        println!("(Showing {} most recent logs)\n", rows.len());
    } else {
        println!("(Following new logs - Ctrl+C to stop)\n");
    }

    // 终端宽度 → message 列折行宽度
    let term_width = terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(120);
    let msg_col_width = term_width.saturating_sub(32).max(20);

    let mut table = Table::new(&rows);

    // 先应用 RowSpan（必须早于 Width::wrap）
    {
        use tabled::settings::Alignment;
        use tabled::settings::Modify;
        use tabled::settings::object::Cell;
        use tabled::settings::span::RowSpan;

        for span_info in &spans {
            if span_info.span > 1 {
                for col in [0usize, 1] {
                    table.with(
                        Modify::new(Cell::new(span_info.start + 1, col))
                            .with(RowSpan::new(span_info.span as isize))
                            .with(Alignment::center())
                            .with(Alignment::center_vertical()),
                    );
                }
            }
        }
    }

    // message 列自动折行
    {
        use tabled::settings::Modify;
        use tabled::settings::Width;
        use tabled::settings::object::Columns;

        table.with(Modify::new(Columns::one(2)).with(Width::wrap(msg_col_width)));
    }

    // message 列标题居中
    {
        use tabled::settings::Alignment;
        use tabled::settings::Modify;
        use tabled::settings::object::Cell;

        table.with(Modify::new(Cell::new(0, 2)).with(Alignment::center()));
    }

    println!("{}", table);

    Ok(())
}

fn cmd_log_tail(
    date_naive: NaiveDate,
    time_range: Option<(NaiveTime, NaiveTime)>,
    level_filter: Option<String>,
) -> Result<()> {
    let dir = get_log_dir();
    let date_str = date_naive.format("%Y-%m-%d").to_string();
    let log_path = dir.join(format!("{}.log", date_str));

    let mut file_pos = std::fs::metadata(&log_path)
        .map(|m| m.len() as usize)
        .unwrap_or(0);

    println!(
        "\n{}",
        format!("Log Records - {} (Tail)", date_str)
            .bold()
            .white()
            .on_green()
    );
    println!("(Following new logs - Ctrl+C to stop)\n");

    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));

        let (new_records, new_pos) = match read_logs_from_path(&log_path, file_pos) {
            Ok((r, p)) => (r, p),
            Err(_) => break,
        };

        if new_records.is_empty() {
            file_pos = new_pos;
            continue;
        }

        file_pos = new_pos;

        let filtered: Vec<&LogRecord> = new_records
            .iter()
            .filter(|r| matches_filters(r, &level_filter, &time_range))
            .collect();

        if filtered.is_empty() {
            continue;
        }

        print_log_lines(&filtered);
    }

    Ok(())
}

fn print_log_lines(records: &[&LogRecord]) {
    for record in records {
        let time = &record.timestamp[11..16];
        let level = &record.level;
        let msg = format_message(&record.level, &record.fields);
        let request_id = record.get_request_id();
        if let Some(id) = request_id {
            println!(
                "{} {} {} [{}]",
                time.yellow(),
                level.cyan(),
                msg,
                id.dimmed()
            );
        } else {
            println!("{} {} {}", time.yellow(), level.cyan(), msg);
        }
    }
}
