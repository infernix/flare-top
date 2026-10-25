use clap::Parser;

fn finite_interval(s: &str) -> Result<f64, String> {
    let v: f64 = s.parse().map_err(|_| format!("invalid number: {s}"))?;
    if v.is_finite() {
        Ok(v)
    } else {
        Err(format!("interval must be finite, got {s}"))
    }
}

/// btop-style live TUI for a FLARE (Helios) inference server.
///
/// Scrapes the FLARE /metrics endpoint once per interval and renders a dense,
/// plain-language status screen. Nothing is persisted; the only state is the
/// in-memory 5s/15s/60s window.
#[derive(Parser, Debug)]
#[command(name = "flare-top", version, about)]
pub struct Args {
    /// Base URL of the FLARE server (FLARE's default port is 8000).
    #[arg(long, default_value = "http://localhost:8000")]
    pub url: String,

    /// Serving key, if the server requires one for /v1/models.
    #[arg(long)]
    pub api_key: Option<String>,

    /// Accept invalid/self-signed TLS certificates (homelab proxies).
    #[arg(long)]
    pub insecure: bool,

    /// Scrape interval in seconds (clamped 0.5–10; adjustable live with +/-).
    #[arg(long, default_value_t = 1.0, value_parser = finite_interval)]
    pub interval: f64,

    /// Theme: gruvbox | catppuccin | tokyonight.
    #[arg(long, default_value = "gruvbox")]
    pub theme: String,

    /// Force compact layout (normally auto-detected from terminal size).
    #[arg(long)]
    pub compact: bool,

    /// Print one text snapshot and exit (for scripts/logs).
    #[arg(long)]
    pub once: bool,
}
