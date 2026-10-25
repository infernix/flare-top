use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::metrics::{Sample, SeriesKey};

/// Number of scrapes retained. At the 1s default interval this covers ~2min,
/// comfortably more than the longest (60s) window.
pub const RING_CAP: usize = 150;

pub struct Entry {
    pub t: Instant,
    pub sample: Arc<Sample>,
}

/// Counter delta across one scrape pair: a reset (the new reading below the
/// old one) counts as rising from zero.
pub fn counter_delta(old: f64, new: f64) -> f64 {
    if new < old {
        new
    } else {
        new - old
    }
}

/// Rolling in-memory buffer of recent scrapes. This is the tool's only
/// storage; everything is discarded on exit.
#[derive(Default)]
pub struct History {
    buf: VecDeque<Entry>,
    pub first_seen: Option<Instant>,
    /// Cached 60s window scan, rebuilt on every push.
    scan: Option<crate::derive::Graphs>,
}

impl History {
    pub fn push(&mut self, t: Instant, sample: Sample) {
        if let Some(last) = self.buf.back() {
            if t <= last.t {
                return;
            }
        }
        if self.first_seen.is_none() {
            self.first_seen = Some(t);
        }
        if self.buf.len() == RING_CAP {
            self.buf.pop_front();
        }
        self.buf.push_back(Entry {
            t,
            sample: Arc::new(sample),
        });
        self.scan = Some(crate::derive::scan_window(self));
    }

    pub fn scan(&self) -> Option<&crate::derive::Graphs> {
        self.scan.as_ref()
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn last(&self) -> Option<&Entry> {
        self.buf.back()
    }

    pub fn entries(&self) -> &VecDeque<Entry> {
        &self.buf
    }

    /// Index of the oldest sample still inside `window`, counting back from
    /// the most recent sample.
    fn window_base(&self, window: Duration) -> Option<usize> {
        let cutoff = self.buf.back()?.t.checked_sub(window)?;
        Some(self.buf.iter().position(|e| e.t >= cutoff).unwrap_or(0))
    }

    /// Sum of per-series rates over `window` for every series matching
    /// `pred`. A series absent from the earlier sample of a pair starts as a
    /// new series and pairs nothing, so no fabricated spike enters the sum.
    pub fn rate_sum<F>(&self, pred: F, window: Duration) -> Option<f64>
    where
        F: Fn(&SeriesKey) -> bool,
    {
        let base = self.window_base(window)?;
        if self.buf.len() - base < 2 {
            return None;
        }
        let mut delta_sum = 0.0;
        let mut any = false;
        for pair in self.buf.range(base..).collect::<Vec<_>>().windows(2) {
            let dt = (pair[1].t - pair[0].t).as_secs_f64();
            if dt <= 0.0 {
                continue;
            }
            for (key, new) in &pair[1].sample.simple {
                if !pred(key) {
                    continue;
                }
                let Some(old) = pair[0].sample.simple.get(key) else {
                    continue;
                };
                delta_sum += counter_delta(*old, *new);
                any = true;
            }
        }
        let dt_total = (self.buf.back()?.t - self.buf[base].t).as_secs_f64();
        if dt_total <= 0.0 || !any {
            return None;
        }
        Some(delta_sum / dt_total)
    }

    /// Sum of one metric family's rates over `window`. Uses the parse-time
    /// family sums, so the cost is one pass over scrape pairs, not over every
    /// labelled series for every metric.
    pub fn rate_family(&self, name: &str, window: Duration) -> Option<f64> {
        let base = self.window_base(window)?;
        if self.buf.len() - base < 2 {
            return None;
        }
        let mut delta_sum = 0.0;
        let mut any = false;
        for pair in self.buf.range(base..).collect::<Vec<_>>().windows(2) {
            let dt = (pair[1].t - pair[0].t).as_secs_f64();
            if dt <= 0.0 {
                continue;
            }
            let (Some(old), Some(new)) = (
                pair[0].sample.family_sum(name),
                pair[1].sample.family_sum(name),
            ) else {
                continue;
            };
            delta_sum += counter_delta(old, new);
            any = true;
        }
        let dt_total = (self.buf.back()?.t - self.buf[base].t).as_secs_f64();
        if dt_total <= 0.0 || !any {
            return None;
        }
        Some(delta_sum / dt_total)
    }

    /// Time-weighted mean of an unlabelled gauge over `window`. With fewer
    /// than two samples it returns the latest value. Missing endpoints skip
    /// that interval, so a gap never reads as zero.
    pub fn gauge_time_mean(&self, name: &str, window: Duration) -> Option<f64> {
        let base = self.window_base(window)?;
        if self.buf.len() - base < 2 {
            return self.buf.back()?.sample.value(name);
        }
        let mut sum = 0.0;
        let mut elapsed = 0.0;
        for pair in self.buf.range(base..).collect::<Vec<_>>().windows(2) {
            let dt = (pair[1].t - pair[0].t).as_secs_f64();
            if dt <= 0.0 {
                continue;
            }
            let (Some(old), Some(new)) = (
                pair[0].sample.value(name),
                pair[1].sample.value(name),
            ) else {
                continue;
            };
            sum += (old + new) * 0.5 * dt;
            elapsed += dt;
        }
        (elapsed > 0.0).then(|| sum / elapsed)
    }

    /// Latest value of the first series matching `pred`.
    pub fn gauge_pred<F>(&self, pred: F) -> Option<f64>
    where
        F: Fn(&SeriesKey) -> bool,
    {
        self.buf
            .back()?
            .sample
            .simple
            .iter()
            .find(|(k, _)| pred(k))
            .map(|(_, v)| *v)
    }

    /// Sum of the latest values of every series matching `pred`.
    pub fn sum_gauge<F>(&self, pred: F) -> Option<f64>
    where
        F: Fn(&SeriesKey) -> bool,
    {
        let last = self.buf.back()?;
        let mut sum = 0.0;
        let mut any = false;
        for (k, v) in &last.sample.simple {
            if pred(k) {
                sum += v;
                any = true;
            }
        }
        any.then_some(sum)
    }

    /// Mean of a histogram family from the latest cumulative snapshot.
    pub fn hist_mean(&self, name: &str) -> Option<f64> {
        self.buf.back()?.sample.hist_mean(name)
    }

    /// Quantile of a histogram family from the latest cumulative snapshot.
    pub fn hist_quantile(&self, name: &str, q: f64) -> Option<f64> {
        self.buf.back()?.sample.hist_quantile(name, q)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::parse;

    fn sample(value: &str) -> Sample {
        parse(value).unwrap()
    }

    #[test]
    fn sums_rates_across_label_series_and_skips_first_appearance() {
        let mut h = History::default();
        let t0 = Instant::now();
        h.push(t0, sample("flare_x_total{a=\"1\"} 10\n"));
        h.push(
            t0 + Duration::from_secs(1),
            sample("flare_x_total{a=\"1\"} 15\nflare_x_total{a=\"2\"} 100\n"),
        );
        h.push(
            t0 + Duration::from_secs(2),
            sample("flare_x_total{a=\"1\"} 25\nflare_x_total{a=\"2\"} 110\n"),
        );
        // a1: 5 + 10; a2: 10 after its first appearance. 25 over 2s.
        assert_eq!(h.rate_sum(|k| k.name == "flare_x_total", Duration::from_secs(60)), Some(12.5));
    }

    #[test]
    fn counter_reset_counts_as_rising_from_zero() {
        let mut h = History::default();
        let t0 = Instant::now();
        h.push(t0, sample("flare_x_total 100\n"));
        h.push(t0 + Duration::from_secs(1), sample("flare_x_total 5\n"));
        assert_eq!(h.rate_sum(|k| k.name == "flare_x_total", Duration::from_secs(60)), Some(5.0));
    }

    #[test]
    fn family_rates_use_parse_time_sums() {
        let mut h = History::default();
        let t0 = Instant::now();
        h.push(
            t0,
            sample("flare_x_total{a=\"1\"} 10\nflare_x_total{a=\"2\"} 100\n"),
        );
        h.push(
            t0 + Duration::from_secs(2),
            sample("flare_x_total{a=\"1\"} 30\nflare_x_total{a=\"2\"} 104\n"),
        );
        assert_eq!(
            h.rate_family("flare_x_total", Duration::from_secs(60)),
            Some(12.0)
        );
    }

    #[test]
    fn time_weighted_gauge_mean_uses_sample_spans() {
        let mut h = History::default();
        let t0 = Instant::now();
        h.push(t0, sample("flare_active_requests 2\n"));
        h.push(
            t0 + Duration::from_secs(3),
            sample("flare_active_requests 4\n"),
        );
        assert_eq!(
            h.gauge_time_mean("flare_active_requests", Duration::from_secs(60)),
            Some(3.0)
        );
    }

    #[test]
    fn gauge_reads_latest_sample() {
        let mut h = History::default();
        h.push(Instant::now(), sample("flare_pending_requests 2\n"));
        assert_eq!(h.gauge_pred(|k| k.name == "flare_pending_requests"), Some(2.0));
    }
}
