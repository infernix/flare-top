use std::io::IsTerminal;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;

use flare_top::{args::Args, metrics, scrape, ui};

/// Rewrites a scrape or parse failure for the anyhow return: parser bails can
/// embed label values verbatim, so the printed message must carry no raw
/// terminal control bytes.
fn safe_err(e: anyhow::Error) -> anyhow::Error {
    anyhow::anyhow!("{}", scrape::sanitize_for_terminal(&format!("{e:#}")))
}

fn main() -> Result<()> {
    let args = Args::parse();
    let url = scrape::metrics_url(&args.url);

    if args.once {
        if args.insecure {
            eprintln!("\u{26a0} insecure TLS: the bearer token and metrics transit unverified");
        }
        let mut history = flare_top::history::History::default();
        // Two scrapes give every window a base for rates.
        for (i, wait) in [0.0, args.interval.clamp(0.5, 10.0)]
            .into_iter()
            .enumerate()
        {
            if i > 0 {
                std::thread::sleep(Duration::from_secs_f64(wait));
            }
            let body = scrape::fetch_once(&url, args.api_key.as_deref(), args.insecure)
                .map_err(safe_err)?;
            let sample = metrics::parse(&body).map_err(safe_err)?;
            history.push(Instant::now(), sample);
        }
        let mut peaks = flare_top::derive::Peaks::default();
        let model = scrape::fetch_model_once(&args.url, args.api_key.as_deref(), args.insecure);
        flare_top::once::print_once(&history, &mut peaks, model.as_deref(), &args.url)?;
        return Ok(());
    }

    if !std::io::stdout().is_terminal() {
        anyhow::bail!("flare-top needs a terminal; use --once for scripts and logs");
    }

    let shared = scrape::Shared::new();
    shared.interval_ms.store(
        (args.interval.clamp(0.5, 10.0) * 1000.0) as u64,
        Ordering::Relaxed,
    );
    scrape::spawn_scraper(
        shared.clone(),
        args.url.clone(),
        args.api_key.clone(),
        args.insecure,
    );

    let mut terminal = ratatui::init();
    let mut ui_state = ui::Ui::new(&args);
    let result = ui::run(&mut terminal, shared, &mut ui_state);
    ratatui::restore();
    result
}
