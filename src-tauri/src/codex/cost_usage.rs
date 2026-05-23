use crate::codex::account_store::default_wovo_codex_root;
use crate::codex::atomic_file::{replace_file, temporary_file_path, write_new_file};
use crate::domain::usage::{CostUsageDailyPoint, CostUsageScanStats, CostUsageSnapshot};
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime, UtcOffset};

mod pricing;

use pricing::{codex_cost_usd, normalize_codex_model};

const CACHE_FILE_NAME: &str = "codex-v2.json";
const CACHE_VERSION: u16 = 3;
const DEFAULT_MODEL: &str = "gpt-5";
const DEFAULT_RANGE_DAYS: u16 = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CostUsageCache {
    version: u16,
    last_scan_unix_ms: i64,
    retention_days: u16,
    files: BTreeMap<String, CostUsageFileUsage>,
    roots: Option<BTreeMap<String, i64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CostUsageFileUsage {
    mtime_unix_ms: i64,
    size: i64,
    events: Vec<CostUsageEvent>,
    parsed_bytes: Option<i64>,
    last_model: Option<String>,
    last_totals: Option<CodexTotals>,
    session_id: Option<String>,
    forked_from_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CostUsageEvent {
    timestamp_unix: i64,
    model: String,
    session_id: Option<String>,
    project: Option<String>,
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    cost_usd: Option<f64>,
    // Set only when the source row lacked an offset/Z and we synthesized
    // timestamp_unix from the parsed YYYY-MM-DD prefix. The string anchors
    // the bucket on the wire-format day so a later local-offset rebucket
    // cannot shift the row off its original calendar day.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    naive_day_key: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
struct CodexTotals {
    input: i64,
    cached: i64,
    output: i64,
}

#[derive(Debug, Clone)]
struct DayRange {
    since_key: String,
    until_key: String,
    scan_since_key: String,
    scan_until_key: String,
    retention_days: u16,
}

#[derive(Debug, Clone)]
struct ParseResult {
    events: Vec<CostUsageEvent>,
    parsed_bytes: i64,
    last_model: Option<String>,
    last_totals: Option<CodexTotals>,
    session_id: Option<String>,
    forked_from_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct ScanStatsAccumulator {
    files_scanned: usize,
    files_reused: usize,
    files_removed: usize,
}

#[derive(Debug, Clone)]
struct SessionMetadata {
    session_id: Option<String>,
    forked_from_id: Option<String>,
    fork_timestamp: Option<String>,
}

#[derive(Debug, Clone)]
struct TimestampedTotals {
    timestamp: String,
    parsed: Option<OffsetDateTime>,
    totals: CodexTotals,
}

pub fn local_codex_logs_exist(codex_home: &Path) -> bool {
    [
        codex_home.join("sessions"),
        codex_home.join("archived_sessions"),
    ]
    .into_iter()
    .any(|root| first_jsonl_file(&root).is_some())
}

pub fn load_cost_usage_snapshot_with_range(
    source_root: PathBuf,
    force_rescan: bool,
    range_days: u16,
) -> Result<CostUsageSnapshot, AppError> {
    load_cost_usage_snapshot_at(
        source_root,
        default_wovo_codex_root().join("cache"),
        OffsetDateTime::now_utc(),
        force_rescan,
        range_days,
    )
}

fn load_cost_usage_snapshot_at(
    source_root: PathBuf,
    cache_root: PathBuf,
    now: OffsetDateTime,
    force_rescan: bool,
    range_days: u16,
) -> Result<CostUsageSnapshot, AppError> {
    let output_range_days = normalize_range_days(range_days);
    let offset = local_utc_offset();
    let range = DayRange::new(now, output_range_days, offset);
    let mut cache = if force_rescan {
        CostUsageCache::new()
    } else {
        load_cache(&cache_root).unwrap_or_default()
    };
    if cache.retention_days < output_range_days {
        cache = CostUsageCache::new();
    }
    cache.retention_days = output_range_days;

    let stats = scan_roots(&source_root, &range, &mut cache, now, force_rescan)?;
    save_cache(&cache_root, &cache)?;

    Ok(build_snapshot_from_cache(
        &cache,
        &range,
        now,
        offset,
        stats,
        source_root.to_string_lossy().to_string(),
    ))
}

impl CostUsageCache {
    fn new() -> Self {
        Self {
            version: CACHE_VERSION,
            last_scan_unix_ms: 0,
            retention_days: DEFAULT_RANGE_DAYS,
            files: BTreeMap::new(),
            roots: None,
        }
    }
}

impl Default for CostUsageCache {
    fn default() -> Self {
        Self::new()
    }
}

impl DayRange {
    fn new(now: OffsetDateTime, range_days: u16, offset: UtcOffset) -> Self {
        let range_days = normalize_range_days(range_days);
        let local_now = now.to_offset(offset);
        let since = local_now - Duration::days(i64::from(range_days.saturating_sub(1)));
        Self {
            since_key: day_key_from_datetime(since),
            until_key: day_key_from_datetime(local_now),
            scan_since_key: day_key_from_datetime(since - Duration::days(1)),
            scan_until_key: day_key_from_datetime(local_now + Duration::days(1)),
            retention_days: range_days,
        }
    }

    fn contains_scan_day(&self, day_key: &str) -> bool {
        day_key >= self.scan_since_key.as_str() && day_key <= self.scan_until_key.as_str()
    }

    fn contains_output_day(&self, day_key: &str) -> bool {
        day_key >= self.since_key.as_str() && day_key <= self.until_key.as_str()
    }
}

fn scan_roots(
    source_root: &Path,
    range: &DayRange,
    cache: &mut CostUsageCache,
    now: OffsetDateTime,
    _force_rescan: bool,
) -> Result<ScanStatsAccumulator, AppError> {
    let roots = [
        source_root.join("sessions"),
        source_root.join("archived_sessions"),
    ];
    let roots_fingerprint = roots_fingerprint(&roots);
    let mut files = Vec::new();
    let mut seen_paths = HashSet::new();

    for root in roots {
        for file in list_jsonl_files(&root)? {
            let key = file.to_string_lossy().to_string();
            if seen_paths.insert(key) {
                files.push(file);
            }
        }
    }

    let session_index = build_session_index(&files);
    let mut seen_session_ids = HashSet::new();
    let mut file_paths_in_scan = HashSet::new();
    let mut stats = ScanStatsAccumulator::default();

    for file in files {
        file_paths_in_scan.insert(file.to_string_lossy().to_string());
        scan_file_into_cache(
            &file,
            range,
            cache,
            &mut seen_session_ids,
            &session_index,
            &mut stats,
        )?;
    }

    let stale_paths: Vec<String> = cache
        .files
        .keys()
        .filter(|path| !file_paths_in_scan.contains(*path))
        .cloned()
        .collect();
    for path in stale_paths {
        if cache.files.remove(&path).is_some() {
            stats.files_removed += 1;
        }
    }

    for usage in cache.files.values_mut() {
        prune_file_usage(usage, range);
    }
    cache.roots = Some(roots_fingerprint);
    cache.last_scan_unix_ms = now.unix_timestamp() * 1000;
    Ok(stats)
}

fn scan_file_into_cache(
    file: &Path,
    range: &DayRange,
    cache: &mut CostUsageCache,
    seen_session_ids: &mut HashSet<String>,
    session_index: &HashMap<String, PathBuf>,
    stats: &mut ScanStatsAccumulator,
) -> Result<(), AppError> {
    let path = file.to_string_lossy().to_string();
    let metadata = match fs::metadata(file) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if cache.files.remove(&path).is_some() {
                stats.files_removed += 1;
            }
            return Ok(());
        }
        Err(error) => return Err(AppError::AccountStore(error.to_string())),
    };
    let mtime_unix_ms = metadata_mtime_ms(&metadata);
    let size = i64::try_from(metadata.len()).unwrap_or(i64::MAX);

    let cached = cache.files.get(&path).cloned();
    if let Some(session_id) = cached
        .as_ref()
        .and_then(|cached| cached.session_id.as_deref())
    {
        if seen_session_ids.contains(session_id) {
            if cache.files.remove(&path).is_some() {
                stats.files_removed += 1;
            }
            return Ok(());
        }
    }

    let needs_session_id = cached
        .as_ref()
        .map(|cached| cached.session_id.is_none())
        .unwrap_or(false);
    if let Some(cached) = cached.as_ref() {
        if cached.mtime_unix_ms == mtime_unix_ms && cached.size == size && !needs_session_id {
            if let Some(session_id) = cached.session_id.as_deref() {
                seen_session_ids.insert(session_id.to_string());
            }
            stats.files_reused += 1;
            return Ok(());
        }
    }

    if let Some(cached) = cached.as_ref() {
        let start_offset = cached.parsed_bytes.unwrap_or(cached.size);
        let can_incremental = size > cached.size
            && start_offset > 0
            && start_offset <= size
            && cached.last_totals.is_some()
            && cached.forked_from_id.is_none();
        if can_incremental {
            let parsed = parse_codex_file(
                file,
                range,
                start_offset,
                cached.last_model.clone(),
                cached.last_totals,
                None,
            )?;
            let session_id = parsed
                .session_id
                .clone()
                .or_else(|| cached.session_id.clone());
            if let Some(session_id) = session_id.as_deref() {
                if seen_session_ids.contains(session_id) {
                    if cache.files.remove(&path).is_some() {
                        stats.files_removed += 1;
                    }
                    return Ok(());
                }
            }

            let mut merged_events = cached.events.clone();
            merged_events.extend(parsed.events);
            cache.files.insert(
                path,
                CostUsageFileUsage {
                    mtime_unix_ms,
                    size,
                    events: merged_events,
                    parsed_bytes: Some(parsed.parsed_bytes),
                    last_model: parsed.last_model,
                    last_totals: parsed.last_totals,
                    session_id: session_id.clone(),
                    forked_from_id: parsed
                        .forked_from_id
                        .or_else(|| cached.forked_from_id.clone()),
                },
            );
            if let Some(session_id) = session_id {
                seen_session_ids.insert(session_id);
            }
            stats.files_scanned += 1;
            return Ok(());
        }
    }

    let _ = cached;

    let inherited = inherited_totals(file, session_index)?;
    let parsed = parse_codex_file(file, range, 0, None, None, inherited)?;
    let session_id = parsed.session_id.clone();
    if let Some(session_id) = session_id.as_deref() {
        if seen_session_ids.contains(session_id) {
            cache.files.remove(&path);
            stats.files_removed += 1;
            return Ok(());
        }
    }

    cache.files.insert(
        path,
        CostUsageFileUsage {
            mtime_unix_ms,
            size,
            events: parsed.events,
            parsed_bytes: Some(parsed.parsed_bytes),
            last_model: parsed.last_model,
            last_totals: parsed.last_totals,
            session_id: session_id.clone(),
            forked_from_id: parsed.forked_from_id,
        },
    );
    if let Some(session_id) = session_id {
        seen_session_ids.insert(session_id);
    }
    stats.files_scanned += 1;
    Ok(())
}

fn inherited_totals(
    file: &Path,
    session_index: &HashMap<String, PathBuf>,
) -> Result<Option<CodexTotals>, AppError> {
    let Some(metadata) = parse_session_metadata(file)? else {
        return Ok(None);
    };
    let Some(parent_id) = metadata.forked_from_id else {
        return Ok(None);
    };
    let Some(parent_file) = session_index.get(&parent_id) else {
        return Ok(None);
    };
    let cutoff = metadata.fork_timestamp.unwrap_or_default();
    let snapshots = parse_token_snapshots(parent_file)?;
    let cutoff_dt = parse_timestamp(&cutoff);
    let mut inherited = None;
    for snapshot in snapshots {
        let is_before = match (snapshot.parsed, cutoff_dt) {
            (Some(left), Some(right)) => left <= right,
            _ => snapshot.timestamp <= cutoff,
        };
        if is_before {
            inherited = Some(snapshot.totals);
        }
    }
    Ok(inherited)
}

fn parse_codex_file(
    file: &Path,
    range: &DayRange,
    start_offset: i64,
    initial_model: Option<String>,
    initial_totals: Option<CodexTotals>,
    inherited_totals: Option<CodexTotals>,
) -> Result<ParseResult, AppError> {
    let mut current_model = initial_model;
    let mut previous_totals = initial_totals;
    let mut session_id = None;
    let mut forked_from_id = None;
    let mut events = Vec::new();

    if start_offset == 0 {
        if let Some(metadata) = parse_session_metadata(file)? {
            session_id = metadata.session_id;
            forked_from_id = metadata.forked_from_id;
        }
    }

    let parsed_bytes = scan_jsonl_file(file, start_offset, 256 * 1024, |line| {
        if !line_has_interesting_codex_type(line) {
            return;
        }
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        let Some(row_type) = value.get("type").and_then(Value::as_str) else {
            return;
        };

        if row_type == "session_meta" {
            if session_id.is_none() {
                session_id = metadata_string(&value, &["session_id", "sessionId", "id"]);
            }
            if forked_from_id.is_none() {
                forked_from_id = metadata_string(
                    &value,
                    &[
                        "forked_from_id",
                        "forkedFromId",
                        "parent_session_id",
                        "parentSessionId",
                    ],
                );
            }
            return;
        }

        let Some(timestamp) = value.get("timestamp").and_then(Value::as_str) else {
            return;
        };
        let Some(day_key) = day_key_from_timestamp(timestamp) else {
            return;
        };

        if row_type == "turn_context" {
            if let Some(payload) = value.get("payload") {
                if let Some(model) = payload.get("model").and_then(Value::as_str).or_else(|| {
                    payload
                        .get("info")
                        .and_then(|info| info.get("model"))
                        .and_then(Value::as_str)
                }) {
                    current_model = Some(model.to_string());
                }
            }
            return;
        }

        if row_type != "event_msg" {
            return;
        }
        let Some(payload) = value.get("payload") else {
            return;
        };
        if payload.get("type").and_then(Value::as_str) != Some("token_count") {
            return;
        }
        let info = payload.get("info");
        let model_from_info = info
            .and_then(|info| info.get("model"))
            .and_then(Value::as_str)
            .or_else(|| {
                info.and_then(|info| info.get("model_name"))
                    .and_then(Value::as_str)
            })
            .or_else(|| payload.get("model").and_then(Value::as_str))
            .or_else(|| value.get("model").and_then(Value::as_str));
        let model = current_model
            .clone()
            .or_else(|| model_from_info.map(str::to_string))
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());

        let total = info.and_then(|info| info.get("total_token_usage"));
        let last = info.and_then(|info| info.get("last_token_usage"));

        let mut delta = CodexTotals {
            input: 0,
            cached: 0,
            output: 0,
        };

        if let Some(total) = total {
            let raw_totals = token_usage_totals(total);
            let current_totals = if let Some(inherited) = inherited_totals {
                CodexTotals {
                    input: (raw_totals.input - inherited.input).max(0),
                    cached: (raw_totals.cached - inherited.cached).max(0),
                    output: (raw_totals.output - inherited.output).max(0),
                }
            } else {
                raw_totals
            };
            let previous = previous_totals.unwrap_or(CodexTotals {
                input: 0,
                cached: 0,
                output: 0,
            });
            delta = CodexTotals {
                input: (current_totals.input - previous.input).max(0),
                cached: (current_totals.cached - previous.cached).max(0),
                output: (current_totals.output - previous.output).max(0),
            };
            previous_totals = Some(current_totals);
        } else if let Some(last) = last {
            let mut raw_delta = token_usage_totals(last);
            raw_delta.input = raw_delta.input.max(0);
            raw_delta.cached = raw_delta.cached.max(0);
            raw_delta.output = raw_delta.output.max(0);
            delta = raw_delta;
            let previous = previous_totals.unwrap_or(CodexTotals {
                input: 0,
                cached: 0,
                output: 0,
            });
            previous_totals = Some(CodexTotals {
                input: previous.input + delta.input,
                cached: previous.cached + delta.cached,
                output: previous.output + delta.output,
            });
        }

        if delta.input == 0 && delta.cached == 0 && delta.output == 0 {
            return;
        }
        let cached = delta.cached.min(delta.input);
        let normalized_model = normalize_codex_model(&model);
        if range.contains_scan_day(&day_key) {
            let (timestamp_unix, naive_day_key) = match timestamp_unix(timestamp) {
                Some(unix) => (Some(unix), None),
                None => (unix_from_day_key(&day_key), Some(day_key.clone())),
            };
            if let Some(timestamp_unix) = timestamp_unix {
                let cost_usd = codex_cost_usd(&normalized_model, delta.input, cached, delta.output);
                events.push(CostUsageEvent {
                    timestamp_unix,
                    model: normalized_model,
                    session_id: session_id.clone(),
                    project: None,
                    input_tokens: delta.input,
                    cached_input_tokens: cached,
                    output_tokens: delta.output,
                    cost_usd,
                    naive_day_key,
                });
            }
        }
    })?;

    if session_id.is_some() {
        for event in &mut events {
            if event.session_id.is_none() {
                event.session_id.clone_from(&session_id);
            }
        }
    }

    Ok(ParseResult {
        events,
        parsed_bytes,
        last_model: current_model,
        last_totals: previous_totals,
        session_id,
        forked_from_id,
    })
}

fn parse_token_snapshots(file: &Path) -> Result<Vec<TimestampedTotals>, AppError> {
    let mut previous_totals = None;
    let mut snapshots = Vec::new();
    scan_jsonl_file(file, 0, 512 * 1024, |line| {
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        if value.get("type").and_then(Value::as_str) != Some("event_msg") {
            return;
        }
        let Some(payload) = value.get("payload") else {
            return;
        };
        if payload.get("type").and_then(Value::as_str) != Some("token_count") {
            return;
        }
        let Some(info) = payload.get("info") else {
            return;
        };
        let Some(timestamp) = value.get("timestamp").and_then(Value::as_str) else {
            return;
        };

        let next = if let Some(total) = info.get("total_token_usage") {
            token_usage_totals(total)
        } else if let Some(last) = info.get("last_token_usage") {
            let base = previous_totals.unwrap_or(CodexTotals {
                input: 0,
                cached: 0,
                output: 0,
            });
            let delta = token_usage_totals(last);
            CodexTotals {
                input: base.input + delta.input,
                cached: base.cached + delta.cached,
                output: base.output + delta.output,
            }
        } else {
            return;
        };
        previous_totals = Some(next);
        snapshots.push(TimestampedTotals {
            timestamp: timestamp.to_string(),
            parsed: parse_timestamp(timestamp),
            totals: next,
        });
    })?;
    Ok(snapshots)
}

fn parse_session_metadata(file: &Path) -> Result<Option<SessionMetadata>, AppError> {
    let mut metadata = None;
    scan_jsonl_file(file, 0, 512 * 1024, |line| {
        if metadata.is_some() {
            return;
        }
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        if value.get("type").and_then(Value::as_str) != Some("session_meta") {
            return;
        }
        metadata = Some(SessionMetadata {
            session_id: metadata_string(&value, &["session_id", "sessionId", "id"]),
            forked_from_id: metadata_string(
                &value,
                &[
                    "forked_from_id",
                    "forkedFromId",
                    "parent_session_id",
                    "parentSessionId",
                ],
            ),
            fork_timestamp: metadata_string(&value, &["timestamp"]),
        });
    })?;
    Ok(metadata)
}

fn metadata_string(value: &Value, keys: &[&str]) -> Option<String> {
    let payload = value.get("payload");
    for key in keys {
        if let Some(text) = payload
            .and_then(|payload| payload.get(*key))
            .and_then(Value::as_str)
            .or_else(|| value.get(*key).and_then(Value::as_str))
        {
            return Some(text.to_string());
        }
    }
    None
}

fn token_usage_totals(value: &Value) -> CodexTotals {
    CodexTotals {
        input: json_i64(value.get("input_tokens")),
        cached: json_i64(
            value
                .get("cached_input_tokens")
                .or_else(|| value.get("cache_read_input_tokens")),
        ),
        output: json_i64(value.get("output_tokens")),
    }
}

fn json_i64(value: Option<&Value>) -> i64 {
    value
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        })
        .unwrap_or(0)
}

fn build_snapshot_from_cache(
    cache: &CostUsageCache,
    range: &DayRange,
    now: OffsetDateTime,
    offset: UtcOffset,
    stats: ScanStatsAccumulator,
    source_root: String,
) -> CostUsageSnapshot {
    let today_key = day_key_from_datetime(now.to_offset(offset));
    let mut daily = Vec::new();
    let mut last_30_days_tokens = 0;
    let mut last_30_days_cost = 0.0;
    let mut last_30_days_priced = true;
    let mut today_tokens = 0;
    let mut today_cost = 0.0;
    let mut today_priced = true;

    let mut by_day = BTreeMap::<String, DailyTotals>::new();
    let mut events_retained = 0;
    for event in cache.files.values().flat_map(|file| file.events.iter()) {
        let Some(day_key) = event_day_key(event, offset) else {
            continue;
        };
        if !range.contains_output_day(&day_key) {
            continue;
        }
        events_retained += 1;
        let day = by_day.entry(day_key).or_default();
        day.input_tokens += event.input_tokens;
        day.cached_input_tokens += event.cached_input_tokens;
        day.output_tokens += event.output_tokens;
        match event.cost_usd {
            Some(cost) => {
                day.cost_usd += cost;
                day.has_priced_cost = true;
            }
            None => {
                day.has_unpriced_cost = true;
            }
        }
        if day.model.is_none() {
            day.model = Some(event.model.clone());
        }
        if day.session_id.is_none() {
            day.session_id.clone_from(&event.session_id);
        }
        if day.project.is_none() {
            day.project.clone_from(&event.project);
        }
    }

    for (day_key, totals) in by_day {
        if !range.contains_output_day(&day_key) {
            continue;
        }
        let input_tokens = totals.input_tokens;
        let cached_input_tokens = totals.cached_input_tokens;
        let output_tokens = totals.output_tokens;
        let total_tokens = input_tokens + output_tokens;
        let day_cost = totals.cost_usd;
        let day_priced = !totals.has_unpriced_cost;
        last_30_days_tokens += total_tokens;
        if day_priced {
            last_30_days_cost += day_cost;
        } else if total_tokens > 0 {
            last_30_days_priced = false;
        }

        if day_key == today_key {
            today_tokens = total_tokens;
            today_cost = day_cost;
            today_priced = day_priced;
        }

        daily.push(CostUsageDailyPoint {
            day_key: day_key.clone(),
            model: totals.model,
            session_id: totals.session_id,
            project: totals.project,
            input_tokens,
            cached_input_tokens,
            output_tokens,
            total_tokens,
            cost_usd: day_priced.then_some(day_cost),
        });
    }

    CostUsageSnapshot {
        today_tokens,
        today_cost_usd: today_priced.then_some(today_cost),
        last_30_days_tokens,
        last_30_days_cost_usd: last_30_days_priced.then_some(last_30_days_cost),
        range_days: range.retention_days,
        timezone: Some(timezone_label(offset)),
        today_key: Some(today_key),
        scan_stats: Some(CostUsageScanStats {
            files_scanned: stats.files_scanned,
            files_reused: stats.files_reused,
            files_removed: stats.files_removed,
            events_retained,
            retention_days: range.retention_days,
        }),
        daily,
        updated_at: now.unix_timestamp(),
        source_root,
    }
}

#[derive(Default)]
struct DailyTotals {
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    cost_usd: f64,
    has_priced_cost: bool,
    has_unpriced_cost: bool,
    model: Option<String>,
    session_id: Option<String>,
    project: Option<String>,
}

fn prune_file_usage(usage: &mut CostUsageFileUsage, range: &DayRange) {
    usage.events.retain(|event| {
        event_day_key(event, UtcOffset::UTC)
            .map(|day_key| range.contains_scan_day(&day_key))
            .unwrap_or(false)
    });
}

fn event_day_key(event: &CostUsageEvent, offset: UtcOffset) -> Option<String> {
    if let Some(naive) = event.naive_day_key.as_deref() {
        return Some(naive.to_string());
    }
    day_key_from_unix_with_offset(event.timestamp_unix, offset)
}

fn build_session_index(files: &[PathBuf]) -> HashMap<String, PathBuf> {
    let mut out = HashMap::new();
    for file in files {
        if let Ok(Some(metadata)) = parse_session_metadata(file) {
            if let Some(session_id) = metadata.session_id {
                out.entry(session_id).or_insert_with(|| file.clone());
            }
        }
    }
    out
}

fn list_jsonl_files(root: &Path) -> Result<Vec<PathBuf>, AppError> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = match fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(AppError::AccountStore(error.to_string())),
        };
        for entry in entries {
            let entry = entry.map_err(|error| AppError::AccountStore(error.to_string()))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|error| AppError::AccountStore(error.to_string()))?;
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("jsonl"))
                    .unwrap_or(false)
            {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn first_jsonl_file(root: &Path) -> Option<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = fs::read_dir(&current).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("jsonl"))
                    .unwrap_or(false)
            {
                return Some(path);
            }
        }
    }
    None
}

fn roots_fingerprint(roots: &[PathBuf; 2]) -> BTreeMap<String, i64> {
    roots
        .iter()
        .map(|root| (root.to_string_lossy().to_string(), 0))
        .collect()
}

fn scan_jsonl_file<F>(
    path: &Path,
    offset: i64,
    max_line_bytes: usize,
    mut on_line: F,
) -> Result<i64, AppError>
where
    F: FnMut(&[u8]),
{
    let file = File::open(path).map_err(|error| AppError::AccountStore(error.to_string()))?;
    let mut reader = BufReader::with_capacity(256 * 1024, file);
    let start_offset = offset.max(0) as u64;
    if start_offset > 0 {
        reader
            .seek(SeekFrom::Start(start_offset))
            .map_err(|error| AppError::AccountStore(error.to_string()))?;
    }

    let mut bytes_read = 0_i64;
    let mut line = Vec::new();
    loop {
        line.clear();
        let count = reader
            .read_until(b'\n', &mut line)
            .map_err(|error| AppError::AccountStore(error.to_string()))?;
        if count == 0 {
            break;
        }
        if !line.ends_with(b"\n") {
            break;
        }
        bytes_read += i64::try_from(count).unwrap_or(0);
        line.pop();
        if line.ends_with(b"\r") {
            line.pop();
        }
        if line.len() <= max_line_bytes {
            on_line(&line);
        }
    }
    Ok(offset.max(0) + bytes_read)
}

fn line_has_interesting_codex_type(line: &[u8]) -> bool {
    contains_bytes(line, br#""type":"event_msg""#)
        || contains_bytes(line, br#""type": "event_msg""#)
        || contains_bytes(line, br#""type":"turn_context""#)
        || contains_bytes(line, br#""type": "turn_context""#)
        || contains_bytes(line, br#""type":"session_meta""#)
        || contains_bytes(line, br#""type": "session_meta""#)
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn load_cache(cache_root: &Path) -> Option<CostUsageCache> {
    let path = cache_path(cache_root);
    let contents = fs::read_to_string(path).ok()?;
    let decoded: CostUsageCache = serde_json::from_str(&contents).ok()?;
    (decoded.version == CACHE_VERSION).then_some(decoded)
}

fn save_cache(cache_root: &Path, cache: &CostUsageCache) -> Result<(), AppError> {
    let path = cache_path(cache_root);
    let parent = path.parent().ok_or_else(|| {
        AppError::AccountStore(format!(
            "cost usage cache path has no parent: {}",
            path.to_string_lossy()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| AppError::AccountStore(error.to_string()))?;
    let contents =
        serde_json::to_vec(cache).map_err(|error| AppError::AccountStore(error.to_string()))?;
    let tmp = temporary_file_path(parent, CACHE_FILE_NAME);
    write_new_file(&tmp, &contents).map_err(|error| AppError::AccountStore(error.to_string()))?;
    if let Err(error) = apply_secure_file_permissions(&tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    replace_file(&tmp, &path).map_err(|error| AppError::AccountStore(error.to_string()))?;
    Ok(())
}

fn cache_path(cache_root: &Path) -> PathBuf {
    cache_root.join("cost-usage").join(CACHE_FILE_NAME)
}

#[cfg(unix)]
fn apply_secure_file_permissions(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, permissions)
        .map_err(|error| AppError::AccountStore(error.to_string()))
}

#[cfg(not(unix))]
fn apply_secure_file_permissions(_path: &Path) -> Result<(), AppError> {
    Ok(())
}

fn metadata_mtime_ms(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

fn parse_timestamp(value: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).ok()
}

fn timestamp_unix(value: &str) -> Option<i64> {
    parse_timestamp(value).map(|timestamp| timestamp.unix_timestamp())
}

fn unix_from_day_key(day_key: &str) -> Option<i64> {
    let mut parts = day_key.split('-');
    let year = parts.next()?.parse::<i32>().ok()?;
    let month = parts.next()?.parse::<u8>().ok()?;
    let day = parts.next()?.parse::<u8>().ok()?;
    let date = time::Date::from_calendar_date(year, month.try_into().ok()?, day).ok()?;
    Some(date.midnight().assume_utc().unix_timestamp())
}

fn day_key_from_unix_with_offset(timestamp_unix: i64, offset: UtcOffset) -> Option<String> {
    OffsetDateTime::from_unix_timestamp(timestamp_unix)
        .ok()
        .map(|timestamp| day_key_from_datetime(timestamp.to_offset(offset)))
}

fn local_utc_offset() -> UtcOffset {
    UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC)
}

fn timezone_label(offset: UtcOffset) -> String {
    if offset == UtcOffset::UTC {
        return "UTC".to_string();
    }
    let seconds = offset.whole_seconds();
    let sign = if seconds < 0 { '-' } else { '+' };
    let seconds = seconds.abs();
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    format!("UTC{sign}{hours:02}:{minutes:02}")
}

fn normalize_range_days(range_days: u16) -> u16 {
    match range_days {
        7 | 30 | 90 => range_days,
        _ => DEFAULT_RANGE_DAYS,
    }
}

fn day_key_from_timestamp(value: &str) -> Option<String> {
    parse_timestamp(value)
        .map(day_key_from_datetime)
        .or_else(|| day_key_prefix(value))
}

fn day_key_prefix(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    if bytes.len() >= 10
        && bytes[0..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
    {
        Some(value[..10].to_string())
    } else {
        None
    }
}

fn day_key_from_datetime(value: OffsetDateTime) -> String {
    let date = value.date();
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

#[cfg(test)]
mod tests;
