use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use crate::history::History;
use crate::metrics;

/// Upper bound on one scrape response. FLARE's full exposition is well under
/// a megabyte; the cap keeps a hostile or broken endpoint from exhausting
/// memory before the parser's own series cap applies.
pub const MAX_BODY_BYTES: u64 = 64 * 1024 * 1024;

pub struct Shared {
    pub history: Mutex<History>,
    pub last_ok: Mutex<Option<Instant>>,
    pub last_error: Mutex<Option<String>>,
    pub paused: AtomicBool,
    /// scraper poll interval, live-adjustable from the TUI
    pub interval_ms: std::sync::atomic::AtomicU64,
    /// session peak rates
    pub peaks: Mutex<crate::derive::Peaks>,
    /// model id from `/v1/models`, when that endpoint is readable
    pub model: Mutex<Option<String>>,
    /// latest fully derived view, rebuilt once per successful scrape
    pub derived: Mutex<Option<Arc<crate::derive::Derived>>>,
    /// first successful sample, for the header before uptime is known
    pub first_seen: Mutex<Option<Instant>>,
}

impl Shared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            history: Mutex::new(History::default()),
            last_ok: Mutex::new(None),
            last_error: Mutex::new(None),
            paused: AtomicBool::new(false),
            interval_ms: std::sync::atomic::AtomicU64::new(1000),
            peaks: Mutex::new(crate::derive::Peaks::default()),
            model: Mutex::new(None),
            derived: Mutex::new(None),
            first_seen: Mutex::new(None),
        })
    }
}

fn agent(insecure: bool, timeout: Duration) -> ureq::Agent {
    let builder = ureq::AgentBuilder::new().timeout(timeout);
    if insecure {
        let verifier: Arc<dyn rustls::client::danger::ServerCertVerifier> =
            Arc::new(danger::NoVerifier);
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let cfg = rustls::ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(rustls::ALL_VERSIONS)
            .expect("tls versions")
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth();
        builder.tls_config(Arc::new(cfg)).build()
    } else {
        builder.build()
    }
}

mod danger {
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::{DigitallySignedStruct, Error, SignatureScheme};

    /// Accepts every server certificate. Only reachable via `--insecure`,
    /// which exists for homelab endpoints behind self-signed certs.
    #[derive(Debug)]
    pub struct NoVerifier;

    impl ServerCertVerifier for NoVerifier {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp_response: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            verify(message, cert, dss)
        }

        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            verify(message, cert, dss)
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            rustls::crypto::ring::default_provider()
                .signature_verification_algorithms
                .supported_schemes()
        }
    }

    fn verify(
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        let algs = &rustls::crypto::ring::default_provider().signature_verification_algorithms;
        rustls::crypto::verify_tls12_signature(message, cert, dss, algs)
            .or_else(|_| rustls::crypto::verify_tls13_signature(message, cert, dss, algs))
    }
}

/// Normalize a user-supplied base URL into `/metrics` and `/v1/models` URLs.
/// A URL already ending in `/metrics` is accepted as-is.
pub fn urls(base: &str) -> (String, String) {
    let base = base.trim_end_matches('/');
    let base = base.strip_suffix("/metrics").unwrap_or(base);
    (format!("{base}/metrics"), format!("{base}/v1/models"))
}

/// Normalize a user-supplied base URL into a full `/metrics` URL.
pub fn metrics_url(base: &str) -> String {
    urls(base).0
}

/// Upper bound, in visible characters, of a display-bound error message.
const MAX_ERROR_CHARS: usize = 256;

/// Rewrites an error message so it is safe to print to a terminal.
/// Control characters become visible escape notation and the result is
/// hard-truncated, because parser errors can embed label values verbatim.
pub fn sanitize_for_terminal(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len().min(MAX_ERROR_CHARS));
    for c in msg.chars() {
        let piece = match c {
            '\n' => "\\n".to_owned(),
            '\r' => "\\r".to_owned(),
            '\t' => "\\t".to_owned(),
            c if c.is_control() => format!("\\x{:02x}", u32::from(c)),
            c => c.to_string(),
        };
        if out.chars().count() + piece.chars().count() > MAX_ERROR_CHARS - 1 {
            out.push('\u{2026}');
            return out;
        }
        out.push_str(&piece);
    }
    out
}

fn get(
    agent: &ureq::Agent,
    url: &str,
    api_key: Option<&str>,
    accept: &str,
) -> Result<String> {
    let mut request = agent
        .get(url)
        .set("Accept", accept)
        .set("User-Agent", concat!("flare-top/", env!("CARGO_PKG_VERSION")));
    if let Some(key) = api_key {
        request = request.set("Authorization", &format!("Bearer {key}"));
    }
    let response = request.call().with_context(|| format!("GET {url}"))?;
    let mut body = Vec::new();
    response
        .into_reader()
        .take(MAX_BODY_BYTES + 1)
        .read_to_end(&mut body)
        .with_context(|| format!("reading {url}"))?;
    if body.len() as u64 > MAX_BODY_BYTES {
        bail!("response body exceeds {MAX_BODY_BYTES} bytes");
    }
    String::from_utf8(body).with_context(|| format!("{url} returned non-UTF-8 data"))
}

pub fn fetch_once(url: &str, api_key: Option<&str>, insecure: bool) -> Result<String> {
    let agent = agent(insecure, Duration::from_secs(5));
    get(&agent, url, api_key, "text/plain")
}

fn fetch_model(agent: &ureq::Agent, url: &str, api_key: Option<&str>) -> Option<String> {
    let body = get(agent, url, api_key, "application/json").ok()?;
    let value: serde_json::Value = serde_json::from_str(&body).ok()?;
    value
        .get("data")?
        .as_array()?
        .first()?
        .get("id")?
        .as_str()
        .map(str::to_owned)
}

pub fn fetch_model_once(base: &str, api_key: Option<&str>, insecure: bool) -> Option<String> {
    let (_, models) = urls(base);
    let agent = agent(insecure, Duration::from_secs(5));
    fetch_model(&agent, &models, api_key)
}

pub fn spawn_scraper(
    shared: Arc<Shared>,
    base: String,
    api_key: Option<String>,
    insecure: bool,
) {
    let (url, models) = urls(&base);
    // Model discovery is independent of the scrape loop: /v1/models may be
    // key-protected or slow, and must not delay the first metrics sample.
    {
        let shared = shared.clone();
        let api_key = api_key.clone();
        std::thread::spawn(move || {
            let agent = agent(insecure, Duration::from_secs(5));
            if let Some(model) = fetch_model(&agent, &models, api_key.as_deref()) {
                if let Ok(mut slot) = shared.model.lock() {
                    *slot = Some(model);
                }
            }
        });
    }
    std::thread::spawn(move || {
        let agent = agent(insecure, Duration::from_secs(5));
        loop {
            if !shared.paused.load(Ordering::Relaxed) {
                match get(&agent, &url, api_key.as_deref(), "text/plain") {
                    Ok(body) => match metrics::parse(&body) {
                        Ok(sample) => {
                            let (first_seen, derived) = if let Ok(mut history) =
                                shared.history.lock()
                            {
                                history.push(Instant::now(), sample);
                                // Lock order is history then peaks; the UI never
                                // holds either while locking the other.
                                if let Ok(mut peaks) = shared.peaks.lock() {
                                    crate::derive::update_peaks(&mut peaks, &history);
                                }
                                (
                                    history.first_seen,
                                    crate::derive::derive(&history).map(Arc::new),
                                )
                            } else {
                                (None, None)
                            };
                            if let Some(first_seen) = first_seen {
                                if let Ok(mut slot) = shared.first_seen.lock() {
                                    *slot = Some(first_seen);
                                }
                            }
                            if let Ok(mut slot) = shared.derived.lock() {
                                *slot = derived;
                            }
                            if let Ok(mut ok) = shared.last_ok.lock() {
                                *ok = Some(Instant::now());
                            }
                            if let Ok(mut error) = shared.last_error.lock() {
                                *error = None;
                            }
                        }
                        Err(error) => {
                            if let Ok(mut slot) = shared.last_error.lock() {
                                *slot = Some(sanitize_for_terminal(&format!("{error:#}")));
                            }
                        }
                    },
                    Err(error) => {
                        if let Ok(mut slot) = shared.last_error.lock() {
                            *slot = Some(sanitize_for_terminal(&format!("{error:#}")));
                        }
                    }
                }
            }
            let interval = shared.interval_ms.load(Ordering::Relaxed).clamp(500, 10_000);
            std::thread::sleep(Duration::from_millis(interval));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_base_metrics_and_models_urls() {
        assert_eq!(
            urls("http://localhost:8000"),
            (
                "http://localhost:8000/metrics".to_string(),
                "http://localhost:8000/v1/models".to_string()
            )
        );
        assert_eq!(
            urls("http://localhost:8000/"),
            (
                "http://localhost:8000/metrics".to_string(),
                "http://localhost:8000/v1/models".to_string()
            )
        );
        assert_eq!(
            urls("http://localhost:8000/metrics").0,
            "http://localhost:8000/metrics"
        );
    }

    #[test]
    fn terminal_errors_hide_control_bytes() {
        assert_eq!(
            sanitize_for_terminal("a\u{1b}[2Jb\nc"),
            "a\\x1b[2Jb\\nc"
        );
        assert!(sanitize_for_terminal(&"x".repeat(1000)).chars().count() <= MAX_ERROR_CHARS);
    }
}
