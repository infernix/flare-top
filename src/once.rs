use anyhow::Result;

use crate::derive::{self, Derived, WIN_LABELS};
use crate::fmt::{bytes, bytes_total, count, duration, ms, num, pct, rate, rate_unit};
use crate::history::History;

/// One-shot text snapshot for scripts, cron logs, and parser verification.
pub fn print_once(
    h: &History,
    peaks: &mut derive::Peaks,
    model: Option<&str>,
    base: &str,
) -> Result<()> {
    println!("{}", snapshot_text(h, peaks, model, base)?);
    Ok(())
}

/// Renders the one-shot snapshot without printing it.
pub fn snapshot_text(
    h: &History,
    peaks: &mut derive::Peaks,
    model: Option<&str>,
    base: &str,
) -> Result<String> {
    let d = derive::derive(h).ok_or_else(|| anyhow::anyhow!("no data scraped yet"))?;
    derive::update_peaks(peaks, h);
    Ok(render(&d, peaks, model, base))
}

fn render(d: &Derived, peaks: &derive::Peaks, model: Option<&str>, base: &str) -> String {
    let f = 2;
    let s = &d.flare;
    let r = &d.rates[f];
    let header = format!(
        "flare-top {} — {} · {} · up {}",
        crate::scrape::sanitize_for_terminal(base),
        model.map_or_else(|| "FLARE".into(), |m| crate::scrape::sanitize_for_terminal(m)),
        if s.active > 0 { "RUNNING" } else { "READY" },
        duration(s.uptime_seconds)
    );
    let windows = WIN_LABELS.join("/");
    let mut lines = vec![header];
    let weights = if s.decode_weight_bytes > 0 {
        format!(" · weights {}", bytes(r.weights))
    } else {
        String::new()
    };
    lines.push(format!(
        "throughput  prefill {} · decode {} · single-stream {} · last minute {} tok{}",
        rate_unit(r.prefill, "tok/s"),
        rate_unit(r.generated, "tok/s"),
        rate_unit(r.per_stream, "tok/s"),
        count(s.generated_last_minute),
        weights
    ));
    lines.push(format!(
        "windows     prefill [{windows}]: {} · decode: {}",
        d.rates
            .iter()
            .map(|w| rate_unit(w.prefill, "tok/s"))
            .collect::<Vec<_>>()
            .join(" / "),
        d.rates
            .iter()
            .map(|w| rate_unit(w.generated, "tok/s"))
            .collect::<Vec<_>>()
            .join(" / ")
    ));
    lines.push(format!(
        "requests    active {} · queue {} · slots {} free/{} · admitted {} · complete {} · failed {} · cancelled {}",
        s.active,
        s.pending,
        s.free_slots,
        s.request_slots(),
        count(s.admitted),
        count(s.completed),
        count(s.failed),
        count(s.cancelled)
    ));
    lines.push(format!(
        "pools       KV {} cached / {} free / {} tok · {} used · snapshots {} slots / {} backing pages",
        count(s.cached_kv_tokens),
        count(s.free_kv_tokens),
        count(s.total_kv_tokens),
        pct(s.kv_usage()),
        count(s.snapshot_capacity),
        count(s.snapshot_backing_pages)
    ));
    lines.push(format!(
        "spec        accept {}/round · depth {}/step · verify {} · accepted {} · spec GPU {} · history drafts {}",
        num(r.acceptance),
        num(r.depth),
        rate_unit(r.verify, "steps/s"),
        rate_unit(r.accepted, "drafts/s"),
        ms(s.spec_gpu_ms_per_round()),
        if s.history_drafts { "on" } else { "off" }
    ));
    lines.push(format!(
        "prefix      GPU hit {} · reused {} · published {} · evicted {} · checkpoints {} · preempt {}",
        pct(r.cache_hit),
        rate_unit(r.reused, "tok/s"),
        rate_unit(r.snapshot_published, "/s"),
        rate_unit(r.snapshot_evicted, "/s"),
        count(s.cached_checkpoints),
        rate_unit(r.preempted, "/s")
    ));
    lines.push(format!(
        "host cache  hit {} · out {} · in {} · mem {} rec / {} · disk {} rec / {} · retries {} · failures {}",
        pct(r.host_hit),
        bytes(r.host_out),
        bytes(r.host_in),
        count(s.host_memory_records),
        bytes_total(s.host_memory_bytes),
        count(s.host_disk_records),
        bytes_total(s.host_disk_bytes),
        count(s.host_retry_success + s.host_retry_abandoned),
        count(s.host_store_failed + s.host_fetch_failed)
    ));
    lines.push(format!(
        "gpu         busy {} · readiness {} · scheduler {} · media {}",
        pct(r.gpu_busy),
        ms(r.readiness_ms),
        rate_unit(r.scheduler_exec, "exec/s"),
        if s.input.present {
            format!(
                "{} images · payload hit {}",
                count(s.input.decoded_images),
                pct(s.input.hit_rate())
            )
        } else {
            "—".into()
        }
    ));
    if let Some(e) = s.engram {
        lines.push(format!(
            "engram      gather {} × {}/s · row hit {} · disk {} · wait {} · cache {} rec / {}",
            ms(e.gather_ms()),
            rate(r.engram_gathers),
            pct(r.engram_hit),
            bytes(r.disk_bytes),
            pct(r.engram_disk_wait),
            count(e.resident_rows),
            bytes_total(e.cache_bytes)
        ));
    }
    lines.push(format!(
        "health      failed {} · cancelled {} · preempt {} · mixed grammar {} · ingress {}/{} · frontend {}/{} · http {}",
        rate_unit(r.failed, "/s"),
        rate_unit(r.cancelled, "/s"),
        rate_unit(r.preempted, "/s"),
        rate_unit(r.mixed, "/s"),
        s.http_inflight,
        s.http_capacity,
        s.frontend_active,
        s.frontend_capacity,
        rate_unit(r.http, "req/s")
    ));
    lines.push(format!(
        "peaks       prefill {} · decode {} · single {} · GPU busy {} · accept {} · depth {}",
        rate_unit(peaks.prefill, "tok/s"),
        rate_unit(peaks.generated, "tok/s"),
        rate_unit(peaks.per_stream, "tok/s"),
        pct(peaks.gpu_busy),
        num(peaks.acceptance),
        num(peaks.depth)
    ));
    lines.push(format!(
        "stalls      {} × {:.1}s across {} · constrained inflight {}",
        d.stalls[f].count,
        d.stalls[f].seconds,
        WIN_LABELS[f],
        s.constrained_max_inflight
    ));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::parse;
    use std::time::{Duration, Instant};

    fn fixture(prefill: u64, generated: u64) -> String {
        format!(
            "# TYPE flare_prefill_tokens_total counter\nflare_prefill_tokens_total {prefill}\n\
             # TYPE flare_generated_tokens_total counter\nflare_generated_tokens_total {generated}\n\
             # TYPE flare_active_requests gauge\nflare_active_requests 1\n\
             # TYPE flare_free_request_slots gauge\nflare_free_request_slots 3\n\
             # TYPE flare_total_kv_tokens gauge\nflare_total_kv_tokens 1000\n\
             # TYPE flare_free_kv_tokens gauge\nflare_free_kv_tokens 250\n\
             # TYPE flare_cached_kv_tokens gauge\nflare_cached_kv_tokens 500\n\
             # TYPE flare_uptime_seconds gauge\nflare_uptime_seconds 3661\n\
             # TYPE flare_http_requests_total counter\nflare_http_requests_total{{api=\"chat\"}} 7\n"
        )
    }

    #[test]
    fn once_output_is_one_line_per_fact_and_sanitizes_headline() {
        let mut h = History::default();
        let t0 = Instant::now();
        h.push(t0, parse(&fixture(0, 0)).unwrap());
        h.push(
            t0 + Duration::from_secs(1),
            parse(&fixture(1500, 400)).unwrap(),
        );
        let mut peaks = derive::Peaks::default();
        let text = snapshot_text(&h, &mut peaks, Some("model\u{1b}[2J"), "http://x").unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert!(lines[0].contains("flare-top http://x"));
        assert!(!lines[0].contains('\u{1b}'));
        assert!(lines[0].contains("\\x1b"));
        assert!(text.contains("prefill 1.5k tok/s"));
        assert!(text.contains("decode 400.0 tok/s") || text.contains("decode 400 tok/s"));
        assert!(text.contains("KV 500 cached / 250 free / 1.0k tok"));
        assert!(text.contains("active 1"));
    }
}
