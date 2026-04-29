use anyhow::Result;
use bifrost_shared::usage::UsageRecord;
use chrono::{Datelike, Local, NaiveDate, NaiveTime};
use clap::{Parser, Subcommand};
use colored::Colorize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use tabled::{Table, Tabled};

use super::printing::print_warning;

#[derive(Parser)]
pub struct UsageArgs {
    /// Quick subcommand for monthly usage summary
    #[command(subcommand)]
    pub subcommand: Option<UsageSubcommand>,

    /// Date to show (YYYY-MM-DD), defaults to today
    #[arg(long)]
    pub date: Option<String>,

    /// Start date for range query (YYYY-MM-DD)
    #[arg(long)]
    pub from: Option<String>,

    /// End date for range query (YYYY-MM-DD)
    #[arg(long)]
    pub to: Option<String>,

    /// Filter by time range (e.g., 12:00-16:00)
    #[arg(short, long)]
    pub time_range: Option<String>,

    /// Filter by provider (supports * wildcard, AND relationship)
    #[arg(short, long)]
    pub provider: Option<String>,

    /// Filter by model (supports * wildcard, AND relationship)
    #[arg(short, long)]
    pub model: Option<String>,
}

/// Subcommands for the usage command
#[derive(Subcommand)]
pub enum UsageSubcommand {
    /// Show current month's token usage grouped by provider.
    ///
    /// Month can be a number (1-12) for the current year, or YYYY-MM format.
    /// Defaults to the current month.
    Month {
        /// Month number (1-12) or YYYY-MM format, defaults to current month
        #[arg(index = 1)]
        month: Option<String>,
    },
}

#[derive(Tabled)]
struct MonthRow {
    provider: String,
    requests: String,
    prompt: String,
    completion: String,
    total: String,
}

#[derive(Tabled)]
struct GroupedRow {
    date: String,
    provider: String,
    model: String,
    requests: String,
    prompt: String,
    completion: String,
    total: String,
}

struct UsageRecordWithDate {
    date: String,
    record: UsageRecord,
}

fn get_usage_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".bifrost")
        .join("usage")
}

fn format_tokens(n: u32) -> String {
    if n >= 1_000_000 {
        format!("{:.2}m", n as f64 / 1_000_000.0)
    } else if n >= 1000 {
        format!("{:.2}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
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
    let pattern_parts: Vec<&str> = pattern.split('*').collect();
    if pattern_parts.is_empty() {
        return true;
    }

    let mut pos = 0;
    for (i, part) in pattern_parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !text.starts_with(part) {
                return false;
            }
            pos = part.len();
        } else if let Some(idx) = text[pos..].find(part) {
            pos += idx + part.len();
        } else {
            return false;
        }
    }
    true
}

fn matches_filters(
    record: &UsageRecord,
    provider: &Option<String>,
    model: &Option<String>,
    time_range: &Option<(NaiveTime, NaiveTime)>,
) -> bool {
    #[expect(clippy::collapsible_if)]
    if let Some(p) = provider {
        if !matches_wildcard(&record.provider, p) {
            return false;
        }
    }
    #[expect(clippy::collapsible_if)]
    if let Some(m) = model {
        if !matches_wildcard(&record.model, m) {
            return false;
        }
    }
    #[expect(clippy::collapsible_if)]
    if let Some((start, end)) = time_range {
        if let Ok(time) = NaiveTime::parse_from_str(&record.time, "%H:%M:%S") {
            if time < *start || time > *end {
                return false;
            }
        }
    }
    true
}

fn read_records_for_range(from: &NaiveDate, to: &NaiveDate) -> Result<Vec<UsageRecordWithDate>> {
    let dir = get_usage_dir();
    let mut all_records = Vec::new();

    let mut current = *from;
    while current <= *to {
        let date_str = current.format("%Y-%m-%d").to_string();
        let path = dir.join(format!("{}.jsonl", date_str));

        if path.exists() {
            let file = File::open(&path)?;
            let reader = BufReader::new(file);
            for line in reader.lines() {
                if let Ok(record) = serde_json::from_str::<UsageRecord>(&line?) {
                    all_records.push(UsageRecordWithDate {
                        date: date_str.clone(),
                        record,
                    });
                }
            }
        }
        current += chrono::Duration::days(1);
    }

    Ok(all_records)
}

fn cmd_month(month_str: Option<&str>) -> Result<()> {
    let today = Local::now().date_naive();
    let (year, month_num) = if let Some(m) = month_str {
        if let Some((y_str, mo_str)) = m.split_once('-') {
            // YYYY-MM format
            let y: i32 = y_str
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid year in month: {m}"))?;
            let mo: u32 = mo_str
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid month in: {m}"))?;
            if !(1..=12).contains(&mo) {
                anyhow::bail!("Invalid month value: {mo}. Must be 1-12");
            }
            (y, mo)
        } else {
            // Bare number: month of current year
            let mo: u32 = m.parse().map_err(|_| {
                anyhow::anyhow!("Invalid month: {m}. Use a month number (1-12) or YYYY-MM format")
            })?;
            if !(1..=12).contains(&mo) {
                anyhow::bail!("Invalid month value: {mo}. Must be 1-12");
            }
            (today.year(), mo)
        }
    } else {
        (today.year(), today.month())
    };

    let first_day = NaiveDate::from_ymd_opt(year, month_num, 1)
        .ok_or_else(|| anyhow::anyhow!("Invalid date: {year}-{month_num:02}"))?;

    // If viewing current month, end at today; otherwise end at last day of month
    let last_day = if year == today.year() && month_num == today.month() {
        today
    } else {
        let next_month = if month_num == 12 {
            NaiveDate::from_ymd_opt(year + 1, 1, 1)
                .ok_or_else(|| anyhow::anyhow!("Invalid date"))?
        } else {
            NaiveDate::from_ymd_opt(year, month_num + 1, 1)
                .ok_or_else(|| anyhow::anyhow!("Invalid date"))?
        };
        next_month - chrono::Duration::days(1)
    };

    let records = read_records_for_range(&first_day, &last_day)?;

    if records.is_empty() {
        print_warning("No usage records found for this month");
        return Ok(());
    }

    use std::collections::BTreeMap;
    type StatsVal = (u32, u32, u32);

    let mut by_provider: BTreeMap<String, StatsVal> = BTreeMap::new();
    for r in &records {
        let entry = by_provider
            .entry(r.record.provider.clone())
            .or_insert((0, 0, 0));
        entry.0 += 1;
        entry.1 += r.record.prompt_tokens;
        entry.2 += r.record.completion_tokens;
    }

    let mut rows: Vec<MonthRow> = Vec::new();
    for (provider, (requests, prompt, completion)) in by_provider {
        let total = prompt + completion;
        rows.push(MonthRow {
            provider,
            requests: requests.to_string(),
            prompt: format_tokens(prompt),
            completion: format_tokens(completion),
            total: format_tokens(total),
        });
    }

    let month_label = format!("{year}-{month_num:02}");
    println!(
        "\n{}",
        format!("Monthly Usage - {}", month_label)
            .bold()
            .white()
            .on_green()
    );

    let mut table = Table::new(&rows);
    use tabled::settings::Style;
    table.with(Style::rounded());
    println!("{}", table);

    let total_requests = records.len();
    let total_prompt: u32 = records.iter().map(|r| r.record.prompt_tokens).sum();
    let total_completion: u32 = records.iter().map(|r| r.record.completion_tokens).sum();
    let total_tokens = total_prompt + total_completion;

    println!();
    println!(
        "Total: {} {} | {} {} | {} {} | {} {}",
        total_requests.to_string().bold().green(),
        "requests".bold(),
        format_tokens(total_prompt).bold().cyan(),
        "prompt".bold(),
        format_tokens(total_completion).bold().yellow(),
        "completion".bold(),
        format_tokens(total_tokens).bold().magenta(),
        "total".bold()
    );
    println!();

    Ok(())
}

pub fn cmd_usage(args: UsageArgs) -> Result<()> {
    if let Some(UsageSubcommand::Month { month }) = &args.subcommand {
        return cmd_month(month.as_deref());
    }

    let time_range = args.time_range.as_ref().and_then(|s| parse_time_range(s));

    let (records, date_label) = if let (Some(from_str), Some(to_str)) = (&args.from, &args.to) {
        let from = NaiveDate::parse_from_str(from_str, "%Y-%m-%d")
            .map_err(|_| anyhow::anyhow!("Invalid from date format: {}", from_str))?;
        let to = NaiveDate::parse_from_str(to_str, "%Y-%m-%d")
            .map_err(|_| anyhow::anyhow!("Invalid to date format: {}", to_str))?;
        let records = read_records_for_range(&from, &to)?;
        let label = if from == to {
            from.format("%Y-%m-%d").to_string()
        } else {
            format!("{} to {}", from.format("%Y-%m-%d"), to.format("%Y-%m-%d"))
        };
        (records, label)
    } else {
        let date = args
            .date
            .unwrap_or_else(|| Local::now().format("%Y-%m-%d").to_string());
        let records = read_records_for_range(
            &NaiveDate::parse_from_str(&date, "%Y-%m-%d")
                .unwrap_or_else(|_| Local::now().date_naive()),
            &NaiveDate::parse_from_str(&date, "%Y-%m-%d")
                .unwrap_or_else(|_| Local::now().date_naive()),
        )?;
        (records, date)
    };

    let filtered: Vec<UsageRecordWithDate> = records
        .into_iter()
        .filter(|r| matches_filters(&r.record, &args.provider, &args.model, &time_range))
        .collect();

    if filtered.is_empty() {
        print_warning("No matching usage records found");
        return Ok(());
    }

    let is_single_day = args.from.is_none() && args.to.is_none();

    println!(
        "\n{}",
        format!("Usage Records - {}", date_label)
            .bold()
            .white()
            .on_green()
    );

    if is_single_day {
        use std::collections::BTreeMap;
        type SlotKey = u32;
        type ProviderKey = String;
        type ModelKey = String;
        type StatsVal = (u32, u32, u32);
        let mut by_slot: BTreeMap<SlotKey, BTreeMap<ProviderKey, BTreeMap<ModelKey, StatsVal>>> =
            BTreeMap::new();

        for r in &filtered {
            let hour: u32 = r.record.time[..2].parse().unwrap_or(0);
            let slot = (hour / 5) * 5;
            let entry = by_slot
                .entry(slot)
                .or_default()
                .entry(r.record.provider.clone())
                .or_default()
                .entry(r.record.model.clone())
                .or_insert((0, 0, 0));
            entry.0 += 1;
            entry.1 += r.record.prompt_tokens;
            entry.2 += r.record.completion_tokens;
        }

        let mut rows: Vec<GroupedRow> = Vec::new();
        for (slot, providers) in by_slot {
            for (provider, models) in providers {
                for (model, (requests, prompt, completion)) in models {
                    let total = prompt + completion;
                    rows.push(GroupedRow {
                        date: format!("{:02}:00-{:02}:59", slot, slot + 4),
                        provider: provider.clone(),
                        model,
                        requests: requests.to_string(),
                        prompt: format_tokens(prompt),
                        completion: format_tokens(completion),
                        total: format_tokens(total),
                    });
                }
            }
        }

        let mut table = Table::new(&rows);
        {
            use tabled::settings::Alignment;
            use tabled::settings::Modify;
            use tabled::settings::object::Cell;
            use tabled::settings::span::RowSpan;

            let mut i = 0;
            while i < rows.len() {
                let mut span = 1;
                while i + span < rows.len() && rows[i + span].date == rows[i].date {
                    span += 1;
                }
                if span > 1 {
                    table.with(
                        Modify::new(Cell::new(i + 1, 0))
                            .with(RowSpan::new(span as isize))
                            .with(Alignment::center())
                            .with(Alignment::center_vertical()),
                    );
                }
                i += span;
            }

            let mut i = 0;
            while i < rows.len() {
                let mut span = 1;
                while i + span < rows.len()
                    && rows[i + span].date == rows[i].date
                    && rows[i + span].provider == rows[i].provider
                {
                    span += 1;
                }
                if span > 1 {
                    table.with(
                        Modify::new(Cell::new(i + 1, 1))
                            .with(RowSpan::new(span as isize))
                            .with(Alignment::center())
                            .with(Alignment::center_vertical()),
                    );
                }
                i += span;
            }
        }
        println!("{}", table);

        let total_requests = filtered.len();
        let total_prompt: u32 = filtered.iter().map(|r| r.record.prompt_tokens).sum();
        let total_completion: u32 = filtered.iter().map(|r| r.record.completion_tokens).sum();
        let total_tokens = total_prompt + total_completion;

        println!();
        println!(
            "Total: {} {} | {} {} | {} {} | {} {}",
            total_requests.to_string().bold().green(),
            "requests".bold(),
            format_tokens(total_prompt).bold().cyan(),
            "prompt".bold(),
            format_tokens(total_completion).bold().yellow(),
            "completion".bold(),
            format_tokens(total_tokens).bold().magenta(),
            "total".bold()
        );
    } else {
        use std::collections::BTreeMap;
        type DateKey = String;
        type ProviderKey = String;
        type ModelKey = String;
        type StatsVal = (u32, u32, u32);

        let mut by_date_provider_model: BTreeMap<
            DateKey,
            BTreeMap<ProviderKey, BTreeMap<ModelKey, StatsVal>>,
        > = BTreeMap::new();

        for r in &filtered {
            let entry = by_date_provider_model
                .entry(r.date.clone())
                .or_default()
                .entry(r.record.provider.clone())
                .or_default()
                .entry(r.record.model.clone())
                .or_insert((0, 0, 0));
            entry.0 += 1;
            entry.1 += r.record.prompt_tokens;
            entry.2 += r.record.completion_tokens;
        }

        let mut rows: Vec<GroupedRow> = Vec::new();
        for (date, providers) in by_date_provider_model {
            for (provider, models) in providers {
                for (model, (requests, prompt, completion)) in models {
                    let total = prompt + completion;
                    rows.push(GroupedRow {
                        date: date.clone(),
                        provider: provider.clone(),
                        model,
                        requests: requests.to_string(),
                        prompt: format_tokens(prompt),
                        completion: format_tokens(completion),
                        total: format_tokens(total),
                    });
                }
            }
        }

        let mut table = Table::new(&rows);
        {
            use tabled::settings::Alignment;
            use tabled::settings::Modify;
            use tabled::settings::object::Cell;
            use tabled::settings::span::RowSpan;

            let mut i = 0;
            while i < rows.len() {
                let mut span = 1;
                while i + span < rows.len() && rows[i + span].date == rows[i].date {
                    span += 1;
                }
                if span > 1 {
                    table.with(
                        Modify::new(Cell::new(i + 1, 0))
                            .with(RowSpan::new(span as isize))
                            .with(Alignment::center())
                            .with(Alignment::center_vertical()),
                    );
                }
                i += span;
            }

            let mut i = 0;
            while i < rows.len() {
                let mut span = 1;
                while i + span < rows.len()
                    && rows[i + span].date == rows[i].date
                    && rows[i + span].provider == rows[i].provider
                {
                    span += 1;
                }
                if span > 1 {
                    table.with(
                        Modify::new(Cell::new(i + 1, 1))
                            .with(RowSpan::new(span as isize))
                            .with(Alignment::center())
                            .with(Alignment::center_vertical()),
                    );
                }
                i += span;
            }
        }
        println!("{}", table);

        let total_requests = filtered.len();
        let total_prompt: u32 = filtered.iter().map(|r| r.record.prompt_tokens).sum();
        let total_completion: u32 = filtered.iter().map(|r| r.record.completion_tokens).sum();
        let total_tokens = total_prompt + total_completion;

        println!();
        println!(
            "Total: {} {} | {} {} | {} {} | {} {}",
            total_requests.to_string().bold().green(),
            "requests".bold(),
            format_tokens(total_prompt).bold().cyan(),
            "prompt".bold(),
            format_tokens(total_completion).bold().yellow(),
            "completion".bold(),
            format_tokens(total_tokens).bold().magenta(),
            "total".bold()
        );
    }
    println!();

    Ok(())
}
