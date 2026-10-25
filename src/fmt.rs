/// Compact number used for rates and counters.
pub fn compact(value: f64) -> String {
    if value >= 1e9 {
        format!("{:.1}G", value / 1e9)
    } else if value >= 1e6 {
        format!("{:.1}M", value / 1e6)
    } else if value >= 1e3 {
        format!("{:.1}k", value / 1e3)
    } else if value >= 100.0 {
        format!("{value:.0}")
    } else if value >= 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    }
}

pub fn rate(value: Option<f64>) -> String {
    value.map_or_else(|| "—".into(), compact)
}

pub fn rate_unit(value: Option<f64>, unit: &str) -> String {
    value.map_or_else(
        || "—".into(),
        |v| {
            if unit.starts_with('/') {
                format!("{}{unit}", compact(v))
            } else {
                format!("{} {unit}", compact(v))
            }
        },
    )
}

pub fn pct(value: Option<f64>) -> String {
    value.map_or_else(|| "—".into(), |v| format!("{:.1}%", v * 100.0))
}

pub fn num(value: Option<f64>) -> String {
    value.map_or_else(|| "—".into(), compact)
}

pub fn count(value: u64) -> String {
    if value >= 1000 {
        compact(value as f64)
    } else {
        value.to_string()
    }
}

/// Byte throughput: `1.2 GB/s`, `4.5 MB/s`, ...
pub fn bytes(value: Option<f64>) -> String {
    match value {
        Some(v) if v >= 1e9 => format!("{:.2} GB/s", v / 1e9),
        Some(v) if v >= 1e6 => format!("{:.1} MB/s", v / 1e6),
        Some(v) if v >= 1e3 => format!("{:.1} kB/s", v / 1e3),
        Some(v) => format!("{v:.0} B/s"),
        None => "—".into(),
    }
}

/// One byte total in binary units.
pub fn bytes_total(value: u64) -> String {
    if value >= 1 << 30 {
        format!("{:.1} GiB", value as f64 / (1u64 << 30) as f64)
    } else if value >= 1 << 20 {
        format!("{:.1} MiB", value as f64 / (1u64 << 20) as f64)
    } else if value >= 1 << 10 {
        format!("{:.1} KiB", value as f64 / (1u64 << 10) as f64)
    } else {
        format!("{value} B")
    }
}

pub fn ms(value: Option<f64>) -> String {
    match value {
        Some(v) if v >= 1000.0 => format!("{:.2}s", v / 1000.0),
        Some(v) => format!("{v:.2}ms"),
        None => "—".into(),
    }
}

pub fn duration(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = seconds / 3600 % 24;
    let minutes = seconds / 60 % 60;
    let secs = seconds % 60;
    if days > 0 {
        format!("{days}d {hours:02}:{minutes:02}:{secs:02}")
    } else {
        format!("{hours:02}:{minutes:02}:{secs:02}")
    }
}
