use anyhow::Result;
use crate::cache::sqlite::SqliteCache;
use crate::cache::CacheBackend;

pub fn print_stats(db_path: &str) -> Result<()> {
    if db_path.is_empty() {
        return Err(anyhow::anyhow!("Please provide a database path using --db-path or HEADROOM_DB_PATH env var."));
    }

    let cache = SqliteCache::open(db_path, 0)?;
    let stats = cache.query_stats()?;
    let current_entries = cache.len()?;

    let ratio = if stats.total_original_bytes > 0 {
        let saved = stats.total_original_bytes.saturating_sub(stats.total_compressed_bytes);
        (saved as f64) / (stats.total_original_bytes as f64) * 100.0
    } else {
        0.0
    };

    println!("╔═══════════════════════════════════════════════════╗");
    println!("║           Headroom MCP — Session Statistics       ║");
    println!("╠═══════════════════════════════════════════════════╣");
    print_line("Total Compressions:", &format_number(stats.total_compressions));
    print_line("Total Original Bytes:", &format_number(stats.total_original_bytes));
    print_line("Total Compressed Bytes:", &format_number(stats.total_compressed_bytes));
    print_line("Overall Compression Ratio:", &format!("{:.1}%", ratio));
    print_line("Cache Entries (current):", &format_number(current_entries as u64));
    print_line("Database Size:", &format_bytes(stats.db_size_bytes));
    println!("╚═══════════════════════════════════════════════════╝");

    Ok(())
}

pub fn print_usage(db_path: &str, model_filter: Option<&str>, json: bool) -> Result<()> {
    if db_path.is_empty() {
        return Err(anyhow::anyhow!("Please provide a database path using --db-path or HEADROOM_DB_PATH env var."));
    }

    let cache = SqliteCache::open(db_path, 0)?;
    let rows = cache.query_usage(model_filter)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }

    if rows.is_empty() {
        println!("No compression history found in database.");
        return Ok(());
    }

    // Compute column widths
    let mut max_model_len = 5; // "Model".len()
    let mut total_orig = 0;
    let mut total_saved = 0;
    let mut total_usd = 0.0;

    for r in &rows {
        max_model_len = max_model_len.max(r.model.len());
        total_orig += r.total_original_tokens;
        total_saved += r.total_saved_tokens;
        total_usd += r.estimated_usd;
    }

    let w_model = max_model_len + 2;
    let w_orig = 14;
    let w_saved = 14;
    let w_pct = 12;
    let w_usd = 12;

    let widths = &[w_model, w_orig, w_saved, w_pct, w_usd];

    // Print top border
    print_border_line("┌", "┬", "┐", "─", widths);

    // Print headers
    println!(
        "│ {:<w1$} │ {:>w2$} │ {:>w3$} │ {:>w4$} │ {:>w5$} │",
        "Model", "Orig Tokens", "Saved Tokens", "Saving %", "USD Saved",
        w1=w_model-2, w2=w_orig-2, w3=w_saved-2, w4=w_pct-2, w5=w_usd-2
    );

    // Print separator
    print_border_line("├", "┼", "┤", "─", widths);

    // Print rows
    for r in &rows {
        println!(
            "│ {:<w1$} │ {:>w2$} │ {:>w3$} │ {:>w4$.1}% │ ${:>w5$.2} │",
            r.model, format_number(r.total_original_tokens), format_number(r.total_saved_tokens), r.saving_pct, r.estimated_usd,
            w1=w_model-2, w2=w_orig-2, w3=w_saved-2, w4=w_pct-3, w5=w_usd-3
        );
    }

    // Print separator
    print_border_line("├", "┼", "┤", "─", widths);

    // Print total
    let total_pct = if total_orig > 0 {
        (total_saved as f64) / (total_orig as f64) * 100.0
    } else {
        0.0
    };

    println!(
        "│ {:<w1$} │ {:>w2$} │ {:>w3$} │ {:>w4$.1}% │ ${:>w5$.2} │",
        "TOTAL", format_number(total_orig), format_number(total_saved), total_pct, total_usd,
        w1=w_model-2, w2=w_orig-2, w3=w_saved-2, w4=w_pct-3, w5=w_usd-3
    );

    // Print bottom border
    print_border_line("└", "┴", "┘", "─", widths);

    Ok(())
}

fn print_line(label: &str, value: &str) {
    let label_part = format!("  {}", label);
    let value_part = format!("{}  ", value);
    let spaces_needed = 51 - label_part.len() - value_part.len();
    let spaces = " ".repeat(spaces_needed);
    println!("║{}{}{}║", label_part, spaces, value_part);
}

fn print_border_line(left: &str, mid: &str, right: &str, sep: &str, widths: &[usize]) {
    let parts: Vec<String> = widths.iter().map(|w| sep.repeat(*w)).collect();
    println!("{}{}{}", left, parts.join(mid), right);
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn format_number(val: u64) -> String {
    let s = val.to_string();
    let mut result = String::new();
    let mut count = 0;
    for c in s.chars().rev() {
        if count > 0 && count % 3 == 0 {
            result.push(',');
        }
        result.push(c);
        count += 1;
    }
    result.chars().rev().collect()
}
