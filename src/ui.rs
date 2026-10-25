use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, BorderType, Chart, Clear, Dataset, GraphType, Paragraph, Wrap};
use ratatui::Frame;

use crate::args::Args;
use crate::derive::{Derived, Peaks};
use crate::fmt::{bytes, bytes_total, compact, count, duration, ms, num, pct, rate, rate_unit};
use crate::scrape::Shared;
use crate::theme::{trouble_color, usage_color, Theme, THEMES};

/// Minimum terminal size (rows, cols) for the full layout; below either, the
/// compact layout takes over.
const FULL_MIN_ROWS: u16 = 30;
const FULL_MIN_COLS: u16 = 90;

pub const GRAPH_PAGES: [&str; 5] = [
    "RATES",
    "SPECULATION",
    "ENG RAM",
    "REQUESTS",
    "CACHE",
];

pub struct Ui {
    pub theme_idx: usize,
    /// None = auto from terminal size, Some = forced
    pub compact: Option<bool>,
    pub graphs_on: bool,
    pub graph_page: usize,
    pub window_focus: usize,
    pub paused_since: Option<Instant>,
    pub insecure: bool,
    pub overlay: Overlay,
    pub scroll: u16,
    pub interval: f64,
    pub url: String,
}

#[derive(PartialEq, Clone, Copy)]
pub enum Overlay {
    None,
    Help,
    Explain,
}

impl Ui {
    pub fn new(args: &Args) -> Self {
        let theme_idx = THEMES
            .iter()
            .position(|t| t.name == args.theme)
            .unwrap_or(0);
        Self {
            theme_idx,
            compact: args.compact.then_some(true),
            graphs_on: true,
            graph_page: 0,
            window_focus: 2,
            paused_since: None,
            insecure: args.insecure,
            overlay: Overlay::None,
            scroll: 0,
            interval: args.interval.clamp(0.5, 10.0),
            url: args.url.clone(),
        }
    }

    pub fn theme(&self) -> &'static Theme {
        &THEMES[self.theme_idx]
    }
}

pub fn run(
    terminal: &mut ratatui::DefaultTerminal,
    shared: Arc<Shared>,
    ui: &mut Ui,
) -> anyhow::Result<()> {
    loop {
        if crossterm::event::poll(Duration::from_millis(120))? {
            if let crossterm::event::Event::Key(k) = crossterm::event::read()? {
                if k.kind == crossterm::event::KeyEventKind::Press
                    && handle_key(ui, &shared, k.code, k.modifiers)
                {
                    return Ok(());
                }
            }
        }
        terminal.draw(|f| draw(f, ui, &shared))?;
    }
}

fn handle_key(
    ui: &mut Ui,
    shared: &Arc<Shared>,
    code: crossterm::event::KeyCode,
    modifiers: crossterm::event::KeyModifiers,
) -> bool {
    use crossterm::event::KeyCode::*;
    if modifiers.contains(crossterm::event::KeyModifiers::CONTROL) {
        // Raw mode delivers Ctrl-C as a key event; without this it would look
        // like the compact-layout key and trap the operator.
        return matches!(code, Char('c'));
    }
    match code {
        Char('q') => true,
        Esc => {
            if ui.overlay == Overlay::None {
                true
            } else {
                ui.overlay = Overlay::None;
                ui.scroll = 0;
                false
            }
        }
        Char(' ') => {
            let now_paused = !shared.paused.load(Ordering::Relaxed);
            shared.paused.store(now_paused, Ordering::Relaxed);
            ui.paused_since = now_paused.then_some(Instant::now());
            false
        }
        Char('c') => {
            let auto = ui.compact.unwrap_or_else(auto_compact);
            ui.compact = Some(!auto);
            false
        }
        Char('g') => {
            ui.graphs_on = !ui.graphs_on;
            false
        }
        Char('v') => {
            ui.graph_page = (ui.graph_page + 1) % GRAPH_PAGES.len();
            false
        }
        Char('V') => {
            ui.graph_page = (ui.graph_page + GRAPH_PAGES.len() - 1) % GRAPH_PAGES.len();
            false
        }
        Char('t') => {
            ui.theme_idx = (ui.theme_idx + 1) % THEMES.len();
            false
        }
        Char(c @ '1'..='3') => {
            ui.window_focus = c.to_digit(10).unwrap_or(3) as usize - 1;
            false
        }
        Char('e') => {
            ui.overlay = if ui.overlay == Overlay::Explain {
                Overlay::None
            } else {
                Overlay::Explain
            };
            ui.scroll = 0;
            false
        }
        Char('?') => {
            ui.overlay = if ui.overlay == Overlay::Help {
                Overlay::None
            } else {
                Overlay::Help
            };
            false
        }
        Char('+') | Char('=') => {
            ui.interval = (ui.interval * 2.0).min(10.0);
            shared
                .interval_ms
                .store((ui.interval * 1000.0) as u64, Ordering::Relaxed);
            false
        }
        Char('-') => {
            ui.interval = (ui.interval / 2.0).max(0.5);
            shared
                .interval_ms
                .store((ui.interval * 1000.0) as u64, Ordering::Relaxed);
            false
        }
        Down | Char('j') => {
            if ui.overlay == Overlay::Explain {
                ui.scroll = ui.scroll.saturating_add(1);
            }
            false
        }
        Up | Char('k') => {
            ui.scroll = ui.scroll.saturating_sub(1);
            false
        }
        _ => false,
    }
}

fn auto_compact() -> bool {
    match crossterm::terminal::size() {
        Ok((cols, rows)) => rows < FULL_MIN_ROWS || cols < FULL_MIN_COLS,
        Err(_) => false,
    }
}

// ---------------------------------------------------------------- rendering

pub fn draw(f: &mut Frame, ui: &Ui, shared: &Arc<Shared>) {
    let t = ui.theme();
    f.render_widget(Block::new().style(Style::new().bg(t.bg)), f.area());

    let derived = shared.derived.lock().unwrap().clone();
    let first_seen = *shared.first_seen.lock().unwrap();
    let peaks = *shared.peaks.lock().unwrap();
    let age = shared.last_ok.lock().unwrap().map(|t| t.elapsed());
    let err = shared.last_error.lock().unwrap().clone();
    let model = shared.model.lock().unwrap().clone();
    let paused = shared.paused.load(Ordering::Relaxed);

    let area = f.area();
    let compact = ui
        .compact
        .unwrap_or(area.height < FULL_MIN_ROWS || area.width < FULL_MIN_COLS);
    if derived.is_none() {
        draw_header(
            f,
            ui,
            t,
            area,
            None,
            first_seen,
            age,
            err.as_deref(),
            model.as_deref(),
            paused,
        );
        draw_waiting(f, t, area, err.as_deref(), &ui.url);
        draw_overlay(f, ui, t, area);
        return;
    }
    let d = derived.as_ref().unwrap();
    let focus = ui.window_focus.min(2);

    let constraints = if compact {
        vec![
            Constraint::Length(1),
            Constraint::Length(4),
            if ui.graphs_on {
                Constraint::Min(3)
            } else {
                Constraint::Length(0)
            },
            Constraint::Min(4),
            Constraint::Length(1),
        ]
    } else {
        vec![
            Constraint::Length(1),
            Constraint::Length(7),
            if ui.graphs_on {
                Constraint::Min(7)
            } else {
                Constraint::Length(0)
            },
            Constraint::Length(14),
            Constraint::Length(1),
        ]
    };
    let chunks = Layout::vertical(constraints).split(area);

    draw_header(
        f,
        ui,
        t,
        chunks[0],
        Some(d),
        first_seen,
        age,
        err.as_deref(),
        model.as_deref(),
        paused,
    );
    if compact {
        draw_compact_summary(f, t, chunks[1], d, &peaks, focus);
    } else {
        draw_hero(f, t, chunks[1], d, focus);
    }
    if ui.graphs_on && chunks[2].height > 0 {
        draw_graphs(f, t, chunks[2], d, &peaks, focus, ui.graph_page);
    }
    if compact {
        draw_compact_detail(f, t, chunks[3], d, focus, &peaks);
    } else {
        draw_detail(f, t, chunks[3], d, focus);
    }
    draw_footer(f, t, chunks[4], ui, &peaks, err.as_deref(), paused);
    draw_overlay(f, ui, t, area);
}

fn draw_header(
    f: &mut Frame,
    ui: &Ui,
    t: &Theme,
    area: Rect,
    d: Option<&Derived>,
    first_seen: Option<Instant>,
    age: Option<Duration>,
    err: Option<&str>,
    model: Option<&str>,
    paused: bool,
) {
    let running = d.is_some_and(|d| d.flare.active > 0);
    let status = if paused {
        ("\u{275a}\u{275a} PAUSED", t.warn)
    } else if err.is_some() {
        if d.is_some() {
            ("\u{25cf} STALE", t.warn)
        } else {
            ("\u{25cf} WAITING", t.bad)
        }
    } else if d.is_some() {
        if running {
            ("\u{25cf} RUNNING", t.good)
        } else {
            ("\u{25cf} READY", t.good)
        }
    } else {
        ("\u{25cf} WAITING", t.dim)
    };
    let up = d.map_or_else(
        || first_seen.map_or_else(|| "\u{2014}".into(), |t| duration(t.elapsed().as_secs())),
        |d| duration(d.flare.uptime_seconds),
    );
    let mut spans = vec![
        Span::styled(
            " FLARE-TOP ",
            Style::default()
                .fg(t.bg)
                .bg(t.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" / HELIOS ", Style::default().fg(t.accent)),
        Span::styled(status.0, Style::default().fg(status.1)),
        Span::styled(format!("  up {up}"), Style::default().fg(t.dim)),
        Span::styled(
            format!(
                "  {}",
                model.map_or_else(|| "FLARE".into(), |m| crate::scrape::sanitize_for_terminal(m))
            ),
            Style::default().fg(t.fg),
        ),
        Span::styled(
            format!(
                "  poll {:.1}s{}",
                ui.interval,
                age.map_or_else(String::new, |a| format!(" · {:.1}s ago", a.as_secs_f64()))
            ),
            Style::default().fg(t.dim),
        ),
    ];
    if ui.insecure {
        spans.push(Span::styled("  TLS!", Style::default().fg(t.bad)));
    }
    spans.push(Span::styled(
        "  ? help \u{b7} e explain",
        Style::default().fg(t.dim),
    ));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_waiting(f: &mut Frame, t: &Theme, area: Rect, err: Option<&str>, url: &str) {
    let chunks = Layout::vertical([
        Constraint::Percentage(40),
        Constraint::Length(5),
        Constraint::Percentage(40),
    ])
    .split(area);
    let mut lines = vec![
        Line::from(Span::styled(
            " waiting for the first FLARE scrape",
            Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!(" {}", crate::scrape::sanitize_for_terminal(url)),
            Style::default().fg(t.dim),
        )),
    ];
    if let Some(err) = err {
        lines.push(Line::from(Span::styled(
            format!(" {err}"),
            Style::default().fg(t.bad),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            " the scraper retries every poll interval",
            Style::default().fg(t.dim),
        )));
    }
    f.render_widget(
        Paragraph::new(lines)
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(t.dim))
                    .title(Span::styled(
                        " FLARE-TOP ",
                        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                    )),
            )
            .alignment(ratatui::layout::Alignment::Center)
            .wrap(Wrap { trim: false }),
        chunks[1],
    );
}

fn metric_line(t: &Theme, label: &str, value: String, color: ratatui::style::Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<9}"), Style::default().fg(t.dim)),
        Span::styled(value, Style::default().fg(color)),
    ])
}

fn draw_hero(f: &mut Frame, t: &Theme, area: Rect, d: &Derived, focus: usize) {
    let s = &d.flare;
    let r = &d.rates[focus];
    let cells = Layout::horizontal([
        Constraint::Percentage(25),
        Constraint::Percentage(25),
        Constraint::Percentage(25),
        Constraint::Percentage(25),
    ])
    .spacing(1)
    .split(area);

    let engine = vec![
        Line::from(Span::styled(
            if s.active > 0 { "RUNNING" } else { "IDLE" },
            Style::default()
                .fg(if s.active > 0 { t.good } else { t.dim })
                .add_modifier(Modifier::BOLD),
        )),
        metric_line(t, "ACTIVE", s.active.to_string(), t.fg),
        metric_line(t, "QUEUE", s.pending.to_string(), trouble_color(t, s.pending as f64)),
        metric_line(t, "SLOTS", format!("{} free", s.free_slots), t.fg),
        metric_line(
            t,
            "INGRESS",
            format!("{}/{}", s.http_inflight, s.http_capacity),
            usage_color(t, s.ingress_usage().unwrap_or(0.0)),
        ),
    ];
    let mut throughput = vec![
        metric_line(t, "PREFILL", rate_unit(r.prefill, "tok/s"), t.s2),
        metric_line(t, "GENERATE", rate_unit(r.generated, "tok/s"), t.s1),
        metric_line(t, "SINGLE", rate_unit(r.per_stream, "tok/s"), t.s3),
    ];
    if s.decode_weight_bytes > 0 {
        throughput.push(metric_line(t, "WEIGHTS", bytes(r.weights), t.s4));
    } else {
        throughput.push(metric_line(
            t,
            "LAST MIN",
            format!("{} tok", count(s.generated_last_minute)),
            t.fg,
        ));
    }
    throughput.push(metric_line(t, "GPU BUSY", pct(r.gpu_busy), t.s1));
    let spec = vec![
        metric_line(
            t,
            "ACCEPT",
            format!("{} /rnd", num(r.acceptance)),
            t.s3,
        ),
        metric_line(
            t,
            "DEPTH",
            format!("{} /step", num(r.depth)),
            t.s3,
        ),
        metric_line(t, "VERIFY", rate_unit(r.verify, "steps/s"), t.s4),
        metric_line(t, "SPEC GPU", ms(s.spec_gpu_ms_per_round()), t.fg),
        metric_line(
            t,
            "HISTORY",
            if s.history_drafts { "drafts on" } else { "drafts off" }.into(),
            if s.history_drafts { t.good } else { t.dim },
        ),
    ];
    let kv_ratio = s.kv_usage().unwrap_or(0.0);
    let pools = vec![
        metric_line(t, "KV USED", pct(s.kv_usage()), usage_color(t, kv_ratio)),
        metric_line(t, "CACHED", format!("{} tok", count(s.cached_kv_tokens)), t.fg),
        metric_line(t, "FREE", format!("{} tok", count(s.free_kv_tokens)), t.fg),
        metric_line(t, "TOTAL", format!("{} tok", count(s.total_kv_tokens)), t.fg),
        metric_line(
            t,
            "SNAP",
            format!(
                "{}/{}",
                count(s.snapshot_capacity.saturating_sub(s.free_snapshots)),
                count(s.snapshot_capacity)
            ),
            t.fg,
        ),
    ];
    let window = crate::derive::WIN_LABELS[focus];
    for (cell, title, lines) in [
        (cells[0], " ENGINE ".to_string(), engine),
        (cells[1], format!(" THROUGHPUT \u{b7} {window} "), throughput),
        (cells[2], format!(" SPECULATION \u{b7} {window} "), spec),
        (cells[3], " MEMORY POOLS ".to_string(), pools),
    ] {
        f.render_widget(
            Paragraph::new(lines).block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(t.dim))
                    .title(Span::styled(
                        title,
                        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                    )),
            ),
            cell,
        );
    }
}

fn draw_compact_summary(f: &mut Frame, t: &Theme, area: Rect, d: &Derived, peaks: &Peaks, focus: usize) {
    let s = &d.flare;
    let r = &d.rates[focus];
    let lines = vec![
        Line::from(vec![
            Span::styled(" PREFILL ", Style::default().fg(t.s2)),
            Span::styled(rate_unit(r.prefill, "tok/s"), Style::default().fg(t.fg)),
            Span::styled("  GEN ", Style::default().fg(t.s1)),
            Span::styled(rate_unit(r.generated, "tok/s"), Style::default().fg(t.fg)),
            Span::styled("  SINGLE ", Style::default().fg(t.s3)),
            Span::styled(rate_unit(r.per_stream, "tok/s"), Style::default().fg(t.fg)),
            Span::styled(
                format!("  ACTIVE {}  QUEUE {}", s.active, s.pending),
                Style::default().fg(t.dim),
            ),
        ]),
        Line::from(vec![
            Span::styled(" KV ", Style::default().fg(t.dim)),
            Span::styled(
                format!(
                    "{} cached / {} free / {} tok",
                    count(s.cached_kv_tokens),
                    count(s.free_kv_tokens),
                    count(s.total_kv_tokens)
                ),
                Style::default().fg(t.fg),
            ),
            Span::styled("  ACCEPT ", Style::default().fg(t.dim)),
            Span::styled(
                num(r.acceptance),
                Style::default().fg(t.s3),
            ),
            Span::styled("  GPU ", Style::default().fg(t.dim)),
            Span::styled(pct(r.gpu_busy), Style::default().fg(t.fg)),
            Span::styled(
                format!("  peak {} tok/s", rate(peaks.generated)),
                Style::default().fg(t.dim),
            ),
        ]),
        Line::from(vec![
            Span::styled(" HOST ", Style::default().fg(t.dim)),
            Span::styled(
                format!(
                    "hit {} · out {} · in {}",
                    pct(r.host_hit),
                    bytes(r.host_out),
                    bytes(r.host_in)
                ),
                Style::default().fg(t.fg),
            ),
        ]),
    ];
    f.render_widget(Paragraph::new(lines), area);
}

struct PlotSeries<'a> {
    name: &'static str,
    color: ratatui::style::Color,
    values: &'a [Option<f64>],
    scale: f64,
}

struct GraphSpec<'a> {
    title: String,
    series: Vec<PlotSeries<'a>>,
}

fn whole(value: Option<f64>) -> String {
    value.map_or_else(|| "—".into(), |v| format!("{v:.0}"))
}

fn max_opt(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

fn mb(value: Option<f64>) -> String {
    value.map_or_else(|| "—".into(), |v| format!("{:.0}", v / 1e6))
}

/// Compact titles for the four-across row used when vertical space is tight.
/// Each one is `now/peak`, the same pair the full titles spell out.
fn pct0(value: Option<f64>) -> String {
    value.map_or_else(|| "—".into(), |v| format!("{:.0}%", v * 100.0))
}

fn short_ms(value: Option<f64>) -> String {
    match value {
        Some(v) if v >= 1000.0 => format!("{:.1}s", v / 1000.0),
        Some(v) if v >= 10.0 => format!("{v:.0}"),
        Some(v) => format!("{v:.1}"),
        None => "—".into(),
    }
}

/// Compact titles for the four-across row used when vertical space is tight.
/// Each one is `now pk peak`, the same pair the full titles spell out.
fn short_titles(
    d: &Derived,
    peaks: &Peaks,
    focus: usize,
    page: usize,
    tiny: bool,
) -> [String; 4] {
    let r = &d.rates[focus];
    let p = peaks;
    match page.min(GRAPH_PAGES.len() - 1) {
        0 => {
            let host_now = max_opt(r.host_out, r.host_in);
            let host_peak = max_opt(p.host_out, p.host_in);
            let prefill = if tiny { "PREF" } else { "PREFILL" };
            let generation = if tiny {
                format!("GEN {} pk {}", rate(r.generated), rate(p.generated))
            } else {
                format!(
                    "GEN {} pk {} s{}",
                    rate(r.generated),
                    rate(p.generated),
                    rate(r.per_stream)
                )
            };
            let (gpu_now, gpu_peak) = if tiny {
                (pct0(r.gpu_busy), pct0(p.gpu_busy))
            } else {
                (pct(r.gpu_busy), pct(p.gpu_busy))
            };
            let host = if tiny {
                format!("HOST {} pk {}", mb(host_now), mb(host_peak))
            } else {
                format!("HOST {} pk {} MB", mb(host_now), mb(host_peak))
            };
            [
                format!("{prefill} {} pk {}", rate(r.prefill), rate(p.prefill)),
                generation,
                format!("GPU {gpu_now} pk {gpu_peak}"),
                host,
            ]
        }
        1 => {
            let accept = if tiny { "ACPT" } else { "ACCEPT" };
            let depth = if tiny { "DPTH" } else { "DEPTH" };
            let verify = if tiny { "VRFY" } else { "VERIFY" };
            let ready = if tiny {
                format!(
                    "RDY {} pk {} ms",
                    short_ms(r.readiness_ms),
                    short_ms(p.readiness_ms)
                )
            } else {
                format!(
                    "READY {} pk {}",
                    ms(r.readiness_ms),
                    ms(p.readiness_ms)
                )
            };
            [
                format!("{accept} {} pk {}", num(r.acceptance), num(p.acceptance)),
                format!("{depth} {} pk {}", num(r.depth), num(p.depth)),
                format!("{verify} {} pk {}", rate(r.verify), rate(p.verify)),
                ready,
            ]
        }
        2 => {
            let gather = if tiny { "GATH" } else { "GATHER" };
            let (wait_now, wait_peak) = if tiny {
                (pct0(r.engram_disk_wait), pct0(p.engram_disk_wait))
            } else {
                (pct(r.engram_disk_wait), pct(p.engram_disk_wait))
            };
            let (hit_now, hit_peak) = if tiny {
                (pct0(r.engram_hit), pct0(p.engram_hit))
            } else {
                (pct(r.engram_hit), pct(p.engram_hit))
            };
            [
                format!(
                    "{gather} {} pk {}",
                    rate(r.engram_gathers),
                    rate(p.engram_gathers)
                ),
                format!(
                    "DISK {} pk {} MB",
                    mb(r.disk_bytes),
                    mb(p.engram_disk_bytes)
                ),
                format!("WAIT {wait_now} pk {wait_peak}"),
                format!("HIT {hit_now} pk {hit_peak}"),
            ]
        }
        3 => {
            let active = if tiny { "ACT" } else { "ACTIVE" };
            let queue = if tiny { "QUE" } else { "QUEUE" };
            [
                format!(
                    "{active} {} pk {}",
                    whole(Some(d.flare.active as f64)),
                    whole(p.active)
                ),
                format!(
                    "{queue} {} pk {}",
                    whole(Some(d.flare.pending as f64)),
                    whole(p.queue)
                ),
                format!("DONE {} pk {}", rate(r.completed), rate(p.completed)),
                format!("FAIL {} pk {}", rate(r.failed), rate(p.failed)),
            ]
        }
        _ => {
            let prefix = if tiny { "PFX" } else { "PREFIX" };
            let reuse = if tiny { "RUSE" } else { "REUSE" };
            let (prefix_now, prefix_peak) = if tiny {
                (pct0(r.cache_hit), pct0(p.cache_hit))
            } else {
                (pct(r.cache_hit), pct(p.cache_hit))
            };
            let (host_now, host_peak) = if tiny {
                (pct0(r.host_hit), pct0(p.host_hit))
            } else {
                (pct(r.host_hit), pct(p.host_hit))
            };
            let (kv_now, kv_peak) = if tiny {
                (pct0(d.flare.kv_usage()), pct0(p.kv_usage))
            } else {
                (pct(d.flare.kv_usage()), pct(p.kv_usage))
            };
            [
                format!("{prefix} {prefix_now} pk {prefix_peak}"),
                format!("HOST {host_now} pk {host_peak}"),
                format!("KV {kv_now} pk {kv_peak}"),
                format!("{reuse} {} pk {}", rate(r.reused), rate(p.reused)),
            ]
        }
    }
}

fn graph_specs<'a>(
    t: &Theme,
    d: &'a Derived,
    peaks: &Peaks,
    focus: usize,
    page: usize,
) -> Vec<GraphSpec<'a>> {
    let r = &d.rates[focus];
    let g = &d.graphs;
    let p = peaks;
    match page.min(GRAPH_PAGES.len() - 1) {
        0 => vec![
            GraphSpec {
                title: format!(
                    " PREFILL  {} · peak {} ",
                    rate_unit(r.prefill, "tok/s"),
                    rate_unit(p.prefill, "tok/s")
                ),
                series: vec![PlotSeries {
                    name: "",
                    color: t.s2,
                    values: &g.prefill.vals,
                    scale: 1.0,
                }],
            },
            GraphSpec {
                title: format!(
                    " GENERATION  {} · peak {} · single {} ",
                    rate_unit(r.generated, "tok/s"),
                    rate_unit(p.generated, "tok/s"),
                    rate_unit(r.per_stream, "tok/s")
                ),
                series: vec![
                    PlotSeries {
                        name: "aggregate",
                        color: t.s1,
                        values: &g.generated.vals,
                        scale: 1.0,
                    },
                    PlotSeries {
                        name: "single",
                        color: t.s3,
                        values: &g.per_stream.vals,
                        scale: 1.0,
                    },
                ],
            },
            GraphSpec {
                title: format!(
                    " GPU REPLAY  {} · peak {} ",
                    pct(r.gpu_busy),
                    pct(p.gpu_busy)
                ),
                series: vec![PlotSeries {
                    name: "",
                    color: t.accent,
                    values: &g.gpu_busy.vals,
                    scale: 100.0,
                }],
            },
            GraphSpec {
                title: format!(
                    " HOST I/O  out {} · in {} · peak {} ",
                    bytes(r.host_out),
                    bytes(r.host_in),
                    bytes(max_opt(p.host_out, p.host_in))
                ),
                series: vec![
                    PlotSeries {
                        name: "out",
                        color: t.s4,
                        values: &g.host_out.vals,
                        scale: 1.0 / 1e6,
                    },
                    PlotSeries {
                        name: "in",
                        color: t.s3,
                        values: &g.host_in.vals,
                        scale: 1.0 / 1e6,
                    },
                ],
            },
        ],
        1 => vec![
            GraphSpec {
                title: format!(
                    " ACCEPT  {} /round · peak {} ",
                    num(r.acceptance),
                    num(p.acceptance)
                ),
                series: vec![PlotSeries {
                    name: "",
                    color: t.s3,
                    values: &g.acceptance.vals,
                    scale: 1.0,
                }],
            },
            GraphSpec {
                title: format!(
                    " DEPTH  {} /step · peak {} ",
                    num(r.depth),
                    num(p.depth)
                ),
                series: vec![PlotSeries {
                    name: "",
                    color: t.accent,
                    values: &g.depth.vals,
                    scale: 1.0,
                }],
            },
            GraphSpec {
                title: format!(
                    " VERIFY  {} · peak {} ",
                    rate_unit(r.verify, "steps/s"),
                    rate_unit(p.verify, "steps/s")
                ),
                series: vec![PlotSeries {
                    name: "",
                    color: t.s4,
                    values: &g.verify.vals,
                    scale: 1.0,
                }],
            },
            GraphSpec {
                title: format!(
                    " READINESS  {} · peak {} ",
                    ms(r.readiness_ms),
                    ms(p.readiness_ms)
                ),
                series: vec![PlotSeries {
                    name: "",
                    color: t.s1,
                    values: &g.readiness_ms.vals,
                    scale: 1.0,
                }],
            },
        ],
        2 => vec![
            GraphSpec {
                title: format!(
                    " ENG GATHER  {} · peak {} ",
                    rate_unit(r.engram_gathers, "/s"),
                    rate_unit(p.engram_gathers, "/s")
                ),
                series: vec![PlotSeries {
                    name: "",
                    color: t.s4,
                    values: &g.engram_gathers.vals,
                    scale: 1.0,
                }],
            },
            GraphSpec {
                title: format!(
                    " ENG DISK  {} · peak {} ",
                    bytes(r.disk_bytes),
                    bytes(p.engram_disk_bytes)
                ),
                series: vec![PlotSeries {
                    name: "",
                    color: t.s3,
                    values: &g.engram_disk_bytes.vals,
                    scale: 1.0 / 1e6,
                }],
            },
            GraphSpec {
                title: format!(
                    " ENG WAIT  {} · peak {} ",
                    pct(r.engram_disk_wait),
                    pct(p.engram_disk_wait)
                ),
                series: vec![PlotSeries {
                    name: "",
                    color: t.warn,
                    values: &g.engram_disk_wait.vals,
                    scale: 100.0,
                }],
            },
            GraphSpec {
                title: format!(
                    " ROW HIT  {} · peak {} ",
                    pct(r.engram_hit),
                    pct(p.engram_hit)
                ),
                series: vec![PlotSeries {
                    name: "",
                    color: t.good,
                    values: &g.engram_hit.vals,
                    scale: 100.0,
                }],
            },
        ],
        3 => vec![
            GraphSpec {
                title: format!(
                    " ACTIVE  {} · peak {} ",
                    whole(Some(d.flare.active as f64)),
                    whole(p.active)
                ),
                series: vec![PlotSeries {
                    name: "",
                    color: t.s1,
                    values: &g.active.vals,
                    scale: 1.0,
                }],
            },
            GraphSpec {
                title: format!(
                    " QUEUE  {} · peak {} ",
                    whole(Some(d.flare.pending as f64)),
                    whole(p.queue)
                ),
                series: vec![PlotSeries {
                    name: "",
                    color: t.warn,
                    values: &g.queue.vals,
                    scale: 1.0,
                }],
            },
            GraphSpec {
                title: format!(
                    " COMPLETED  {} · peak {} ",
                    rate_unit(r.completed, "/s"),
                    rate_unit(p.completed, "/s")
                ),
                series: vec![PlotSeries {
                    name: "",
                    color: t.good,
                    values: &g.completed.vals,
                    scale: 1.0,
                }],
            },
            GraphSpec {
                title: format!(
                    " FAILED  {} · peak {} ",
                    rate_unit(r.failed, "/s"),
                    rate_unit(p.failed, "/s")
                ),
                series: vec![PlotSeries {
                    name: "",
                    color: t.bad,
                    values: &g.failed.vals,
                    scale: 1.0,
                }],
            },
        ],
        _ => vec![
            GraphSpec {
                title: format!(
                    " PREFIX HIT  {} · peak {} ",
                    pct(r.cache_hit),
                    pct(p.cache_hit)
                ),
                series: vec![PlotSeries {
                    name: "",
                    color: t.s1,
                    values: &g.cache_hit.vals,
                    scale: 100.0,
                }],
            },
            GraphSpec {
                title: format!(
                    " HOST HIT  {} · peak {} ",
                    pct(r.host_hit),
                    pct(p.host_hit)
                ),
                series: vec![PlotSeries {
                    name: "",
                    color: t.s4,
                    values: &g.host_hit.vals,
                    scale: 100.0,
                }],
            },
            GraphSpec {
                title: format!(
                    " KV USED  {} · peak {} ",
                    pct(d.flare.kv_usage()),
                    pct(p.kv_usage)
                ),
                series: vec![PlotSeries {
                    name: "",
                    color: t.accent,
                    values: &g.kv_usage.vals,
                    scale: 100.0,
                }],
            },
            GraphSpec {
                title: format!(
                    " REUSED  {} · peak {} ",
                    rate_unit(r.reused, "tok/s"),
                    rate_unit(p.reused, "tok/s")
                ),
                series: vec![PlotSeries {
                    name: "",
                    color: t.s3,
                    values: &g.reused.vals,
                    scale: 1.0,
                }],
            },
        ],
    }
}

fn draw_graphs(
    f: &mut Frame,
    t: &Theme,
    area: Rect,
    d: &Derived,
    peaks: &Peaks,
    focus: usize,
    page: usize,
) {
    let specs = graph_specs(t, d, peaks, focus, page);
    if specs.is_empty() {
        return;
    }
    let shorts = short_titles(d, peaks, focus, page, area.width < 100);
    if area.height < 12 && area.width >= 80 {
        let cols = Layout::horizontal([Constraint::Percentage(25); 4])
            .spacing(1)
            .split(area);
        for ((cell, spec), title) in cols.iter().zip(specs.iter()).zip(shorts.iter()) {
            plot(
                f,
                *cell,
                t,
                title,
                &spec.series,
                &d.graphs.dt,
                &d.graphs.stall,
            );
        }
        return;
    }
    if area.height >= 12 && area.width >= 80 {
        let rows = Layout::vertical([Constraint::Percentage(50); 2]).split(area);
        let top = Layout::horizontal([Constraint::Percentage(50); 2])
            .spacing(1)
            .split(rows[0]);
        let bottom = Layout::horizontal([Constraint::Percentage(50); 2])
            .spacing(1)
            .split(rows[1]);
        for (cell, spec) in top.iter().chain(bottom.iter()).zip(specs.iter()) {
            plot(
                f,
                *cell,
                t,
                &spec.title,
                &spec.series,
                &d.graphs.dt,
                &d.graphs.stall,
            );
        }
        return;
    }
    if area.width < 50 || specs.len() < 2 {
        let index = if page == 0 && specs.len() > 1 { 1 } else { 0 };
        let spec = &specs[index];
        plot(
            f,
            area,
            t,
            &shorts[index],
            &spec.series,
            &d.graphs.dt,
            &d.graphs.stall,
        );
        return;
    }
    let cols = Layout::horizontal([Constraint::Percentage(50); 2])
        .spacing(1)
        .split(area);
    for ((cell, spec), title) in cols.iter().zip(specs.iter()).zip(shorts.iter()) {
        plot(
            f,
            *cell,
            t,
            title,
            &spec.series,
            &d.graphs.dt,
            &d.graphs.stall,
        );
    }
}

fn plot(
    f: &mut Frame,
    area: Rect,
    t: &Theme,
    title: &str,
    series: &[PlotSeries<'_>],
    dt: &[f64],
    stall: &[bool],
) {
    if area.height < 3 || area.width < 4 {
        return;
    }
    let total: f64 = dt.iter().sum();
    let mut point_sets: Vec<Vec<(f64, f64)>> = Vec::with_capacity(series.len());
    let mut peak: f64 = 0.0;
    for series in series {
        let mut elapsed = 0.0;
        let mut points = Vec::new();
        for (i, value) in series.values.iter().enumerate().take(dt.len()) {
            elapsed += dt[i];
            if let Some(value) = value {
                let value = value * series.scale;
                peak = peak.max(value);
                points.push((elapsed - total, value));
            }
        }
        point_sets.push(points);
    }
    let mut datasets = Vec::new();
    for (series, points) in series.iter().zip(&point_sets) {
        if points.is_empty() {
            continue;
        }
        let mut dataset = Dataset::default()
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(series.color))
            .data(points);
        if !series.name.is_empty() {
            dataset = dataset.name(series.name);
        }
        datasets.push(dataset);
    }
    let mut ticks = Vec::new();
    if stall.iter().any(|s| *s) {
        let mut elapsed = 0.0;
        ticks = stall
            .iter()
            .enumerate()
            .take(dt.len())
            .filter_map(|(i, flagged)| {
                elapsed += dt[i];
                flagged.then_some((elapsed - total, 0.0))
            })
            .collect();
    }
    if !ticks.is_empty() {
        datasets.push(
            Dataset::default()
                .marker(Marker::Dot)
                .graph_type(GraphType::Scatter)
                .style(Style::default().fg(t.bad))
                .data(&ticks),
        );
    }
    let bound = (peak.max(1.0) * 1.15).max(1.0);
    let y_label = if peak <= 0.0 {
        String::new()
    } else if series.len() == 1 && series[0].scale == 100.0 {
        format!("{peak:.0}%")
    } else {
        compact(peak)
    };
    let mut block_title = title.to_owned();
    let stall_count = stall.iter().filter(|s| **s).count();
    if stall_count > 0 {
        block_title = format!("{block_title}\u{00d7}{stall_count}");
    }
    f.render_widget(
        Chart::new(datasets)
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(t.dim))
                    .title(Span::styled(
                        block_title,
                        Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
                    )),
            )
            .x_axis(
                Axis::default()
                    .bounds([-60.0, 0.0])
                    .labels(["-60s".to_string(), "now".to_string()])
                    .style(Style::default().fg(t.dim)),
            )
            .y_axis(
                Axis::default()
                    .bounds([0.0, bound])
                    .labels(["0".to_string(), y_label])
                    .style(Style::default().fg(t.dim)),
            ),
        area,
    );
}

struct Panel {
    title: &'static str,
    lines: Vec<Line<'static>>,
}

fn kv(t: &Theme, label: &str, value: String, color: ratatui::style::Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {label:<12}"), Style::default().fg(t.dim)),
        Span::styled(value, Style::default().fg(color)),
    ])
}

fn panels(t: &Theme, d: &Derived, focus: usize) -> [Panel; 4] {
    let s = &d.flare;
    let r = &d.rates[focus];
    let cache = [
        kv(
            t,
            "GPU HIT",
            pct(r.cache_hit),
            t.s1,
        ),
        kv(t, "REUSED", rate_unit(r.reused, "tok/s"), t.fg),
        kv(
            t,
            "CHECKPOINT",
            format!("{} cached", count(s.cached_checkpoints)),
            t.fg,
        ),
        kv(
            t,
            "PUBLISHED",
            rate_unit(r.snapshot_published, "/s"),
            t.fg,
        ),
        kv(t, "SNAPSHOT", pct(s.snapshot_usage()), t.fg),
        kv(
            t,
            "HOST HIT",
            pct(r.host_hit),
            t.s4,
        ),
        kv(t, "HOST OUT", bytes(r.host_out), t.fg),
        kv(t, "HOST IN", bytes(r.host_in), t.fg),
        kv(
            t,
            "BACKING",
            format!("{} pages", count(s.snapshot_backing_pages)),
            t.fg,
        ),
        kv(
            t,
            "CACHED %",
            pct(
                (s.total_kv_tokens > 0)
                    .then(|| s.cached_kv_tokens as f64 / s.total_kv_tokens as f64),
            ),
            t.fg,
        ),
        kv(
            t,
            "HOST MEM",
            format!(
                "{} rec / {}",
                count(s.host_memory_records),
                bytes_total(s.host_memory_bytes)
            ),
            t.fg,
        ),
        kv(
            t,
            "HOST DISK",
            format!(
                "{} rec / {}",
                count(s.host_disk_records),
                bytes_total(s.host_disk_bytes)
            ),
            t.fg,
        ),
    ];
    let spec = [
        kv(
            t,
            "ACCEPT",
            format!("{} /round", num(r.acceptance)),
            t.s3,
        ),
        kv(
            t,
            "DEPTH",
            format!("{} /step", num(r.depth)),
            t.s3,
        ),
        kv(t, "VERIFY", rate_unit(r.verify, "steps/s"), t.s4),
        kv(t, "ACCEPTED", rate_unit(r.accepted, "drafts/s"), t.fg),
        kv(t, "SPEC GPU", ms(s.spec_gpu_ms_per_round()), t.fg),
        kv(t, "EMITTED", format!("{} /round", num(s.emitted_per_round())), t.fg),
        kv(t, "OFFERED", format!("{} /round", num(s.offered_per_round())), t.fg),
        kv(
            t,
            "PROPOSAL",
            rate_unit(r.proposal, "/s"),
            t.fg,
        ),
        kv(
            t,
            "HISTORY",
            if s.history_drafts { "drafts on" } else { "drafts off" }.into(),
            if s.history_drafts { t.good } else { t.dim },
        ),
        kv(
            t,
            "CONSTRAIN",
            format!("{} inflight", s.constrained_max_inflight),
            t.fg,
        ),
        kv(t, "MIXED", rate_unit(r.mixed, "/s"), t.fg),
        kv(
            t,
            "GRAPH SPEC",
            format!("{} launches", count(s.sampling_launches("speculative"))),
            t.fg,
        ),
    ];
    let requests = [
        kv(
            t,
            "ACTIVE",
            format!("{}  queue {}", s.active, s.pending),
            if s.pending > 0 { t.warn } else { t.fg },
        ),
        kv(
            t,
            "FAIL",
            rate_unit(r.failed, "/s"),
            trouble_color(t, r.failed.unwrap_or(0.0)),
        ),
        kv(
            t,
            "CANCEL",
            rate_unit(r.cancelled, "/s"),
            trouble_color(t, r.cancelled.unwrap_or(0.0)),
        ),
        kv(
            t,
            "PREEMPT",
            rate_unit(r.preempted, "/s"),
            trouble_color(t, r.preempted.unwrap_or(0.0)),
        ),
        kv(t, "READY WAIT", ms(r.readiness_ms), t.fg),
        kv(t, "COMPLETE", rate_unit(r.completed, "/s"), t.fg),
        kv(
            t,
            "SLOTS",
            format!("{} free / {}", s.free_slots, s.request_slots()),
            t.fg,
        ),
        kv(
            t,
            "INGRESS",
            format!("{}/{}", s.http_inflight, s.http_capacity),
            usage_color(t, s.ingress_usage().unwrap_or(0.0)),
        ),
        kv(
            t,
            "FRONTEND",
            format!("{}/{}", s.frontend_active, s.frontend_capacity),
            usage_color(t, s.frontend_usage().unwrap_or(0.0)),
        ),
        kv(t, "HTTP", rate_unit(r.http, "req/s"), t.fg),
        kv(t, "SCHED", rate_unit(r.scheduler_exec, "exec/s"), t.fg),
    ];
    let mut host = Vec::new();
    if let Some(e) = s.engram {
        host.push(kv(
            t,
            "ENG GATHER",
            format!("{} /s · {}", rate(r.engram_gathers), ms(e.gather_ms())),
            t.s4,
        ));
        host.push(kv(
            t,
            "ENG DISK",
            format!("{} · wait {}", bytes(r.disk_bytes), pct(r.engram_disk_wait)),
            t.fg,
        ));
        host.push(kv(
            t,
            "ENG CACHE",
            format!("{} / {} rows", count(e.resident_rows), count(e.capacity_rows)),
            t.fg,
        ));
        host.push(kv(t, "ROW HIT", pct(r.engram_hit), t.fg));
    }
    host.push(kv(
        t,
        "HOST LOOKUP",
        rate_unit(r.host_lookup, "/s"),
        t.fg,
    ));
    host.push(kv(t, "HOST HITS", count(s.host_hits), t.fg));
    host.push(kv(
        t,
        "STORE/FETCH",
        format!("{} / {}", count(s.host_stored), count(s.host_fetched)),
        t.fg,
    ));
    host.push(kv(
        t,
        "RETRY",
        format!("{} ok / {}", count(s.host_retry_success), count(s.host_retry_abandoned)),
        t.fg,
    ));
    host.push(kv(
        t,
        "FAILURES",
        format!(
            "{} store / {} fetch",
            count(s.host_store_failed),
            count(s.host_fetch_failed)
        ),
        trouble_color(t, (s.host_store_failed + s.host_fetch_failed) as f64),
    ));
    host.push(kv(
        t,
        "OUTCOMES",
        format!(
            "{} adm · {} miss · {} rej",
            count(s.host_lookup_admitted),
            count(s.host_lookup_no_match),
            count(s.host_lookup_rejected)
        ),
        t.fg,
    ));
    if s.input.present {
        host.push(kv(
            t,
            "INPUT",
            format!(
                "hit {} · {} mem / {} disk",
                pct(s.input.hit_rate()),
                count(s.input.memory_records),
                count(s.input.disk_records)
            ),
            t.fg,
        ));
    }
    [
        Panel {
            title: " PREFIX CACHE ",
            lines: cache.to_vec(),
        },
        Panel {
            title: " SPECULATION ",
            lines: spec.to_vec(),
        },
        Panel {
            title: " REQUESTS & HEALTH ",
            lines: requests.to_vec(),
        },
        Panel {
            title: " HOST & ENG RAM ",
            lines: host,
        },
    ]
}

fn draw_detail(f: &mut Frame, t: &Theme, area: Rect, d: &Derived, focus: usize) {
    let panels = panels(t, d, focus);
    if area.width >= 128 {
        let cols = Layout::horizontal([Constraint::Percentage(25); 4])
            .spacing(1)
            .split(area);
        for (cell, panel) in cols.iter().zip(panels.iter()) {
            draw_panel(f, t, *cell, panel);
        }
    } else {
        let rows = Layout::vertical([Constraint::Percentage(50); 2]).split(area);
        let top = Layout::horizontal([Constraint::Percentage(50); 2])
            .spacing(1)
            .split(rows[0]);
        let bottom = Layout::horizontal([Constraint::Percentage(50); 2])
            .spacing(1)
            .split(rows[1]);
        for (cell, panel) in top.iter().chain(bottom.iter()).zip(panels.iter()) {
            draw_panel(f, t, *cell, panel);
        }
    }
}

fn draw_panel(f: &mut Frame, t: &Theme, area: Rect, panel: &Panel) {
    f.render_widget(
        Paragraph::new(panel.lines.clone()).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(t.dim))
                .title(Span::styled(
                    panel.title,
                    Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                )),
        ),
        area,
    );
}

fn draw_compact_detail(
    f: &mut Frame,
    t: &Theme,
    area: Rect,
    d: &Derived,
    focus: usize,
    peaks: &Peaks,
) {
    let r = &d.rates[focus];
    let lines = vec![
        Line::from(vec![
            Span::styled(" cache ", Style::default().fg(t.dim)),
            Span::styled(
                format!(
                    "gpu hit {} · reused {} · host hit {}",
                    pct(r.cache_hit),
                    rate_unit(r.reused, "tok/s"),
                    pct(r.host_hit)
                ),
                Style::default().fg(t.fg),
            ),
        ]),
        Line::from(vec![
            Span::styled(" spec  ", Style::default().fg(t.dim)),
            Span::styled(
                format!(
                    "accept {} · depth {} · verify {}",
                    num(r.acceptance),
                    num(r.depth),
                    rate_unit(r.verify, "steps/s")
                ),
                Style::default().fg(t.fg),
            ),
        ]),
        Line::from(vec![
            Span::styled(" health", Style::default().fg(t.dim)),
            Span::styled(
                format!(
                    " fail {} · cancel {} · preempt {} · ready {}",
                    rate_unit(r.failed, "/s"),
                    rate_unit(r.cancelled, "/s"),
                    rate_unit(r.preempted, "/s"),
                    ms(r.readiness_ms)
                ),
                Style::default().fg(t.fg),
            ),
        ]),
        Line::from(vec![
            Span::styled(" peaks ", Style::default().fg(t.dim)),
            Span::styled(
                format!(
                    "gen {} · prefill {} · single {} tok/s",
                    rate(peaks.generated),
                    rate(peaks.prefill),
                    rate(peaks.per_stream)
                ),
                Style::default().fg(t.fg),
            ),
        ]),
    ];
    f.render_widget(Paragraph::new(lines), area);
}

fn draw_footer(
    f: &mut Frame,
    t: &Theme,
    area: Rect,
    ui: &Ui,
    peaks: &Peaks,
    err: Option<&str>,
    paused: bool,
) {
    let page = ui.graph_page.min(GRAPH_PAGES.len() - 1);
    let mut spans = vec![
        Span::styled(" GRAPHS ", Style::default().fg(t.dim)),
        if ui.graphs_on {
            Span::styled(
                format!(
                    "{}/{} {} (v)",
                    page + 1,
                    GRAPH_PAGES.len(),
                    GRAPH_PAGES[page]
                ),
                Style::default().fg(t.accent),
            )
        } else {
            Span::styled("off (g)", Style::default().fg(t.warn))
        },
        Span::styled("  ·  PEAKS ", Style::default().fg(t.dim)),
        Span::styled(
            format!(
                "prefill {} tok/s · decode {} tok/s · single {} tok/s · GPU {} · accept {} · depth {}",
                rate(peaks.prefill),
                rate(peaks.generated),
                rate(peaks.per_stream),
                pct(peaks.gpu_busy),
                num(peaks.acceptance),
                num(peaks.depth)
            ),
            Style::default().fg(t.fg),
        ),
        Span::styled("   ", Style::default()),
    ];
    if let Some(err) = err {
        spans.push(Span::styled(
            format!("error: {err}"),
            Style::default().fg(t.bad),
        ));
    } else if paused {
        spans.push(Span::styled(
            format!(
                "paused {}s",
                ui.paused_since.map_or(0, |t| t.elapsed().as_secs())
            ),
            Style::default().fg(t.warn),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_overlay(f: &mut Frame, ui: &Ui, t: &Theme, area: Rect) {
    let (title, lines) = match ui.overlay {
        Overlay::None => return,
        Overlay::Help => (" KEYS ", help_lines(t)),
        Overlay::Explain => (" EXPLAIN ", explain_lines(t)),
    };
    let popup = Layout::vertical([
        Constraint::Percentage(8),
        Constraint::Percentage(84),
        Constraint::Percentage(8),
    ])
    .split(area)[1];
    let popup = Layout::horizontal([
        Constraint::Percentage(8),
        Constraint::Percentage(84),
        Constraint::Percentage(8),
    ])
    .split(popup)[1];
    f.render_widget(Clear, popup);
    f.render_widget(
        Paragraph::new(lines)
            .block(
                Block::bordered()
                    .border_type(BorderType::Double)
                    .border_style(Style::default().fg(t.accent))
                    .title(Span::styled(
                        title,
                        Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
                    )),
            )
            .style(Style::default().bg(t.bg))
            .wrap(Wrap { trim: false })
            .scroll((ui.scroll, 0)),
        popup,
    );
}

fn help_lines(t: &Theme) -> Vec<Line<'static>> {
    let key = |k: &str, text: &str| {
        Line::from(vec![
            Span::styled(format!(" {k:<10}"), Style::default().fg(t.accent)),
            Span::styled(text.to_string(), Style::default().fg(t.fg)),
        ])
    };
    vec![
        Line::from(Span::styled(
            " flare-top keys",
            Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
        )),
        key("q/Esc/^C", "quit (Esc closes an overlay first)"),
        key("space", "pause scraping"),
        key("c", "toggle compact layout"),
        key("g", "toggle graphs"),
        key("v / V", "next / previous graph page"),
        key("1 2 3", "focus the 5s / 15s / 60s window"),
        key("t", "cycle theme (gruvbox, catppuccin, tokyonight)"),
        key("+ / -", "poll interval (clamped 0.5–10s)"),
        key("e", "explain the dashboard"),
        key("?", "this keymap"),
        key("↑ / ↓", "scroll the explain screen"),
        Line::default(),
        Line::from(Span::styled(
            " Graph window is always the trailing 60s; the 1/2/3 focus changes",
            Style::default().fg(t.dim),
        )),
        Line::from(Span::styled(
            " the headline rates, panel rates, and stall budget.",
            Style::default().fg(t.dim),
        )),
    ]
}

fn explain_lines(t: &Theme) -> Vec<Line<'static>> {
    let heading = |text: &str| {
        Line::from(Span::styled(
            format!(" {text}"),
            Style::default().fg(t.accent).add_modifier(Modifier::BOLD),
        ))
    };
    let body = |text: &str| {
        Line::from(Span::styled(
            format!(" {text}"),
            Style::default().fg(t.fg),
        ))
    };
    let muted = |text: &str| {
        Line::from(Span::styled(
            format!(" {text}"),
            Style::default().fg(t.dim),
        ))
    };
    vec![
        heading("flare-top · reading the Flare / Helios dashboard"),
        Line::default(),
        heading("THROUGHPUT"),
        body("PREFILL is prompt tokens consumed by completed prefill, excluding padding and prefix reuse."),
        body("GENERATE is committed generated tokens, including EOS and tokens later withheld by stops."),
        body("SINGLE divides the window's generation rate by the time-weighted mean live model slots: the per-request pace."),
        body("The graphs use a fixed trailing 60s wall-clock axis; the rightmost point is now."),
        muted("Every rate is a counter delta over the selected 5s/15s/60s window."),
        Line::default(),
        heading("GRAPHS AND PEAKS"),
        body("v / V cycle five pages: rates, speculation, Engram, requests, cache."),
        body("Every graph title shows the focused-window value and the session peak."),
        body("At tight heights four graphs share a row; those titles read now pk peak."),
        body("The footer repeats peak prefill, generation, single-stream, GPU, acceptance, and depth."),
        Line::default(),
        heading("SPECULATION"),
        body("ACCEPT is accepted draft tokens per draft-offering request round, excluding the bonus token."),
        body("DEPTH is allocated draft positions per progressing decode or verification round, rejected drafts included."),
        body("SPEC GPU is cumulative speculative/verification CUDA time per completed combined graph."),
        body("HISTORY reports whether shared decoded history augments learned proposals."),
        Line::default(),
        heading("MEMORY AND PREFIX CACHE"),
        body("KV USED is (total - free) logical KV tokens after completed GPU fences; cached pages are retained checkpoints."),
        body("CHECKPOINTS are GPU-ready immutable snapshots; PUBLISHED is their rate and BACKING pages are pinned by them."),
        body("HOST HIT is the fraction of host prefix-store lookups whose chain admission fetched."),
        body("HOST OUT/IN is measured GPU-to-host and host-to-GPU copy bandwidth."),
        Line::default(),
        heading("GPU AND REQUESTS"),
        body("GPU BUSY is completed graph CUDA time (maximum rank-local duration, including embedded readiness waits) per wall second."),
        body("READY WAIT is exposed embedding readiness delay per instrumented graph; RAM and disk delivery both count."),
        body("SLOTS is active + free + retiring model slots. INGRESS is reserved HTTP generation slots."),
        body("FAIL/CANCEL/PREEMPT are rates of failed, cancelled, and KV-pressure readmissions."),
        Line::default(),
        heading("HOST STORE AND ENG RAM"),
        body("HOST STORE counters cover copy-out, restore, retries, and failures of the prefix-cache host tier."),
        body("ENG GATHER is embedding row gather throughput and wall time; ENG DISK is direct-I/O row delivery."),
        body("ENG DISK WAIT is disk completion wait as a share of gather wall time; ROW HIT is the row-cache hit fraction."),
        Line::default(),
        heading("STALLS"),
        body("A stall interval has generation below 25% of the 60s nonzero median while prefill and requests are present."),
        body("Red dots on the graphs mark stall intervals; the footer counts them and their frozen seconds."),
        muted("Missing rate families read as —, never as zero. Press e or Esc to close."),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::parse;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn shared() -> Arc<Shared> {
        let s = Shared::new();
        let t0 = Instant::now();
        let body = "# TYPE flare_prefill_tokens_total counter\nflare_prefill_tokens_total 1000\n\
                    # TYPE flare_generated_tokens_total counter\nflare_generated_tokens_total 400\n\
                    # TYPE flare_active_requests gauge\nflare_active_requests 2\n\
                    # TYPE flare_pending_requests gauge\nflare_pending_requests 1\n\
                    # TYPE flare_free_request_slots gauge\nflare_free_request_slots 6\n\
                    # TYPE flare_http_inflight_requests gauge\nflare_http_inflight_requests 2\n\
                    # TYPE flare_http_request_capacity gauge\nflare_http_request_capacity 16\n\
                    # TYPE flare_total_kv_tokens gauge\nflare_total_kv_tokens 100000\n\
                    # TYPE flare_free_kv_tokens gauge\nflare_free_kv_tokens 25000\n\
                    # TYPE flare_cached_kv_tokens gauge\nflare_cached_kv_tokens 50000\n\
                    # TYPE flare_snapshot_capacity gauge\nflare_snapshot_capacity 256\n\
                    # TYPE flare_uptime_seconds gauge\nflare_uptime_seconds 3661\n\
                    # TYPE flare_draft_request_rounds_total counter\nflare_draft_request_rounds_total 10\n\
                    # TYPE flare_accepted_draft_tokens_total counter\nflare_accepted_draft_tokens_total 20\n\
                    # TYPE flare_verification_draft_tokens_total counter\nflare_verification_draft_tokens_total 40\n\
                    # TYPE flare_verification_request_rounds_total counter\nflare_verification_request_rounds_total 10\n\
                    # TYPE flare_cache_lookups_total counter\nflare_cache_lookups_total 10\n\
                    # TYPE flare_cache_hits_total counter\nflare_cache_hits_total 9\n\
                    # TYPE flare_replay_gpu_nanoseconds_total counter\nflare_replay_gpu_nanoseconds_total 900000000\n";
        let mut h = s.history.lock().unwrap();
        h.push(t0, parse(body).unwrap());
        let body2 = body
            .replace("prefill_tokens_total 1000", "prefill_tokens_total 2500")
            .replace("generated_tokens_total 400", "generated_tokens_total 1200")
            .replace("replay_gpu_nanoseconds_total 900000000", "replay_gpu_nanoseconds_total 1800000000");
        h.push(t0 + Duration::from_secs(1), parse(&body2).unwrap());
        {
            let mut peaks = s.peaks.lock().unwrap();
            crate::derive::update_peaks(&mut peaks, &h);
        }
        *s.derived.lock().unwrap() = crate::derive::derive(&h).map(Arc::new);
        drop(h);
        *s.first_seen.lock().unwrap() = Some(t0);
        *s.last_ok.lock().unwrap() = Some(Instant::now());
        *s.model.lock().unwrap() = Some("DeepSeek-V4.1-Flash".into());
        s
    }

    fn args() -> Args {
        Args {
            url: "http://localhost:8000".into(),
            api_key: None,
            insecure: false,
            interval: 1.0,
            theme: "gruvbox".into(),
            compact: false,
            once: false,
        }
    }

    fn render(width: u16, height: u16, ui: &Ui) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let shared = shared();
        terminal.draw(|f| draw(f, ui, &shared)).unwrap();
        let buffer = terminal.backend().buffer();
        buffer
            .content
            .chunks(width as usize)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn full_layout_names_every_panel_and_shows_rates() {
        let ui = Ui::new(&args());
        let text = render(150, 40, &ui);
        for needle in [
            "FLARE-TOP",
            "DeepSeek-V4.1-Flash",
            "ENGINE",
            "THROUGHPUT",
            "SPECULATION",
            "MEMORY POOLS",
            "PREFILL",
            "GENERATION",
            "PREFIX CACHE",
            "REQUESTS & HEALTH",
            "HOST & ENG RAM",
            "PEAKS",
        ] {
            assert!(text.contains(needle), "missing {needle:?}:\n{text}");
        }
        assert!(text.contains("1.5k") || text.contains("1500"));
        assert!(text.contains("THROUGHPUT \u{b7} 60s"));
        assert!(text.contains("peak"));
        assert!(text.contains("GRAPHS 1/5 RATES (v)"));
    }

    #[test]
    fn graph_pages_cycle_and_each_title_has_a_peak() {
        for (page, needle) in [
            (0, "PREFILL"),
            (1, "READINESS"),
            (2, "ENG GATHER"),
            (3, "COMPLETED"),
            (4, "PREFIX HIT"),
        ] {
            let mut ui = Ui::new(&args());
            ui.graph_page = page;
            let text = render(150, 40, &ui);
            assert!(text.contains(needle), "page {page} missing {needle:?}:\n{text}");
            assert!(text.contains("peak"), "page {page} lacks peak label:\n{text}");
        }
    }

    #[test]
    fn compact_layout_fits_a_small_terminal() {
        let mut ui = Ui::new(&args());
        ui.compact = None;
        let text = render(80, 24, &ui);
        assert!(text.contains("FLARE-TOP"));
        assert!(text.contains("PREFILL"));
        assert!(text.contains("GEN"));
        assert!(text.contains("pk"));
        assert!(text.contains("peaks"));
        assert!(!text.contains("PREFIX CACHE"));
    }

    #[test]
    fn keys_toggle_pause_theme_window_and_quit() {
        let shared = Shared::new();
        let mut ui = Ui::new(&args());
        assert!(!handle_key(
            &mut ui,
            &shared,
            crossterm::event::KeyCode::Char(' '),
            crossterm::event::KeyModifiers::NONE,
        ));
        assert!(shared.paused.load(Ordering::Relaxed));
        assert!(ui.paused_since.is_some());
        assert!(!handle_key(
            &mut ui,
            &shared,
            crossterm::event::KeyCode::Char('1'),
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(ui.window_focus, 0);
        let theme = ui.theme_idx;
        assert!(!handle_key(
            &mut ui,
            &shared,
            crossterm::event::KeyCode::Char('t'),
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_ne!(ui.theme_idx, theme);
        assert_eq!(ui.graph_page, 0);
        assert!(!handle_key(
            &mut ui,
            &shared,
            crossterm::event::KeyCode::Char('v'),
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(ui.graph_page, 1);
        assert!(!handle_key(
            &mut ui,
            &shared,
            crossterm::event::KeyCode::Char('V'),
            crossterm::event::KeyModifiers::NONE,
        ));
        assert_eq!(ui.graph_page, 0);
        assert!(!handle_key(
            &mut ui,
            &shared,
            crossterm::event::KeyCode::Char('?'),
            crossterm::event::KeyModifiers::NONE,
        ));
        assert!(ui.overlay == Overlay::Help);
        assert!(!handle_key(
            &mut ui,
            &shared,
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert!(ui.overlay == Overlay::None);
        assert!(handle_key(
            &mut ui,
            &shared,
            crossterm::event::KeyCode::Char('c'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        assert!(handle_key(
            &mut ui,
            &shared,
            crossterm::event::KeyCode::Char('q'),
            crossterm::event::KeyModifiers::NONE,
        ));
    }

    #[test]
    fn overlays_render_without_panic() {
        let mut ui = Ui::new(&args());
        ui.overlay = Overlay::Help;
        assert!(render(120, 40, &ui).contains("flare-top keys"));
        ui.overlay = Overlay::Explain;
        let text = render(120, 40, &ui);
        assert!(text.contains("THROUGHPUT"));
        assert!(text.contains("SPECULATION"));
    }
}
