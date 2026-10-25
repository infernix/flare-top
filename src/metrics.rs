use std::collections::BTreeMap;

/// Identity of one time series: metric family name plus its sorted labels.
/// Histograms fold `_bucket`/`_sum`/`_count` into the parent family and strip
/// the `le` label.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SeriesKey {
    pub name: String,
    pub labels: Vec<(String, String)>,
}

impl SeriesKey {
    pub fn has_label(&self, key: &str, value: &str) -> bool {
        self.labels.iter().any(|(k, v)| k == key && v == value)
    }
}

impl std::fmt::Display for SeriesKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)?;
        for (i, (k, v)) in self.labels.iter().enumerate() {
            write!(f, "{}{k}=\"{v}\"", if i == 0 { "{" } else { "," })?;
        }
        if !self.labels.is_empty() {
            write!(f, "}}")?;
        }
        Ok(())
    }
}

/// Cumulative histogram state at one scrape.
#[derive(Debug, Clone, Default)]
pub struct HistSeries {
    /// Upper bounds, ascending; the trailing bound is `+Inf`.
    pub le: Vec<f64>,
    /// Cumulative counts, parallel to `le`.
    pub counts: Vec<f64>,
    pub count: u64,
    pub sum: f64,
}

/// One scrape: simple gauge/counter series plus histogram families.
#[derive(Debug, Clone, Default)]
pub struct Sample {
    pub simple: BTreeMap<SeriesKey, f64>,
    pub hist: BTreeMap<SeriesKey, HistSeries>,
    /// Sum of every simple series in each metric family. Family rates use
    /// this instead of rescanning every labelled series per metric.
    pub family: BTreeMap<String, f64>,
}

impl Sample {
    /// Latest value of the first unlabelled series with this name.
    pub fn value(&self, name: &str) -> Option<f64> {
        self.simple
            .iter()
            .find(|(k, _)| k.name == name && k.labels.is_empty())
            .map(|(_, v)| *v)
            .or_else(|| self.simple.iter().find(|(k, _)| k.name == name).map(|(_, v)| *v))
    }

    /// Latest value of the series with this name and label.
    pub fn labeled(&self, name: &str, key: &str, value: &str) -> Option<f64> {
        self.simple
            .iter()
            .find(|(k, _)| k.name == name && k.has_label(key, value))
            .map(|(_, v)| *v)
    }

    /// Sum of every series with this family name; `None` when absent.
    pub fn sum(&self, name: &str) -> Option<f64> {
        let mut sum = 0.0;
        let mut any = false;
        for (k, v) in &self.simple {
            if k.name == name {
                sum += v;
                any = true;
            }
        }
        any.then_some(sum)
    }

    pub fn has_simple(&self, name: &str) -> bool {
        self.family.contains_key(name)
    }

    /// Sum of every series in a metric family at this scrape.
    pub fn family_sum(&self, name: &str) -> Option<f64> {
        self.family.get(name).copied()
    }

    pub fn has_hist(&self, name: &str) -> bool {
        self.hist.keys().any(|k| k.name == name)
    }

    /// Mean of a histogram family, from cumulative `_sum`/`_count`.
    pub fn hist_mean(&self, name: &str) -> Option<f64> {
        let mut sum = 0.0;
        let mut count = 0u64;
        for (k, h) in &self.hist {
            if k.name == name {
                sum += h.sum;
                count = count.saturating_add(h.count);
            }
        }
        (count > 0).then(|| sum / count as f64)
    }

    /// Cumulative p-quantile of a histogram family, merging label
    /// combinations by bucket bound. Returns the bucket's upper bound.
    pub fn hist_quantile(&self, name: &str, q: f64) -> Option<f64> {
        let mut merged: BTreeMap<u64, (f64, f64)> = BTreeMap::new();
        for (k, h) in &self.hist {
            if k.name != name {
                continue;
            }
            for (&le, &count) in h.le.iter().zip(&h.counts) {
                let entry = merged.entry(le.to_bits()).or_insert((le, 0.0));
                entry.1 += count;
            }
        }
        let total = merged.values().map(|(_, c)| *c).fold(0.0, f64::max);
        if total <= 0.0 {
            return None;
        }
        let target = total * q.clamp(0.0, 1.0);
        merged
            .values()
            .find(|(_, count)| *count >= target)
            .map(|(le, _)| *le)
    }
}

/// Maximum number of simple series plus histogram families accepted in one
/// scrape.
pub const SERIES_CAP: usize = 16_384;

/// Maximum number of `_bucket` records accepted in one histogram family.
pub const BUCKET_CAP: usize = 256;

fn parse_value(src: &str) -> Option<f64> {
    match src.trim() {
        "+Inf" | "Inf" | "inf" => Some(f64::INFINITY),
        "-Inf" | "-inf" => Some(f64::NEG_INFINITY),
        "NaN" | "nan" => Some(f64::NAN),
        s => s.parse().ok(),
    }
}

fn label_block_end(line: &str, open: usize) -> Option<usize> {
    let mut quoted = false;
    let mut escaped = false;
    for (i, c) in line[open + 1..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '}' if !quoted => return Some(open + 1 + i),
            _ => {}
        }
    }
    None
}

/// Parse a comma-separated Prometheus label block. Values support `\\`,
/// `\"` and `\n` escapes. Any malformed label drops the record.
fn parse_labels(src: &str) -> Option<Vec<(String, String)>> {
    let mut labels = Vec::new();
    let mut rest = src;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let eq = rest.find('=')?;
        let key = rest[..eq].trim();
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return None;
        }
        rest = rest[eq + 1..].trim_start();
        let mut chars = rest.chars();
        if chars.next()? != '"' {
            return None;
        }
        let mut value = String::new();
        let mut consumed = 1usize;
        let mut closed = false;
        let mut escaped = false;
        for c in chars {
            consumed += c.len_utf8();
            if escaped {
                value.push(match c {
                    'n' => '\n',
                    '\\' => '\\',
                    '"' => '"',
                    other => other,
                });
                escaped = false;
                continue;
            }
            match c {
                '\\' => escaped = true,
                '"' => {
                    closed = true;
                    break;
                }
                c => value.push(c),
            }
        }
        if !closed {
            return None;
        }
        labels.push((key.to_string(), value));
        rest = rest[consumed..].trim_start();
        if rest.is_empty() {
            break;
        }
        rest = rest.strip_prefix(',')?;
    }
    labels.sort();
    Some(labels)
}

fn parse_type(line: &str) -> Option<(String, String)> {
    let mut it = line.split_whitespace();
    (it.next()? == "TYPE").then_some(())?;
    Some((it.next()?.to_string(), it.next()?.to_string()))
}

/// Parse Prometheus text exposition format 0.0.4 into a [`Sample`].
///
/// Handles HELP/TYPE headers, labels with escapes, optional timestamps and
/// histogram bucket ladders. Malformed records are skipped line by line.
/// Rejects the whole payload when it exceeds [`SERIES_CAP`] series or a
/// histogram family exceeds [`BUCKET_CAP`] buckets.
pub fn parse(body: &str) -> anyhow::Result<Sample> {
    let mut types: BTreeMap<String, String> = BTreeMap::new();
    for line in body.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("# ") {
            if let Some((name, kind)) = parse_type(rest) {
                types.insert(name, kind);
            }
        }
    }

    let mut simple = BTreeMap::new();
    let mut hists: BTreeMap<SeriesKey, HistSeries> = BTreeMap::new();

    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, label_src, value_src) = match line.find('{') {
            Some(open) => match label_block_end(line, open) {
                Some(close) => (
                    line[..open].trim(),
                    Some(&line[open + 1..close]),
                    line[close + 1..].trim(),
                ),
                None => continue,
            },
            None => match line.split_once(char::is_whitespace) {
                Some((name, value)) => (name.trim(), None, value.trim()),
                None => continue,
            },
        };
        if name.is_empty() {
            continue;
        }
        let Some(value) = value_src.split_whitespace().next().and_then(parse_value) else {
            continue;
        };
        if !value.is_finite() {
            continue;
        }
        let mut labels = match label_src.map(parse_labels) {
            Some(Some(labels)) => labels,
            Some(None) => continue,
            None => Vec::new(),
        };

        let (base, part, le) = if let Some(base) = name.strip_suffix("_bucket") {
            let le = labels
                .iter()
                .find(|(k, _)| k == "le")
                .and_then(|(_, v)| parse_value(v))
                .filter(|le| le.is_finite() || *le == f64::INFINITY);
            (base, 0u8, le)
        } else if let Some(base) = name.strip_suffix("_sum") {
            (base, 1, None)
        } else if let Some(base) = name.strip_suffix("_count") {
            (base, 2, None)
        } else {
            (name, 3, None)
        };
        let is_hist = part < 3 && types.get(base).is_some_and(|kind| kind == "histogram");

        if is_hist {
            labels.retain(|(k, _)| k != "le");
            let key = SeriesKey {
                name: base.to_string(),
                labels,
            };
            if part == 0
                && hists
                    .get(&key)
                    .is_some_and(|h| h.le.len() >= BUCKET_CAP)
            {
                anyhow::bail!(
                    "endpoint too large: {key} has {} buckets (cap {BUCKET_CAP})",
                    BUCKET_CAP + 1
                );
            }
            let hist = hists.entry(key).or_default();
            match part {
                0 => {
                    if let Some(le) = le {
                        hist.le.push(le);
                        hist.counts.push(value.max(0.0));
                    }
                }
                1 => hist.sum = value.max(0.0),
                _ => hist.count = value.max(0.0) as u64,
            }
        } else {
            let key = SeriesKey {
                name: name.to_string(),
                labels,
            };
            simple.insert(key, value);
        }

        let series = simple.len() + hists.len();
        if series > SERIES_CAP {
            anyhow::bail!("endpoint too large: {series} series (cap {SERIES_CAP})");
        }
    }

    for hist in hists.values_mut() {
        let mut buckets: Vec<(f64, f64)> = hist
            .le
            .iter()
            .copied()
            .zip(hist.counts.iter().copied())
            .collect();
        buckets.sort_by(|a, b| a.0.total_cmp(&b.0));
        buckets.dedup_by(|later, earlier| later.0 == earlier.0);
        hist.le = buckets.iter().map(|(le, _)| *le).collect();
        hist.counts = buckets.iter().map(|(_, count)| *count).collect();
    }

    if simple.is_empty() && hists.is_empty() {
        anyhow::bail!("no metric series found");
    }

    let mut family: BTreeMap<String, f64> = BTreeMap::new();
    for (key, value) in &simple {
        *family.entry(key.name.clone()).or_insert(0.0) += value;
    }

    Ok(Sample {
        simple,
        hist: hists,
        family,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_labelled_counters_and_gauges_with_escapes() {
        let sample = parse(
            "# HELP flare_x_total help\n\
             # TYPE flare_x_total counter\n\
             flare_x_total{api=\"chat\",model=\"a,b}c\"} 12\n\
             # TYPE flare_active_requests gauge\n\
             flare_active_requests 3\n\
             flare_text{note=\"a\\\"b\\nc\"} 1\n",
        )
        .unwrap();
        assert_eq!(
            sample.labeled("flare_x_total", "api", "chat"),
            Some(12.0)
        );
        assert_eq!(
            sample.labeled("flare_x_total", "model", "a,b}c"),
            Some(12.0)
        );
        assert_eq!(sample.value("flare_active_requests"), Some(3.0));
        assert_eq!(sample.sum("flare_x_total"), Some(12.0));
        let text = sample
            .simple
            .keys()
            .find(|k| k.name == "flare_text")
            .unwrap();
        assert!(text.has_label("note", "a\"b\nc"));
    }

    #[test]
    fn parses_histogram_buckets_sum_and_count() {
        let sample = parse(
            "# TYPE flare_host_cache_missing_tokens histogram\n\
             flare_host_cache_missing_tokens_bucket{le=\"64\"} 1\n\
             flare_host_cache_missing_tokens_bucket{le=\"256\"} 3\n\
             flare_host_cache_missing_tokens_bucket{le=\"+Inf\"} 4\n\
             flare_host_cache_missing_tokens_sum 512\n\
             flare_host_cache_missing_tokens_count 4\n",
        )
        .unwrap();
        let h = sample
            .hist
            .values()
            .find(|_| true)
            .expect("histogram family");
        assert_eq!(h.le, vec![64.0, 256.0, f64::INFINITY]);
        assert_eq!(h.counts, vec![1.0, 3.0, 4.0]);
        assert_eq!(h.count, 4);
        assert_eq!(h.sum, 512.0);
        assert_eq!(sample.hist_mean("flare_host_cache_missing_tokens"), Some(128.0));
        assert_eq!(
            sample.hist_quantile("flare_host_cache_missing_tokens", 0.95),
            Some(f64::INFINITY)
        );
    }

    #[test]
    fn skips_malformed_and_nonfinite_records() {
        let sample = parse(
            "flare_ok 1\n\
             flare_nan NaN\n\
             flare_inf +Inf\n\
             flare_bad {oops\n\
             flare_bad_label{x=\"unterminated} 2\n\
             flare_bad_value 3 not-a-timestamp\n",
        )
        .unwrap();
        assert_eq!(sample.value("flare_ok"), Some(1.0));
        assert!(sample.value("flare_nan").is_none());
        assert!(sample.value("flare_inf").is_none());
    }

    #[test]
    fn rejects_payloads_without_series() {
        assert!(parse("").is_err());
        assert!(parse("<html>not metrics</html>").is_err());
    }

    #[test]
    fn rejects_bucket_overflow() {
        let mut body = String::from("# TYPE x histogram\n");
        for i in 0..=BUCKET_CAP {
            body.push_str(&format!("x_bucket{{le=\"{i}\"}} {i}\n"));
        }
        assert!(parse(&body).is_err());
    }
}
