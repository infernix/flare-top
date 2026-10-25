use std::time::Duration;

use crate::history::{counter_delta, History};
use crate::metrics::{Sample, SeriesKey};

pub const WINDOWS: [Duration; 3] = [
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(60),
];

pub const WIN_LABELS: [&str; 3] = ["5s", "15s", "60s"];

pub const GRAPH_KINDS: [&str; 12] = [
    "init",
    "restore",
    "resume",
    "prefill",
    "decode",
    "vision",
    "snapshot",
    "speculative",
    "mixed",
    "proposal",
    "verification",
    "packed_speculative",
];

pub const SAMPLING_KINDS: [&str; 5] = ["prefill", "decode", "resume", "speculative", "mixed"];
pub const SCHEDULER_REASONS: [&str; 4] =
    ["ordinary", "discovery", "timing_refresh", "acceptance_learning"];
pub const HTTP_APIS: [&str; 5] = [
    "chat",
    "completions",
    "messages",
    "responses",
    "count_tokens",
];

const PREFILL: &str = "flare_prefill_tokens_total";
const GENERATED: &str = "flare_generated_tokens_total";
const ACCEPTED_DRAFTS: &str = "flare_accepted_draft_tokens_total";
const DRAFT_ROUNDS: &str = "flare_draft_request_rounds_total";
const VERIFY_DRAFTS: &str = "flare_verification_draft_tokens_total";
const VERIFY_ROUNDS: &str = "flare_verification_request_rounds_total";
const ADMITTED: &str = "flare_requests_admitted_total";
const COMPLETED: &str = "flare_requests_completed_total";
const CANCELLED: &str = "flare_requests_cancelled_total";
const FAILED: &str = "flare_requests_failed_total";
const CACHE_LOOKUPS: &str = "flare_cache_lookups_total";
const CACHE_HITS: &str = "flare_cache_hits_total";
const CACHE_MISSES: &str = "flare_cache_misses_total";
const CACHE_REUSED: &str = "flare_cache_reused_tokens_total";
const SNAPSHOT_PUBLISHED: &str = "flare_snapshot_publications_total";
const SNAPSHOT_EVICTED: &str = "flare_snapshot_evictions_total";
const PREEMPTIONS: &str = "flare_preemptions_total";
const HOST_LOOKUPS: &str = "flare_host_cache_lookups_total";
const HOST_HITS: &str = "flare_host_cache_hits_total";
const HOST_REUSED: &str = "flare_host_cache_reused_tokens_total";
const HOST_BYTES_OUT: &str = "flare_host_cache_bytes_out_total";
const HOST_BYTES_IN: &str = "flare_host_cache_bytes_in_total";
const HOST_STORED: &str = "flare_host_cache_stored_total";
const HOST_STORE_FAILED: &str = "flare_host_cache_store_failed_total";
const HOST_FETCHED: &str = "flare_host_cache_fetched_total";
const HOST_FETCH_FAILED: &str = "flare_host_cache_fetch_failed_total";
const REPLAY_GPU: &str = "flare_replay_gpu_nanoseconds_total";
const READINESS_WAIT: &str = "flare_replay_readiness_wait_nanoseconds_total";
const READINESS_SAMPLES: &str = "flare_replay_readiness_samples_total";
const SPEC_ROUNDS: &str = "flare_speculative_rounds_total";
const SPEC_GPU: &str = "flare_speculative_gpu_nanoseconds_total";
const SPEC_EMITTED: &str = "flare_speculative_emitted_tokens_total";
const SPEC_OFFERED: &str = "flare_speculative_offered_drafts_total";
const PROPOSAL_ROUNDS: &str = "flare_proposal_rounds_total";
const PROPOSAL_GPU: &str = "flare_proposal_gpu_nanoseconds_total";
const ENGRAM_GATHERS: &str = "flare_engram_gathers_total";
const ENGRAM_GATHER_NS: &str = "flare_engram_gather_nanoseconds_total";
const ENGRAM_DISK_BYTES: &str = "flare_engram_disk_bytes_total";
const ENGRAM_DISK_WAIT_NS: &str = "flare_engram_disk_wait_nanoseconds_total";
const ENGRAM_HITS: &str = "flare_engram_row_cache_hits_total";
const ENGRAM_MISSES: &str = "flare_engram_row_cache_misses_total";

fn v(sample: &Sample, name: &str) -> u64 {
    sample.value(name).unwrap_or(0.0).max(0.0) as u64
}

fn opt(sample: &Sample, name: &str) -> Option<u64> {
    sample.value(name).map(|value| value.max(0.0) as u64)
}

fn labeled(sample: &Sample, name: &str, key: &str, value: &str) -> u64 {
    sample.labeled(name, key, value).unwrap_or(0.0).max(0.0) as u64
}

fn total(sample: &Sample, name: &str) -> u64 {
    sample.sum(name).unwrap_or(0.0).max(0.0) as u64
}

fn has_prefix(sample: &Sample, prefix: &str) -> bool {
    sample.simple.keys().any(|k| k.name.starts_with(prefix))
        || sample.hist.keys().any(|k| k.name.starts_with(prefix))
}

#[derive(Clone, Copy, Debug, Default)]
pub struct InputCache {
    pub present: bool,
    pub decoded_images: u64,
    pub decoded_source_bytes: u64,
    pub decoded_source_capacity: u64,
    pub metadata_hits: u64,
    pub metadata_misses: u64,
    pub payload_hits: u64,
    pub payload_misses: u64,
    pub memory_hits: u64,
    pub disk_hits: u64,
    pub failures: u64,
    pub stored: u64,
    pub skipped: u64,
    pub queued: u64,
    pub pending_writes: u64,
    pub staging_bytes: u64,
    pub staging_limit: u64,
    pub index_bytes: u64,
    pub memory_limit: u64,
    pub memory_records: u64,
    pub memory_bytes: u64,
    pub disk_records: u64,
    pub disk_bytes: u64,
}

impl InputCache {
    fn from(sample: &Sample) -> Self {
        Self {
            present: sample.has_simple("flare_input_cache_metadata_hits_total"),
            decoded_images: v(sample, "flare_input_decoded_images_total"),
            decoded_source_bytes: v(sample, "flare_input_decoded_source_bytes"),
            decoded_source_capacity: v(sample, "flare_input_decoded_source_capacity"),
            metadata_hits: v(sample, "flare_input_cache_metadata_hits_total"),
            metadata_misses: v(sample, "flare_input_cache_metadata_misses_total"),
            payload_hits: v(sample, "flare_input_cache_payload_hits_total"),
            payload_misses: v(sample, "flare_input_cache_payload_misses_total"),
            memory_hits: v(sample, "flare_input_cache_memory_hits_total"),
            disk_hits: v(sample, "flare_input_cache_disk_hits_total"),
            failures: v(sample, "flare_input_cache_failures_total"),
            stored: v(sample, "flare_input_cache_stored_total"),
            skipped: v(sample, "flare_input_cache_skipped_total"),
            queued: v(sample, "flare_input_cache_queued"),
            pending_writes: v(sample, "flare_input_cache_pending_writes"),
            staging_bytes: v(sample, "flare_input_cache_staging_bytes"),
            staging_limit: v(sample, "flare_input_cache_staging_limit"),
            index_bytes: v(sample, "flare_input_cache_index_bytes"),
            memory_limit: v(sample, "flare_input_cache_memory_limit"),
            memory_records: v(sample, "flare_input_cache_memory_records"),
            memory_bytes: v(sample, "flare_input_cache_memory_bytes"),
            disk_records: v(sample, "flare_input_cache_disk_records"),
            disk_bytes: v(sample, "flare_input_cache_disk_bytes"),
        }
    }

    pub fn hit_rate(&self) -> Option<f64> {
        let hits = self.payload_hits + self.metadata_hits;
        let misses = self.payload_misses + self.metadata_misses;
        (hits + misses > 0).then(|| hits as f64 / (hits + misses) as f64)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Engram {
    pub gathers: u64,
    pub gather_ns: u64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub resident_rows: u64,
    pub capacity_rows: u64,
    pub cache_bytes: u64,
    pub resident_scale_bytes: u64,
    pub disk_reads: u64,
    pub disk_bytes: u64,
    pub disk_plan_ns: u64,
    pub disk_submit_ns: u64,
    pub disk_wait_ns: u64,
    pub disk_complete_ns: u64,
}

impl Engram {
    pub fn gather_ms(&self) -> Option<f64> {
        (self.gathers > 0).then(|| self.gather_ns as f64 / self.gathers as f64 / 1e6)
    }
}

/// Point-in-time counters and gauges read from one FLARE `/metrics` scrape.
#[derive(Clone, Copy, Debug, Default)]
pub struct Flare {
    pub active: u64,
    pub pending: u64,
    pub free_slots: u64,
    pub retiring_slots: u64,
    pub http_inflight: u64,
    pub http_capacity: u64,
    pub frontend_active: u64,
    pub frontend_capacity: u64,

    pub prefill_tokens: u64,
    pub generated_tokens: u64,
    pub accepted_drafts: u64,
    pub draft_rounds: u64,
    pub verification_drafts: u64,
    pub verification_rounds: u64,

    pub admitted: u64,
    pub completed: u64,
    pub cancelled: u64,
    pub failed: u64,

    pub cache_lookups: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_reused_tokens: u64,
    pub snapshot_publications: u64,
    pub snapshot_evictions: u64,
    pub message_checkpoints: u64,
    pub prompt_checkpoints: u64,
    pub cached_checkpoints: u64,
    pub preemptions: u64,

    pub total_pages: u64,
    pub free_pages: u64,
    pub cached_pages: u64,
    pub page_tokens: u64,
    pub total_kv_tokens: u64,
    pub free_kv_tokens: u64,
    pub cached_kv_tokens: u64,
    pub snapshot_capacity: u64,
    pub free_snapshots: u64,
    pub snapshot_backing_pages: u64,

    pub host_lookups: u64,
    pub host_hits: u64,
    pub host_reused_tokens: u64,
    pub host_row_refills: u64,
    pub host_retry_success: u64,
    pub host_retry_abandoned: u64,
    pub host_stored: u64,
    pub host_store_skipped: u64,
    pub host_store_failed: u64,
    pub host_fetched: u64,
    pub host_fetch_failed: u64,
    pub host_bytes_out: u64,
    pub host_bytes_in: u64,
    pub host_memory_records: u64,
    pub host_memory_bytes: u64,
    pub host_disk_records: u64,
    pub host_disk_bytes: u64,
    pub host_outbound_rate: u64,
    pub host_inbound_rate: u64,
    pub host_outbound_in_flight: u64,
    pub host_inbound_in_flight: u64,
    pub host_inbound_waiting: u64,
    pub host_lookup_no_match: u64,
    pub host_lookup_rejected: u64,
    pub host_lookup_admitted: u64,
    pub host_store_present: u64,
    pub host_store_retry: u64,
    pub host_store_oversize: u64,

    pub replay_gpu_ns: u64,
    pub readiness_wait_ns: u64,
    pub readiness_samples: u64,

    pub spec_rounds: u64,
    pub spec_gpu_ns: u64,
    pub spec_emitted: u64,
    pub spec_offered: u64,
    pub proposal_rounds: u64,
    pub proposal_gpu_ns: u64,
    pub history_drafts: bool,

    pub graph_launches: [u64; 12],
    pub sampling_launches: [u64; 5],
    pub scheduler_ns: [u64; 4],
    pub scheduler_executions: [u64; 4],
    pub http_requests: [u64; 5],

    pub uptime_seconds: u64,
    pub generated_last_minute: u64,
    pub decode_weight_bytes: u64,
    pub mixed_grammar_steps: u64,
    pub constrained_max_inflight: u64,

    pub engram: Option<Engram>,
    pub input: InputCache,
}

impl Flare {
    pub fn from(sample: &Sample) -> Self {
        let total_kv_tokens = opt(sample, "flare_total_kv_tokens");
        let free_kv_tokens = opt(sample, "flare_free_kv_tokens");
        let cached_kv_tokens = opt(sample, "flare_cached_kv_tokens");
        let page_tokens = v(sample, "flare_kv_page_tokens");
        let total_pages = v(sample, "flare_total_kv_pages");
        let free_pages = v(sample, "flare_free_kv_pages");
        let cached_pages = v(sample, "flare_cached_kv_pages");
        let engram = has_prefix(sample, "flare_engram_").then(|| Engram {
            gathers: v(sample, ENGRAM_GATHERS),
            gather_ns: v(sample, ENGRAM_GATHER_NS),
            hits: v(sample, ENGRAM_HITS),
            misses: v(sample, ENGRAM_MISSES),
            evictions: v(sample, "flare_engram_row_cache_evictions_total"),
            resident_rows: v(sample, "flare_engram_row_cache_resident_rows"),
            capacity_rows: v(sample, "flare_engram_row_cache_capacity_rows"),
            cache_bytes: v(sample, "flare_engram_row_cache_bytes"),
            resident_scale_bytes: v(sample, "flare_engram_resident_scale_bytes"),
            disk_reads: v(sample, "flare_engram_disk_reads_total"),
            disk_bytes: v(sample, ENGRAM_DISK_BYTES),
            disk_plan_ns: v(sample, "flare_engram_disk_plan_nanoseconds_total"),
            disk_submit_ns: v(sample, "flare_engram_disk_submit_nanoseconds_total"),
            disk_wait_ns: v(sample, ENGRAM_DISK_WAIT_NS),
            disk_complete_ns: v(sample, "flare_engram_disk_complete_nanoseconds_total"),
        });
        let mut graph_launches = [0; 12];
        for (slot, kind) in graph_launches.iter_mut().zip(GRAPH_KINDS) {
            *slot = labeled(sample, "flare_graph_launches_total", "kind", kind);
        }
        let mut sampling_launches = [0; 5];
        for (slot, kind) in sampling_launches.iter_mut().zip(SAMPLING_KINDS) {
            *slot = labeled(sample, "flare_sampling_graph_launches_total", "kind", kind);
        }
        let mut scheduler_ns = [0; 4];
        let mut scheduler_executions = [0; 4];
        for (i, reason) in SCHEDULER_REASONS.into_iter().enumerate() {
            scheduler_ns[i] = labeled(
                sample,
                "flare_scheduler_execution_nanoseconds_total",
                "reason",
                reason,
            );
            scheduler_executions[i] =
                labeled(sample, "flare_scheduler_executions_total", "reason", reason);
        }
        let mut http_requests = [0; 5];
        for (slot, api) in http_requests.iter_mut().zip(HTTP_APIS) {
            *slot = labeled(sample, "flare_http_requests_total", "api", api);
        }
        Self {
            active: v(sample, "flare_active_requests"),
            pending: v(sample, "flare_pending_requests"),
            free_slots: v(sample, "flare_free_request_slots"),
            retiring_slots: v(sample, "flare_retiring_request_slots"),
            http_inflight: v(sample, "flare_http_inflight_requests"),
            http_capacity: v(sample, "flare_http_request_capacity"),
            frontend_active: v(sample, "flare_frontend_active_workers"),
            frontend_capacity: v(sample, "flare_frontend_worker_capacity"),
            prefill_tokens: v(sample, PREFILL),
            generated_tokens: v(sample, GENERATED),
            accepted_drafts: v(sample, ACCEPTED_DRAFTS),
            draft_rounds: v(sample, DRAFT_ROUNDS),
            verification_drafts: v(sample, VERIFY_DRAFTS),
            verification_rounds: v(sample, VERIFY_ROUNDS),
            admitted: v(sample, ADMITTED),
            completed: v(sample, COMPLETED),
            cancelled: v(sample, CANCELLED),
            failed: v(sample, FAILED),
            cache_lookups: v(sample, CACHE_LOOKUPS),
            cache_hits: v(sample, CACHE_HITS),
            cache_misses: v(sample, CACHE_MISSES),
            cache_reused_tokens: v(sample, CACHE_REUSED),
            snapshot_publications: v(sample, SNAPSHOT_PUBLISHED),
            snapshot_evictions: v(sample, SNAPSHOT_EVICTED),
            message_checkpoints: v(sample, "flare_message_checkpoints_total"),
            prompt_checkpoints: v(sample, "flare_prompt_checkpoints_total"),
            cached_checkpoints: v(sample, "flare_cached_checkpoints"),
            preemptions: v(sample, PREEMPTIONS),
            total_pages,
            free_pages,
            cached_pages,
            page_tokens,
            total_kv_tokens: total_kv_tokens
                .unwrap_or_else(|| total_pages.saturating_mul(page_tokens)),
            free_kv_tokens: free_kv_tokens
                .unwrap_or_else(|| free_pages.saturating_mul(page_tokens)),
            cached_kv_tokens: cached_kv_tokens
                .unwrap_or_else(|| cached_pages.saturating_mul(page_tokens)),
            snapshot_capacity: v(sample, "flare_snapshot_capacity"),
            free_snapshots: v(sample, "flare_free_snapshots"),
            snapshot_backing_pages: v(sample, "flare_snapshot_backing_pages"),
            host_lookups: v(sample, HOST_LOOKUPS),
            host_hits: v(sample, HOST_HITS),
            host_reused_tokens: v(sample, HOST_REUSED),
            host_row_refills: v(sample, "flare_host_cache_row_refills_total"),
            host_retry_success: v(sample, "flare_host_cache_retry_success_total"),
            host_retry_abandoned: v(sample, "flare_host_cache_retry_abandoned_total"),
            host_stored: v(sample, HOST_STORED),
            host_store_skipped: v(sample, "flare_host_cache_store_skipped_total"),
            host_store_failed: v(sample, HOST_STORE_FAILED),
            host_fetched: v(sample, HOST_FETCHED),
            host_fetch_failed: v(sample, HOST_FETCH_FAILED),
            host_bytes_out: v(sample, HOST_BYTES_OUT),
            host_bytes_in: v(sample, HOST_BYTES_IN),
            host_memory_records: v(sample, "flare_host_cache_memory_records"),
            host_memory_bytes: v(sample, "flare_host_cache_memory_bytes"),
            host_disk_records: v(sample, "flare_host_cache_disk_records"),
            host_disk_bytes: v(sample, "flare_host_cache_disk_bytes"),
            host_outbound_rate: v(sample, "flare_host_cache_outbound_bytes_per_second"),
            host_inbound_rate: v(sample, "flare_host_cache_inbound_bytes_per_second"),
            host_outbound_in_flight: v(sample, "flare_host_cache_outbound_in_flight"),
            host_inbound_in_flight: v(sample, "flare_host_cache_inbound_in_flight"),
            host_inbound_waiting: v(sample, "flare_host_cache_inbound_waiting"),
            host_lookup_no_match: labeled(
                sample,
                "flare_host_cache_lookup_outcomes_total",
                "reason",
                "no_match",
            ),
            host_lookup_rejected: labeled(
                sample,
                "flare_host_cache_lookup_outcomes_total",
                "reason",
                "cost_rejected",
            ) + labeled(
                sample,
                "flare_host_cache_lookup_outcomes_total",
                "reason",
                "capacity_rejected",
            ) + labeled(
                sample,
                "flare_host_cache_lookup_outcomes_total",
                "reason",
                "enqueue_rejected",
            ),
            host_lookup_admitted: labeled(
                sample,
                "flare_host_cache_lookup_outcomes_total",
                "reason",
                "admitted",
            ),
            host_store_present: labeled(
                sample,
                "flare_host_cache_store_reasons_total",
                "reason",
                "present",
            ),
            host_store_retry: labeled(
                sample,
                "flare_host_cache_store_reasons_total",
                "reason",
                "inflight",
            ) + labeled(
                sample,
                "flare_host_cache_store_reasons_total",
                "reason",
                "slabs",
            ) + labeled(
                sample,
                "flare_host_cache_store_reasons_total",
                "reason",
                "queue",
            ) + labeled(
                sample,
                "flare_host_cache_store_reasons_total",
                "reason",
                "budget",
            ),
            host_store_oversize: labeled(
                sample,
                "flare_host_cache_store_reasons_total",
                "reason",
                "oversize",
            ),
            replay_gpu_ns: v(sample, REPLAY_GPU),
            readiness_wait_ns: v(sample, READINESS_WAIT),
            readiness_samples: v(sample, READINESS_SAMPLES),
            spec_rounds: total(sample, SPEC_ROUNDS),
            spec_gpu_ns: total(sample, SPEC_GPU),
            spec_emitted: total(sample, SPEC_EMITTED),
            spec_offered: total(sample, SPEC_OFFERED),
            proposal_rounds: total(sample, PROPOSAL_ROUNDS),
            proposal_gpu_ns: total(sample, PROPOSAL_GPU),
            history_drafts: v(sample, "flare_history_drafts_enabled") != 0,
            graph_launches,
            sampling_launches,
            scheduler_ns,
            scheduler_executions,
            http_requests,
            uptime_seconds: v(sample, "flare_uptime_seconds"),
            generated_last_minute: v(sample, "flare_generated_tokens_last_minute"),
            decode_weight_bytes: v(sample, "flare_decode_weight_bytes"),
            mixed_grammar_steps: v(sample, "flare_mixed_grammar_decode_steps_total"),
            constrained_max_inflight: v(sample, "flare_constrained_max_inflight"),
            engram,
            input: InputCache::from(sample),
        }
    }

    pub fn kv_used_tokens(&self) -> u64 {
        self.total_kv_tokens.saturating_sub(self.free_kv_tokens)
    }

    pub fn kv_usage(&self) -> Option<f64> {
        (self.total_kv_tokens > 0).then(|| self.kv_used_tokens() as f64 / self.total_kv_tokens as f64)
    }

    pub fn snapshot_usage(&self) -> Option<f64> {
        (self.snapshot_capacity > 0).then(|| {
            self.snapshot_capacity.saturating_sub(self.free_snapshots) as f64
                / self.snapshot_capacity as f64
        })
    }

    pub fn request_slots(&self) -> u64 {
        self.active
            .saturating_add(self.free_slots)
            .saturating_add(self.retiring_slots)
    }

    pub fn ingress_usage(&self) -> Option<f64> {
        (self.http_capacity > 0).then(|| self.http_inflight as f64 / self.http_capacity as f64)
    }

    pub fn frontend_usage(&self) -> Option<f64> {
        (self.frontend_capacity > 0).then(|| {
            self.frontend_active as f64 / self.frontend_capacity as f64
        })
    }

    pub fn spec_gpu_ms_per_round(&self) -> Option<f64> {
        (self.spec_rounds > 0)
            .then(|| self.spec_gpu_ns as f64 / self.spec_rounds as f64 / 1e6)
    }

    pub fn emitted_per_round(&self) -> Option<f64> {
        (self.spec_rounds > 0).then(|| self.spec_emitted as f64 / self.spec_rounds as f64)
    }

    pub fn offered_per_round(&self) -> Option<f64> {
        (self.spec_rounds > 0).then(|| self.spec_offered as f64 / self.spec_rounds as f64)
    }

    pub fn http_total(&self) -> u64 {
        self.http_requests.iter().sum()
    }

    pub fn graph_launches(&self, kind: &str) -> u64 {
        GRAPH_KINDS
            .iter()
            .position(|k| *k == kind)
            .map_or(0, |i| self.graph_launches[i])
    }

    pub fn sampling_launches(&self, kind: &str) -> u64 {
        SAMPLING_KINDS
            .iter()
            .position(|k| *k == kind)
            .map_or(0, |i| self.sampling_launches[i])
    }
}

/// Rates over one window. `None` means the counter family was absent from the
/// window; a present counter with no traffic reads `Some(0.0)`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Rates {
    pub prefill: Option<f64>,
    pub generated: Option<f64>,
    pub per_stream: Option<f64>,
    pub verify: Option<f64>,
    pub weights: Option<f64>,
    pub accepted: Option<f64>,
    pub acceptance: Option<f64>,
    pub depth: Option<f64>,
    pub gpu_busy: Option<f64>,
    pub host_out: Option<f64>,
    pub host_in: Option<f64>,
    pub disk_bytes: Option<f64>,
    pub completed: Option<f64>,
    pub failed: Option<f64>,
    pub cancelled: Option<f64>,
    pub admitted: Option<f64>,
    pub preempted: Option<f64>,
    pub reused: Option<f64>,
    pub cache_hit: Option<f64>,
    pub snapshot_published: Option<f64>,
    pub snapshot_evicted: Option<f64>,
    pub readiness_ms: Option<f64>,
    pub host_hit: Option<f64>,
    pub scheduler_exec: Option<f64>,
    pub http: Option<f64>,
    pub mixed: Option<f64>,
    pub proposal: Option<f64>,
    pub host_lookup: Option<f64>,
    pub engram_gathers: Option<f64>,
    pub engram_disk_wait: Option<f64>,
    pub engram_hit: Option<f64>,
}

fn rates(h: &History, window: Duration, flare: Flare) -> Rates {
    let r = |name: &str| h.rate_family(name, window);
    let prefill = r(PREFILL);
    let generated = r(GENERATED);
    let accepted = r(ACCEPTED_DRAFTS);
    let rounds = r(DRAFT_ROUNDS);
    let drafts = r(VERIFY_DRAFTS);
    let verifications = r(VERIFY_ROUNDS);
    let gpu_ns = r(REPLAY_GPU);
    let waits = r(READINESS_WAIT);
    let samples = r(READINESS_SAMPLES);
    let lookups = r(CACHE_LOOKUPS);
    let hits = r(CACHE_HITS);
    let host_lookups = r(HOST_LOOKUPS);
    let host_hits = r(HOST_HITS);
    let gathers = r(ENGRAM_GATHERS);
    let gather_ns = r(ENGRAM_GATHER_NS);
    let disk_wait_ns = r(ENGRAM_DISK_WAIT_NS);
    let verify = r(SPEC_ROUNDS);
    let engram_hits = r(ENGRAM_HITS);
    let engram_misses = r(ENGRAM_MISSES);
    let active = h
        .gauge_time_mean("flare_active_requests", window)
        .unwrap_or(flare.active as f64);
    Rates {
        prefill,
        generated,
        per_stream: generated.filter(|_| active > 0.0).map(|g| g / active),
        verify,
        weights: verify.map(|steps| steps * flare.decode_weight_bytes as f64),
        accepted,
        acceptance: match (accepted, rounds) {
            (Some(accepted), Some(rounds)) if rounds > 0.0 => Some(accepted / rounds),
            _ => None,
        },
        depth: match (drafts, verifications) {
            (Some(drafts), Some(rounds)) if rounds > 0.0 => Some(drafts / rounds),
            _ => None,
        },
        gpu_busy: gpu_ns.map(|ns| ns / 1e9),
        host_out: r(HOST_BYTES_OUT),
        host_in: r(HOST_BYTES_IN),
        disk_bytes: r(ENGRAM_DISK_BYTES),
        completed: r(COMPLETED),
        failed: r(FAILED),
        cancelled: r(CANCELLED),
        admitted: r(ADMITTED),
        preempted: r(PREEMPTIONS),
        reused: r(CACHE_REUSED),
        cache_hit: match (hits, lookups) {
            (Some(hits), Some(lookups)) if lookups > 0.0 => Some(hits / lookups),
            _ => None,
        },
        snapshot_published: r(SNAPSHOT_PUBLISHED),
        snapshot_evicted: r(SNAPSHOT_EVICTED),
        readiness_ms: match (waits, samples) {
            (Some(waits), Some(samples)) if samples > 0.0 => Some(waits / samples / 1e6),
            _ => None,
        },
        host_hit: match (host_hits, host_lookups) {
            (Some(hits), Some(lookups)) if lookups > 0.0 => Some(hits / lookups),
            _ => None,
        },
        scheduler_exec: r("flare_scheduler_executions_total"),
        http: r("flare_http_requests_total"),
        mixed: r("flare_mixed_grammar_decode_steps_total"),
        proposal: r(PROPOSAL_ROUNDS),
        host_lookup: host_lookups,
        engram_gathers: gathers,
        engram_disk_wait: match (disk_wait_ns, gather_ns) {
            (Some(wait), Some(gather)) if gather > 0.0 => Some(wait / gather),
            _ => None,
        },
        engram_hit: match (engram_hits, engram_misses) {
            (Some(hits), Some(misses)) if hits + misses > 0.0 => {
                Some(hits / (hits + misses))
            }
            _ => None,
        },
    }
}

/// One rate lane over the trailing 60s, one point per scrape interval.
#[derive(Clone, Default)]
pub struct Lane {
    pub vals: Vec<Option<f64>>,
}

impl Lane {
    pub fn last(&self) -> Option<f64> {
        self.vals.iter().rev().find_map(|v| *v)
    }

    pub fn peak(&self) -> Option<f64> {
        self.vals.iter().flatten().copied().reduce(f64::max)
    }
}

/// The trailing 60s window in the shape the graphs draw.
#[derive(Clone, Default)]
pub struct Graphs {
    pub prefill: Lane,
    pub generated: Lane,
    pub per_stream: Lane,
    pub active: Lane,
    pub queue: Lane,
    pub gpu_busy: Lane,
    pub host_out: Lane,
    pub host_in: Lane,
    pub acceptance: Lane,
    pub depth: Lane,
    pub verify: Lane,
    pub readiness_ms: Lane,
    pub completed: Lane,
    pub failed: Lane,
    pub reused: Lane,
    pub cache_hit: Lane,
    pub host_hit: Lane,
    pub kv_usage: Lane,
    pub engram_gathers: Lane,
    pub engram_disk_bytes: Lane,
    pub engram_disk_wait: Lane,
    pub engram_hit: Lane,
    pub stall: Vec<bool>,
    /// interval durations in seconds, parallel to every lane
    pub dt: Vec<f64>,
}

/// Per-pair deltas used by both the graph scan and the peak tracker.
fn pair_rate<F>(old: &Sample, new: &Sample, pred: F, dt: f64) -> Option<f64>
where
    F: Fn(&SeriesKey) -> bool,
{
    if dt <= 0.0 {
        return None;
    }
    let mut delta = 0.0;
    let mut any = false;
    for (key, new_value) in &new.simple {
        if !pred(key) {
            continue;
        }
        let Some(old_value) = old.simple.get(key) else {
            continue;
        };
        delta += counter_delta(*old_value, *new_value);
        any = true;
    }
    any.then(|| delta / dt)
}

fn pair_delta<F>(old: &Sample, new: &Sample, pred: F) -> Option<f64>
where
    F: Fn(&SeriesKey) -> bool,
{
    pair_rate(old, new, pred, 1.0)
}

fn ratio(numerator: Option<f64>, denominator: Option<f64>) -> Option<f64> {
    match (numerator, denominator) {
        (Some(n), Some(d)) if d > 0.0 => Some(n / d),
        _ => None,
    }
}

struct Interval {
    prefill: Option<f64>,
    generated: Option<f64>,
    per_stream: Option<f64>,
    active: Option<f64>,
    queue: Option<f64>,
    gpu_busy: Option<f64>,
    host_out: Option<f64>,
    host_in: Option<f64>,
    acceptance: Option<f64>,
    depth: Option<f64>,
    verify: Option<f64>,
    readiness_ms: Option<f64>,
    completed: Option<f64>,
    failed: Option<f64>,
    reused: Option<f64>,
    cache_hit: Option<f64>,
    host_hit: Option<f64>,
    kv_usage: Option<f64>,
    engram_gathers: Option<f64>,
    engram_disk_bytes: Option<f64>,
    engram_disk_wait: Option<f64>,
    engram_hit: Option<f64>,
}

fn kv_usage_at(sample: &Sample) -> Option<f64> {
    let page_tokens = sample.value("flare_kv_page_tokens").unwrap_or(0.0);
    let total = sample.value("flare_total_kv_tokens").or_else(|| {
        sample
            .value("flare_total_kv_pages")
            .map(|pages| pages * page_tokens)
    })?;
    let free = sample
        .value("flare_free_kv_tokens")
        .or_else(|| {
            sample
                .value("flare_free_kv_pages")
                .map(|pages| pages * page_tokens)
        })
        .unwrap_or(0.0);
    (total > 0.0).then(|| (total - free).max(0.0) / total)
}

fn interval(old: &Sample, new: &Sample, dt: f64) -> Interval {
    let prefill = pair_rate(old, new, |k| k.name == PREFILL, dt);
    let generated = pair_rate(old, new, |k| k.name == GENERATED, dt);
    let active = new.value("flare_active_requests");
    let queue = new.value("flare_pending_requests");
    let gpu = pair_rate(old, new, |k| k.name == REPLAY_GPU, dt).map(|ns| ns / 1e9);
    let acceptance = ratio(
        pair_delta(old, new, |k| k.name == ACCEPTED_DRAFTS),
        pair_delta(old, new, |k| k.name == DRAFT_ROUNDS),
    );
    let depth = ratio(
        pair_delta(old, new, |k| k.name == VERIFY_DRAFTS),
        pair_delta(old, new, |k| k.name == VERIFY_ROUNDS),
    );
    let verify = pair_rate(old, new, |k| k.name == SPEC_ROUNDS, dt);
    let readiness_ms = ratio(
        pair_rate(old, new, |k| k.name == READINESS_WAIT, dt),
        pair_rate(old, new, |k| k.name == READINESS_SAMPLES, dt),
    )
    .map(|ns| ns / 1e6);
    let cache_hit = ratio(
        pair_delta(old, new, |k| k.name == CACHE_HITS),
        pair_delta(old, new, |k| k.name == CACHE_LOOKUPS),
    );
    let host_hit = ratio(
        pair_delta(old, new, |k| k.name == HOST_HITS),
        pair_delta(old, new, |k| k.name == HOST_LOOKUPS),
    );
    let engram_hits = pair_delta(old, new, |k| k.name == ENGRAM_HITS);
    let engram_misses = pair_delta(old, new, |k| k.name == ENGRAM_MISSES);
    let engram_hit = match (engram_hits, engram_misses) {
        (Some(hits), Some(misses)) if hits + misses > 0.0 => {
            Some(hits / (hits + misses))
        }
        _ => None,
    };
    let gather_ns = pair_rate(old, new, |k| k.name == ENGRAM_GATHER_NS, dt);
    Interval {
        prefill,
        generated,
        per_stream: generated.filter(|_| active.unwrap_or(0.0) > 0.0).map(|g| {
            g / active.unwrap_or(1.0)
        }),
        active,
        queue,
        gpu_busy: gpu,
        host_out: pair_rate(old, new, |k| k.name == HOST_BYTES_OUT, dt),
        host_in: pair_rate(old, new, |k| k.name == HOST_BYTES_IN, dt),
        acceptance,
        depth,
        verify,
        readiness_ms,
        completed: pair_rate(old, new, |k| k.name == COMPLETED, dt),
        failed: pair_rate(old, new, |k| k.name == FAILED, dt),
        reused: pair_rate(old, new, |k| k.name == CACHE_REUSED, dt),
        cache_hit,
        host_hit,
        kv_usage: kv_usage_at(new),
        engram_gathers: pair_rate(old, new, |k| k.name == ENGRAM_GATHERS, dt),
        engram_disk_bytes: pair_rate(old, new, |k| k.name == ENGRAM_DISK_BYTES, dt),
        engram_disk_wait: ratio(
            pair_rate(old, new, |k| k.name == ENGRAM_DISK_WAIT_NS, dt),
            gather_ns,
        ),
        engram_hit,
    }
}

fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    Some(sorted[sorted.len() / 2])
}

/// Builds the trailing 60s graph window in one pass. A decode-rate collapse
/// below 25% of the window's nonzero median, while prefill work and requests
/// are present, marks the interval as a stall. The detector is intentionally
/// conservative: it needs five measured intervals and a positive median.
pub fn scan_window(h: &History) -> Graphs {
    const WINDOW: f64 = 60.0;
    let Some(last) = h.entries().back() else {
        return Graphs::default();
    };
    let now = last.t;
    let mut graphs = Graphs::default();
    let entries: Vec<_> = h.entries().iter().collect();
    for pair in entries.windows(2) {
        let (old, new) = (pair[0], pair[1]);
        if now.duration_since(new.t).as_secs_f64() > WINDOW {
            continue;
        }
        let dt = (new.t - old.t).as_secs_f64();
        if dt <= 0.0 {
            continue;
        }
        let i = interval(&old.sample, &new.sample, dt);
        graphs.prefill.vals.push(i.prefill);
        graphs.generated.vals.push(i.generated);
        graphs.per_stream.vals.push(i.per_stream);
        graphs.active.vals.push(i.active);
        graphs.queue.vals.push(i.queue);
        graphs.gpu_busy.vals.push(i.gpu_busy);
        graphs.host_out.vals.push(i.host_out);
        graphs.host_in.vals.push(i.host_in);
        graphs.acceptance.vals.push(i.acceptance);
        graphs.depth.vals.push(i.depth);
        graphs.verify.vals.push(i.verify);
        graphs.readiness_ms.vals.push(i.readiness_ms);
        graphs.completed.vals.push(i.completed);
        graphs.failed.vals.push(i.failed);
        graphs.reused.vals.push(i.reused);
        graphs.cache_hit.vals.push(i.cache_hit);
        graphs.host_hit.vals.push(i.host_hit);
        graphs.kv_usage.vals.push(i.kv_usage);
        graphs.engram_gathers.vals.push(i.engram_gathers);
        graphs.engram_disk_bytes.vals.push(i.engram_disk_bytes);
        graphs.engram_disk_wait.vals.push(i.engram_disk_wait);
        graphs.engram_hit.vals.push(i.engram_hit);
        graphs.dt.push(dt);
    }
    let measured: Vec<f64> = graphs.generated.vals.iter().flatten().copied().collect();
    let median = median(&measured);
    if measured.len() >= 5 {
        if let Some(median) = median.filter(|m| *m > 0.0) {
            graphs.stall = (0..graphs.generated.vals.len())
                .map(|i| {
                    let generated = graphs.generated.vals[i];
                    let prefill = graphs.prefill.vals[i].unwrap_or(0.0);
                    let active = graphs.active.vals[i].unwrap_or(0.0);
                    generated.is_some_and(|g| g < 0.25 * median)
                        && prefill > 0.0
                        && active > 0.0
                })
                .collect();
        }
    }
    if graphs.stall.len() < graphs.generated.vals.len() {
        graphs.stall.resize(graphs.generated.vals.len(), false);
    }
    graphs
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stalls {
    pub count: u32,
    pub seconds: f64,
}

fn fold_stalls(flags: &[bool], dt: &[f64]) -> [Stalls; 3] {
    let mut out = [Stalls::default(); 3];
    for (i, window) in WINDOWS.iter().enumerate() {
        let mut budget = window.as_secs_f64();
        for j in (0..flags.len().min(dt.len())).rev() {
            if budget <= 0.0 {
                break;
            }
            let take = dt[j].min(budget);
            if flags[j] {
                out[i].count += 1;
                out[i].seconds += take;
            }
            budget -= dt[j];
        }
    }
    out
}

/// Everything the UI needs for one draw.
pub struct Derived {
    pub flare: Flare,
    pub rates: [Rates; 3],
    pub graphs: Graphs,
    pub stalls: [Stalls; 3],
}

pub fn derive(h: &History) -> Option<Derived> {
    let flare = Flare::from(&h.last()?.sample);
    let rates = WINDOWS.map(|window| rates(h, window, flare));
    let graphs = h.scan().cloned().unwrap_or_default();
    let stalls = fold_stalls(&graphs.stall, &graphs.dt);
    Some(Derived {
        flare,
        rates,
        graphs,
        stalls,
    })
}

/// Session peak rates, accumulated across the whole run.
#[derive(Clone, Copy, Debug, Default)]
pub struct Peaks {
    pub prefill: Option<f64>,
    pub generated: Option<f64>,
    pub per_stream: Option<f64>,
    pub active: Option<f64>,
    pub queue: Option<f64>,
    pub gpu_busy: Option<f64>,
    pub host_out: Option<f64>,
    pub host_in: Option<f64>,
    pub acceptance: Option<f64>,
    pub depth: Option<f64>,
    pub verify: Option<f64>,
    pub readiness_ms: Option<f64>,
    pub completed: Option<f64>,
    pub failed: Option<f64>,
    pub reused: Option<f64>,
    pub cache_hit: Option<f64>,
    pub host_hit: Option<f64>,
    pub kv_usage: Option<f64>,
    pub engram_gathers: Option<f64>,
    pub engram_disk_bytes: Option<f64>,
    pub engram_disk_wait: Option<f64>,
    pub engram_hit: Option<f64>,
}

fn bump(slot: &mut Option<f64>, value: Option<f64>) {
    if let Some(value) = value {
        *slot = Some(slot.map_or(value, |old| old.max(value)));
    }
}

pub fn update_peaks(peaks: &mut Peaks, h: &History) {
    let mut entries = h.entries().iter().rev();
    let Some(new) = entries.next() else {
        return;
    };
    let Some(old) = entries.next() else {
        return;
    };
    let dt = (new.t - old.t).as_secs_f64();
    let i = interval(&old.sample, &new.sample, dt);
    bump(&mut peaks.prefill, i.prefill);
    bump(&mut peaks.generated, i.generated);
    bump(&mut peaks.per_stream, i.per_stream);
    bump(&mut peaks.active, i.active);
    bump(&mut peaks.queue, i.queue);
    bump(&mut peaks.gpu_busy, i.gpu_busy);
    bump(&mut peaks.host_out, i.host_out);
    bump(&mut peaks.host_in, i.host_in);
    bump(&mut peaks.acceptance, i.acceptance);
    bump(&mut peaks.depth, i.depth);
    bump(&mut peaks.verify, i.verify);
    bump(&mut peaks.readiness_ms, i.readiness_ms);
    bump(&mut peaks.completed, i.completed);
    bump(&mut peaks.failed, i.failed);
    bump(&mut peaks.reused, i.reused);
    bump(&mut peaks.cache_hit, i.cache_hit);
    bump(&mut peaks.host_hit, i.host_hit);
    bump(&mut peaks.kv_usage, i.kv_usage);
    bump(&mut peaks.engram_gathers, i.engram_gathers);
    bump(&mut peaks.engram_disk_bytes, i.engram_disk_bytes);
    bump(&mut peaks.engram_disk_wait, i.engram_disk_wait);
    bump(&mut peaks.engram_hit, i.engram_hit);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::parse;
    use std::time::Instant;

    fn body(prefill: u64, generated: u64, active: u64) -> String {
        format!(
            "# TYPE flare_prefill_tokens_total counter\nflare_prefill_tokens_total {prefill}\n\
             # TYPE flare_generated_tokens_total counter\nflare_generated_tokens_total {generated}\n\
             # TYPE flare_active_requests gauge\nflare_active_requests {active}\n\
             # TYPE flare_draft_request_rounds_total counter\nflare_draft_request_rounds_total 10\n\
             # TYPE flare_accepted_draft_tokens_total counter\nflare_accepted_draft_tokens_total 20\n\
             # TYPE flare_verification_draft_tokens_total counter\nflare_verification_draft_tokens_total 40\n\
             # TYPE flare_verification_request_rounds_total counter\nflare_verification_request_rounds_total 10\n\
             # TYPE flare_replay_gpu_nanoseconds_total counter\nflare_replay_gpu_nanoseconds_total {}\n\
             # TYPE flare_total_kv_tokens gauge\nflare_total_kv_tokens 1000\n\
             # TYPE flare_free_kv_tokens gauge\nflare_free_kv_tokens 250\n"
            , (prefill as f64 * 1e9) as u64
        )
    }

    #[test]
    fn derives_window_rates_ratios_and_pools() {
        let mut h = History::default();
        let t0 = Instant::now();
        h.push(t0, parse(&body(1000, 500, 2)).unwrap());
        let body2 = body(4000, 1500, 3)
            .replace(
                "flare_draft_request_rounds_total 10",
                "flare_draft_request_rounds_total 20",
            )
            .replace(
                "flare_accepted_draft_tokens_total 20",
                "flare_accepted_draft_tokens_total 40",
            )
            .replace(
                "flare_verification_draft_tokens_total 40",
                "flare_verification_draft_tokens_total 80",
            )
            .replace(
                "flare_verification_request_rounds_total 10",
                "flare_verification_request_rounds_total 20",
            );
        h.push(t0 + Duration::from_secs(2), parse(&body2).unwrap());
        let d = derive(&h).unwrap();
        assert_eq!(d.rates[0].prefill, Some(1500.0));
        assert_eq!(d.rates[0].generated, Some(500.0));
        assert_eq!(d.rates[0].per_stream, Some(200.0));
        assert_eq!(d.rates[0].acceptance, Some(2.0));
        assert_eq!(d.rates[0].depth, Some(4.0));
        assert_eq!(d.flare.kv_used_tokens(), 750);
        assert_eq!(d.flare.kv_usage(), Some(0.75));
    }

    #[test]
    fn scan_window_carries_rate_points_and_stall_ticks() {
        let mut h = History::default();
        let t0 = Instant::now();
        for (i, generated) in [1000u64, 2000, 3000, 4000, 5000, 5100].into_iter().enumerate() {
            h.push(
                t0 + Duration::from_secs(i as u64),
                parse(&body(1000 + i as u64 * 1000, generated, 1)).unwrap(),
            );
        }
        let d = derive(&h).unwrap();
        assert_eq!(d.graphs.generated.vals.len(), 5);
        assert_eq!(d.graphs.dt, vec![1.0; 5]);
        assert_eq!(d.stalls[2].count, 1);
    }

    #[test]
    fn labelled_families_fold_into_totals() {
        let body = "# TYPE flare_http_requests_total counter\n                    flare_http_requests_total{api=\"chat\"} 7\n                    flare_http_requests_total{api=\"completions\"} 3\n                    # TYPE flare_graph_launches_total counter\n                    flare_graph_launches_total{kind=\"decode\"} 5\n                    flare_graph_launches_total{kind=\"speculative\"} 9\n                    # TYPE flare_speculative_rounds_total counter\n                    flare_speculative_rounds_total{batch=\"1\",width=\"8\"} 11\n                    flare_speculative_rounds_total{batch=\"4\",width=\"8\"} 4\n                    # TYPE flare_host_cache_lookup_outcomes_total counter\n                    flare_host_cache_lookup_outcomes_total{reason=\"cost_rejected\"} 2\n                    flare_host_cache_lookup_outcomes_total{reason=\"capacity_rejected\"} 3\n                    flare_host_cache_lookup_outcomes_total{reason=\"enqueue_rejected\"} 5\n";
        let flare = Flare::from(&parse(body).unwrap());
        assert_eq!(flare.http_total(), 10);
        assert_eq!(flare.graph_launches("decode"), 5);
        assert_eq!(flare.graph_launches("speculative"), 9);
        assert_eq!(flare.spec_rounds, 15);
        assert_eq!(flare.host_lookup_rejected, 10);
    }

    fn extended_body(step: u64) -> String {
        let n = |value: u64| value * step;
        format!(
            "# TYPE flare_prefill_tokens_total counter\nflare_prefill_tokens_total {}\n             # TYPE flare_generated_tokens_total counter\nflare_generated_tokens_total {}\n             # TYPE flare_active_requests gauge\nflare_active_requests {}\n             # TYPE flare_pending_requests gauge\nflare_pending_requests {}\n             # TYPE flare_accepted_draft_tokens_total counter\nflare_accepted_draft_tokens_total {}\n             # TYPE flare_draft_request_rounds_total counter\nflare_draft_request_rounds_total {}\n             # TYPE flare_verification_draft_tokens_total counter\nflare_verification_draft_tokens_total {}\n             # TYPE flare_verification_request_rounds_total counter\nflare_verification_request_rounds_total {}\n             # TYPE flare_speculative_rounds_total counter\nflare_speculative_rounds_total{{batch=\"1\",width=\"8\"}} {}\n             # TYPE flare_cache_lookups_total counter\nflare_cache_lookups_total {}\n             # TYPE flare_cache_hits_total counter\nflare_cache_hits_total {}\n             # TYPE flare_cache_reused_tokens_total counter\nflare_cache_reused_tokens_total {}\n             # TYPE flare_host_cache_lookups_total counter\nflare_host_cache_lookups_total {}\n             # TYPE flare_host_cache_hits_total counter\nflare_host_cache_hits_total {}\n             # TYPE flare_host_cache_bytes_out_total counter\nflare_host_cache_bytes_out_total {}\n             # TYPE flare_host_cache_bytes_in_total counter\nflare_host_cache_bytes_in_total {}\n             # TYPE flare_requests_completed_total counter\nflare_requests_completed_total {}\n             # TYPE flare_requests_failed_total counter\nflare_requests_failed_total {}\n             # TYPE flare_replay_gpu_nanoseconds_total counter\nflare_replay_gpu_nanoseconds_total {}\n             # TYPE flare_replay_readiness_wait_nanoseconds_total counter\nflare_replay_readiness_wait_nanoseconds_total {}\n             # TYPE flare_replay_readiness_samples_total counter\nflare_replay_readiness_samples_total {}\n             # TYPE flare_total_kv_tokens gauge\nflare_total_kv_tokens {}\n             # TYPE flare_free_kv_tokens gauge\nflare_free_kv_tokens {}\n             # TYPE flare_engram_gathers_total counter\nflare_engram_gathers_total {}\n             # TYPE flare_engram_gather_nanoseconds_total counter\nflare_engram_gather_nanoseconds_total {}\n             # TYPE flare_engram_disk_bytes_total counter\nflare_engram_disk_bytes_total {}\n             # TYPE flare_engram_disk_wait_nanoseconds_total counter\nflare_engram_disk_wait_nanoseconds_total {}\n             # TYPE flare_engram_row_cache_hits_total counter\nflare_engram_row_cache_hits_total {}\n             # TYPE flare_engram_row_cache_misses_total counter\nflare_engram_row_cache_misses_total {}\n",
            n(100),
            n(200),
            if step > 0 { 2 } else { 0 },
            if step > 0 { 3 } else { 0 },
            n(20),
            n(10),
            n(40),
            n(10),
            n(10),
            n(10),
            n(5),
            n(1000),
            n(10),
            n(4),
            n(1000),
            n(2000),
            n(2),
            n(1),
            n(1_000_000_000),
            n(1_000_000),
            n(2),
            if step > 0 { 1000 } else { 0 },
            if step > 0 { 500 } else { 0 },
            n(10),
            n(1_000_000_000),
            n(1_000_000),
            n(500_000_000),
            n(5),
            n(5),
        )
    }

    #[test]
    fn extended_lanes_and_peaks_cover_spec_cache_host_and_engram() {
        let mut h = History::default();
        let t0 = Instant::now();
        h.push(t0, parse(&extended_body(0)).unwrap());
        h.push(t0 + Duration::from_secs(1), parse(&extended_body(1)).unwrap());
        let d = derive(&h).unwrap();
        let last = |lane: &Lane| lane.vals.last().copied().flatten();
        assert_eq!(last(&d.graphs.per_stream), Some(100.0));
        assert_eq!(last(&d.graphs.acceptance), Some(2.0));
        assert_eq!(last(&d.graphs.depth), Some(4.0));
        assert_eq!(last(&d.graphs.verify), Some(10.0));
        assert_eq!(last(&d.graphs.completed), Some(2.0));
        assert_eq!(last(&d.graphs.failed), Some(1.0));
        assert_eq!(last(&d.graphs.reused), Some(1000.0));
        assert_eq!(last(&d.graphs.cache_hit), Some(0.5));
        assert_eq!(last(&d.graphs.host_hit), Some(0.4));
        assert_eq!(last(&d.graphs.kv_usage), Some(0.5));
        assert_eq!(last(&d.graphs.engram_gathers), Some(10.0));
        assert_eq!(last(&d.graphs.engram_disk_bytes), Some(1_000_000.0));
        assert_eq!(last(&d.graphs.engram_disk_wait), Some(0.5));
        assert_eq!(last(&d.graphs.engram_hit), Some(0.5));
        assert_eq!(last(&d.graphs.readiness_ms), Some(0.5));
        assert_eq!(last(&d.graphs.gpu_busy), Some(1.0));
        assert_eq!(last(&d.graphs.queue), Some(3.0));

        let mut peaks = Peaks::default();
        update_peaks(&mut peaks, &h);
        assert_eq!(peaks.verify, Some(10.0));
        assert_eq!(peaks.cache_hit, Some(0.5));
        assert_eq!(peaks.host_hit, Some(0.4));
        assert_eq!(peaks.kv_usage, Some(0.5));
        assert_eq!(peaks.engram_gathers, Some(10.0));
        assert_eq!(peaks.engram_disk_bytes, Some(1_000_000.0));
        assert_eq!(peaks.engram_disk_wait, Some(0.5));
        assert_eq!(peaks.engram_hit, Some(0.5));
        assert_eq!(peaks.active, Some(2.0));
        assert_eq!(peaks.queue, Some(3.0));
        assert_eq!(peaks.completed, Some(2.0));
        assert_eq!(peaks.failed, Some(1.0));
        assert_eq!(peaks.reused, Some(1000.0));
    }

    #[test]
    fn peak_tracking_keeps_session_maximum() {
        let mut h = History::default();
        let t0 = Instant::now();
        h.push(t0, parse(&body(0, 0, 1)).unwrap());
        h.push(t0 + Duration::from_secs(1), parse(&body(100, 50, 1)).unwrap());
        h.push(t0 + Duration::from_secs(2), parse(&body(400, 100, 1)).unwrap());
        let mut peaks = Peaks::default();
        update_peaks(&mut peaks, &h);
        assert_eq!(peaks.prefill, Some(300.0));
        assert_eq!(peaks.generated, Some(50.0));
    }
}
