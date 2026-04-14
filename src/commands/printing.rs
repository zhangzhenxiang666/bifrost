use colored::Colorize;
use tabled::{
    Table,
    settings::{
        Alignment, Remove, Style,
        object::{Columns, Rows},
    },
};

pub fn print_info(label: &str, value: &str) {
    println!("{:<18} {}", label.bold().cyan(), value);
}

pub fn print_success(message: &str) {
    println!("{} {}", "✓".green().bold(), message.green());
}

pub fn print_error(message: &str) {
    eprintln!("{} {}", "✗".red().bold(), message.red());
}

pub fn print_warning(message: &str) {
    println!("{} {}", "⚠".yellow().bold(), message.yellow());
}

pub fn print_header(title: &str) {
    println!("\n{}", title.bold().white().on_purple());
}

pub fn print_kv_table(rows: &[(&str, String)]) {
    let mut table = Table::new(rows);
    table
        .with(Style::modern())
        .modify(Columns::first(), Alignment::left())
        .modify(Columns::last(), Alignment::left());
    table.with(Remove::row(Rows::new(0..1)));
    println!("{}", table);
}

pub fn print_process_table(
    pid: u32,
    name: &str,
    memory: f32,
    cpu: f32,
    port: Option<u16>,
    proxy: Option<&str>,
) {
    let mut rows = vec![
        ("PID", pid.to_string()),
        ("Process", name.to_string()),
        ("Memory", format!("{:.2} MB", memory)),
        ("CPU", format!("{:.1}%", cpu)),
    ];
    if let Some(port) = port {
        rows.push(("Port", port.to_string()));
    }
    if let Some(proxy) = proxy {
        rows.push(("Proxy", proxy.to_string()));
    } else {
        rows.push(("Proxy", "None".to_string()));
    }
    print_kv_table(&rows);
}
