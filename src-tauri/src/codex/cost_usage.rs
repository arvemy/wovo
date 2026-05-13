use crate::codex::account_store::default_wovo_codex_root;
use crate::domain::usage::{CostUsageDailyPoint, CostUsageSnapshot};
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

const CACHE_FILE_NAME: &str = "codex-v1.json";
const CACHE_VERSION: u16 = 1;
const DEFAULT_MODEL: &str = "gpt-5";

type PackedDays = BTreeMap<String, BTreeMap<String, Vec<i64>>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CostUsageCache {
    version: u16,
    last_scan_unix_ms: i64,
    files: BTreeMap<String, CostUsageFileUsage>,
    days: PackedDays,
    roots: Option<BTreeMap<String, i64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CostUsageFileUsage {
    mtime_unix_ms: i64,
    size: i64,
    days: PackedDays,
    parsed_bytes: Option<i64>,
    last_model: Option<String>,
    last_totals: Option<CodexTotals>,
    session_id: Option<String>,
    forked_from_id: Option<String>,
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
}

#[derive(Debug, Clone)]
struct ParseResult {
    days: PackedDays,
    parsed_bytes: i64,
    last_model: Option<String>,
    last_totals: Option<CodexTotals>,
    session_id: Option<String>,
    forked_from_id: Option<String>,
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

#[derive(Debug, Clone, Copy)]
struct CodexPricing {
    input_cost_per_token: f64,
    output_cost_per_token: f64,
    cache_read_input_cost_per_token: Option<f64>,
}

pub fn local_codex_logs_exist(codex_home: &Path) -> bool {
    [
        codex_home.join("sessions"),
        codex_home.join("archived_sessions"),
    ]
    .into_iter()
    .any(|root| first_jsonl_file(&root).is_some())
}

pub fn load_cost_usage_snapshot(
    source_root: PathBuf,
    force_rescan: bool,
) -> Result<CostUsageSnapshot, AppError> {
    load_cost_usage_snapshot_at(
        source_root,
        default_wovo_codex_root().join("cache"),
        OffsetDateTime::now_utc(),
        force_rescan,
    )
}

fn load_cost_usage_snapshot_at(
    source_root: PathBuf,
    cache_root: PathBuf,
    now: OffsetDateTime,
    force_rescan: bool,
) -> Result<CostUsageSnapshot, AppError> {
    let range = DayRange::new(now);
    let mut cache = if force_rescan {
        CostUsageCache::new()
    } else {
        load_cache(&cache_root).unwrap_or_default()
    };

    scan_roots(&source_root, &range, &mut cache, now, force_rescan)?;
    save_cache(&cache_root, &cache)?;

    Ok(build_snapshot_from_cache(
        &cache,
        &range,
        now,
        source_root.to_string_lossy().to_string(),
    ))
}

impl CostUsageCache {
    fn new() -> Self {
        Self {
            version: CACHE_VERSION,
            last_scan_unix_ms: 0,
            files: BTreeMap::new(),
            days: BTreeMap::new(),
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
    fn new(now: OffsetDateTime) -> Self {
        let since = now - Duration::days(29);
        Self {
            since_key: day_key_from_datetime(since),
            until_key: day_key_from_datetime(now),
            scan_since_key: day_key_from_datetime(since - Duration::days(1)),
            scan_until_key: day_key_from_datetime(now + Duration::days(1)),
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
) -> Result<(), AppError> {
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

    for file in files {
        file_paths_in_scan.insert(file.to_string_lossy().to_string());
        scan_file_into_cache(&file, range, cache, &mut seen_session_ids, &session_index)?;
    }

    let stale_paths: Vec<String> = cache
        .files
        .keys()
        .filter(|path| !file_paths_in_scan.contains(*path))
        .cloned()
        .collect();
    for path in stale_paths {
        if let Some(old) = cache.files.remove(&path) {
            apply_file_days(&mut cache.days, &old.days, -1);
        }
    }

    prune_days(
        &mut cache.days,
        &range.scan_since_key,
        &range.scan_until_key,
    );
    cache.roots = Some(roots_fingerprint);
    cache.last_scan_unix_ms = now.unix_timestamp() * 1000;
    Ok(())
}

fn scan_file_into_cache(
    file: &Path,
    range: &DayRange,
    cache: &mut CostUsageCache,
    seen_session_ids: &mut HashSet<String>,
    session_index: &HashMap<String, PathBuf>,
) -> Result<(), AppError> {
    let path = file.to_string_lossy().to_string();
    let metadata = match fs::metadata(file) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Some(old) = cache.files.remove(&path) {
                apply_file_days(&mut cache.days, &old.days, -1);
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
            if let Some(old) = cache.files.remove(&path) {
                apply_file_days(&mut cache.days, &old.days, -1);
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
                    if let Some(old) = cache.files.remove(&path) {
                        apply_file_days(&mut cache.days, &old.days, -1);
                    }
                    return Ok(());
                }
            }

            if !parsed.days.is_empty() {
                apply_file_days(&mut cache.days, &parsed.days, 1);
            }

            let mut merged_days = cached.days.clone();
            merge_file_days(&mut merged_days, &parsed.days);
            cache.files.insert(
                path,
                CostUsageFileUsage {
                    mtime_unix_ms,
                    size,
                    days: merged_days,
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
            return Ok(());
        }
    }

    if let Some(old) = cached {
        apply_file_days(&mut cache.days, &old.days, -1);
    }

    let inherited = inherited_totals(file, session_index)?;
    let parsed = parse_codex_file(file, range, 0, None, None, inherited)?;
    let session_id = parsed.session_id.clone();
    if let Some(session_id) = session_id.as_deref() {
        if seen_session_ids.contains(session_id) {
            cache.files.remove(&path);
            return Ok(());
        }
    }

    apply_file_days(&mut cache.days, &parsed.days, 1);
    cache.files.insert(
        path,
        CostUsageFileUsage {
            mtime_unix_ms,
            size,
            days: parsed.days,
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
    let mut days: PackedDays = BTreeMap::new();

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
        add_day_usage(
            &mut days,
            range,
            &day_key,
            &model,
            delta.input,
            cached,
            delta.output,
        );
    })?;

    Ok(ParseResult {
        days,
        parsed_bytes,
        last_model: current_model,
        last_totals: previous_totals,
        session_id,
        forked_from_id,
    })
}

fn add_day_usage(
    days: &mut PackedDays,
    range: &DayRange,
    day_key: &str,
    model: &str,
    input: i64,
    cached: i64,
    output: i64,
) {
    if !range.contains_scan_day(day_key) {
        return;
    }
    let normalized_model = normalize_codex_model(model);
    let day_models = days.entry(day_key.to_string()).or_default();
    let packed = day_models
        .entry(normalized_model)
        .or_insert_with(|| vec![0, 0, 0]);
    ensure_packed_len(packed);
    packed[0] += input;
    packed[1] += cached;
    packed[2] += output;
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
    source_root: String,
) -> CostUsageSnapshot {
    let today_key = day_key_from_datetime(now);
    let mut daily = Vec::new();
    let mut last_30_days_tokens = 0;
    let mut last_30_days_cost = 0.0;
    let mut last_30_days_priced = true;
    let mut today_tokens = 0;
    let mut today_cost = 0.0;
    let mut today_priced = true;

    for (day_key, models) in &cache.days {
        if !range.contains_output_day(day_key) {
            continue;
        }
        let mut input_tokens = 0;
        let mut cached_input_tokens = 0;
        let mut output_tokens = 0;
        let mut day_cost = 0.0;
        let mut day_priced = true;

        for (model, packed) in models {
            let input = packed_value(packed, 0);
            let cached = packed_value(packed, 1);
            let output = packed_value(packed, 2);
            input_tokens += input;
            cached_input_tokens += cached;
            output_tokens += output;
            if let Some(cost) = codex_cost_usd(model, input, cached, output) {
                day_cost += cost;
            } else {
                day_priced = false;
            }
        }

        let total_tokens = input_tokens + output_tokens;
        last_30_days_tokens += total_tokens;
        if day_priced {
            last_30_days_cost += day_cost;
        } else if total_tokens > 0 {
            last_30_days_priced = false;
        }

        if day_key == &today_key {
            today_tokens = total_tokens;
            today_cost = day_cost;
            today_priced = day_priced;
        }

        daily.push(CostUsageDailyPoint {
            day_key: day_key.clone(),
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
        daily,
        updated_at: now.unix_timestamp(),
        source_root,
    }
}

fn codex_cost_usd(
    model: &str,
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
) -> Option<f64> {
    let pricing = codex_pricing(&normalize_codex_model(model))?;
    let cached = cached_input_tokens.max(0).min(input_tokens.max(0));
    let non_cached = (input_tokens - cached).max(0);
    let cached_rate = pricing
        .cache_read_input_cost_per_token
        .unwrap_or(pricing.input_cost_per_token);
    Some(
        non_cached as f64 * pricing.input_cost_per_token
            + cached as f64 * cached_rate
            + output_tokens.max(0) as f64 * pricing.output_cost_per_token,
    )
}

fn codex_pricing(model: &str) -> Option<CodexPricing> {
    let pricing = match model {
        "gpt-5" | "gpt-5-codex" | "gpt-5.1" | "gpt-5.1-codex" | "gpt-5.1-codex-max" => {
            CodexPricing {
                input_cost_per_token: 1.25e-6,
                output_cost_per_token: 1e-5,
                cache_read_input_cost_per_token: Some(1.25e-7),
            }
        }
        "gpt-5-mini" => CodexPricing {
            input_cost_per_token: 2.5e-7,
            output_cost_per_token: 2e-6,
            cache_read_input_cost_per_token: Some(2.5e-8),
        },
        "gpt-5-nano" => CodexPricing {
            input_cost_per_token: 5e-8,
            output_cost_per_token: 4e-7,
            cache_read_input_cost_per_token: Some(5e-9),
        },
        "gpt-5-pro" => CodexPricing {
            input_cost_per_token: 1.5e-5,
            output_cost_per_token: 1.2e-4,
            cache_read_input_cost_per_token: None,
        },
        "gpt-5.1-codex-mini" => CodexPricing {
            input_cost_per_token: 2.5e-7,
            output_cost_per_token: 2e-6,
            cache_read_input_cost_per_token: Some(2.5e-8),
        },
        "gpt-5.2" | "gpt-5.2-codex" | "gpt-5.3-codex" => CodexPricing {
            input_cost_per_token: 1.75e-6,
            output_cost_per_token: 1.4e-5,
            cache_read_input_cost_per_token: Some(1.75e-7),
        },
        "gpt-5.2-pro" => CodexPricing {
            input_cost_per_token: 2.1e-5,
            output_cost_per_token: 1.68e-4,
            cache_read_input_cost_per_token: None,
        },
        "gpt-5.3-codex-spark" => CodexPricing {
            input_cost_per_token: 0.0,
            output_cost_per_token: 0.0,
            cache_read_input_cost_per_token: Some(0.0),
        },
        "gpt-5.4" => CodexPricing {
            input_cost_per_token: 2.5e-6,
            output_cost_per_token: 1.5e-5,
            cache_read_input_cost_per_token: Some(2.5e-7),
        },
        "gpt-5.4-mini" => CodexPricing {
            input_cost_per_token: 7.5e-7,
            output_cost_per_token: 4.5e-6,
            cache_read_input_cost_per_token: Some(7.5e-8),
        },
        "gpt-5.4-nano" => CodexPricing {
            input_cost_per_token: 2e-7,
            output_cost_per_token: 1.25e-6,
            cache_read_input_cost_per_token: Some(2e-8),
        },
        "gpt-5.4-pro" | "gpt-5.5-pro" => CodexPricing {
            input_cost_per_token: 3e-5,
            output_cost_per_token: 1.8e-4,
            cache_read_input_cost_per_token: None,
        },
        "gpt-5.5" => CodexPricing {
            input_cost_per_token: 5e-6,
            output_cost_per_token: 3e-5,
            cache_read_input_cost_per_token: Some(5e-7),
        },
        _ => return None,
    };
    Some(pricing)
}

fn normalize_codex_model(raw: &str) -> String {
    let mut trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("openai/") {
        trimmed = rest;
    }
    if codex_pricing_exact(trimmed) {
        return trimmed.to_string();
    }
    if let Some(base) = strip_dated_suffix(trimmed) {
        if codex_pricing_exact(base) {
            return base.to_string();
        }
    }
    trimmed.to_string()
}

fn codex_pricing_exact(model: &str) -> bool {
    codex_pricing(model).is_some()
}

fn strip_dated_suffix(value: &str) -> Option<&str> {
    if value.len() < 11 {
        return None;
    }
    let suffix = &value[value.len() - 11..];
    let bytes = suffix.as_bytes();
    if bytes[0] == b'-'
        && bytes[1..5].iter().all(u8::is_ascii_digit)
        && bytes[5] == b'-'
        && bytes[6..8].iter().all(u8::is_ascii_digit)
        && bytes[8] == b'-'
        && bytes[9..11].iter().all(u8::is_ascii_digit)
    {
        Some(&value[..value.len() - 11])
    } else {
        None
    }
}

fn apply_file_days(cache_days: &mut PackedDays, file_days: &PackedDays, sign: i64) {
    for (day, models) in file_days {
        let remove_day = {
            let day_models = cache_days.entry(day.clone()).or_default();
            for (model, packed) in models {
                let existing = day_models
                    .entry(model.clone())
                    .or_insert_with(|| vec![0, 0, 0]);
                ensure_packed_len(existing);
                for (index, value) in existing.iter_mut().enumerate().take(3) {
                    *value = (*value + sign * packed_value(packed, index)).max(0);
                }
                if existing.iter().all(|value| *value == 0) {
                    day_models.remove(model);
                }
            }
            day_models.is_empty()
        };
        if remove_day {
            cache_days.remove(day);
        }
    }
}

fn merge_file_days(existing: &mut PackedDays, delta: &PackedDays) {
    apply_file_days(existing, delta, 1);
}

fn prune_days(days: &mut PackedDays, since_key: &str, until_key: &str) {
    let stale: Vec<String> = days
        .keys()
        .filter(|day| day.as_str() < since_key || day.as_str() > until_key)
        .cloned()
        .collect();
    for key in stale {
        days.remove(&key);
    }
}

fn ensure_packed_len(values: &mut Vec<i64>) {
    while values.len() < 3 {
        values.push(0);
    }
}

fn packed_value(values: &[i64], index: usize) -> i64 {
    values.get(index).copied().unwrap_or(0)
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
    let tmp = parent.join(format!(".{CACHE_FILE_NAME}.{}.tmp", unique_nonce()));
    fs::write(&tmp, contents).map_err(|error| AppError::AccountStore(error.to_string()))?;
    apply_secure_file_permissions(&tmp)?;
    replace_file(&tmp, &path)?;
    Ok(())
}

fn cache_path(cache_root: &Path) -> PathBuf {
    cache_root.join("cost-usage").join(CACHE_FILE_NAME)
}

fn replace_file(tmp: &Path, target: &Path) -> Result<(), AppError> {
    match fs::rename(tmp, target) {
        Ok(()) => Ok(()),
        Err(error) => {
            #[cfg(windows)]
            {
                if target.exists() {
                    fs::remove_file(target)
                        .map_err(|remove_error| AppError::AccountStore(remove_error.to_string()))?;
                    fs::rename(tmp, target)
                        .map_err(|rename_error| AppError::AccountStore(rename_error.to_string()))
                } else {
                    let _ = fs::remove_file(tmp);
                    Err(AppError::AccountStore(error.to_string()))
                }
            }
            #[cfg(not(windows))]
            {
                let _ = fs::remove_file(tmp);
                Err(AppError::AccountStore(error.to_string()))
            }
        }
    }
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

fn unique_nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
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
mod tests {
    use super::*;
    use uuid::Uuid;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("wovo-cost-{name}-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_session(root: &Path, day_key: &str, filename: &str, lines: &[Value]) -> PathBuf {
        let parts: Vec<&str> = day_key.split('-').collect();
        let dir = root
            .join("sessions")
            .join(parts[0])
            .join(parts[1])
            .join(parts[2]);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(filename);
        fs::write(&path, jsonl(lines)).unwrap();
        path
    }

    fn write_archived(root: &Path, filename: &str, lines: &[Value]) -> PathBuf {
        let dir = root.join("archived_sessions");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(filename);
        fs::write(&path, jsonl(lines)).unwrap();
        path
    }

    fn jsonl(lines: &[Value]) -> String {
        lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    }

    fn now() -> OffsetDateTime {
        OffsetDateTime::parse("2026-04-12T12:00:00Z", &Rfc3339).unwrap()
    }

    fn token_total(timestamp: &str, model: &str, input: i64, cached: i64, output: i64) -> Value {
        serde_json::json!({
            "type": "event_msg",
            "timestamp": timestamp,
            "payload": {
                "type": "token_count",
                "info": {
                    "model": model,
                    "total_token_usage": {
                        "input_tokens": input,
                        "cached_input_tokens": cached,
                        "output_tokens": output
                    }
                }
            }
        })
    }

    fn token_last(timestamp: &str, model: &str, input: i64, cached: i64, output: i64) -> Value {
        serde_json::json!({
            "type": "event_msg",
            "timestamp": timestamp,
            "payload": {
                "type": "token_count",
                "info": {
                    "model": model,
                    "last_token_usage": {
                        "input_tokens": input,
                        "cached_input_tokens": cached,
                        "output_tokens": output
                    }
                }
            }
        })
    }

    fn session_meta(session_id: &str, forked_from_id: Option<&str>, timestamp: &str) -> Value {
        let mut payload = serde_json::json!({ "session_id": session_id });
        if let Some(forked_from_id) = forked_from_id {
            payload["forked_from_id"] = Value::String(forked_from_id.to_string());
        }
        serde_json::json!({
            "type": "session_meta",
            "timestamp": timestamp,
            "payload": payload
        })
    }

    #[test]
    fn parses_total_token_usage_deltas() {
        let root = temp_root("total-deltas");
        let cache = root.join("cache");
        write_session(
            &root,
            "2026-04-12",
            "session.jsonl",
            &[
                token_total("2026-04-12T10:00:00Z", "gpt-5.4", 100, 20, 10),
                token_total("2026-04-12T10:01:00Z", "gpt-5.4", 160, 30, 16),
            ],
        );

        let snapshot = load_cost_usage_snapshot_at(root.clone(), cache, now(), false).unwrap();

        assert_eq!(snapshot.today_tokens, 176);
        assert_eq!(snapshot.daily[0].input_tokens, 160);
        assert_eq!(snapshot.daily[0].cached_input_tokens, 30);
        assert_eq!(snapshot.daily[0].output_tokens, 16);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parses_last_token_usage_rows() {
        let root = temp_root("last-rows");
        let cache = root.join("cache");
        write_session(
            &root,
            "2026-04-12",
            "session.jsonl",
            &[
                token_last("2026-04-12T10:00:00Z", "gpt-5.4", 50, 5, 3),
                token_last("2026-04-12T10:01:00Z", "gpt-5.4", 70, 7, 5),
            ],
        );

        let snapshot = load_cost_usage_snapshot_at(root.clone(), cache, now(), false).unwrap();

        assert_eq!(snapshot.today_tokens, 128);
        assert_eq!(snapshot.daily[0].input_tokens, 120);
        assert_eq!(snapshot.daily[0].cached_input_tokens, 12);
        assert_eq!(snapshot.daily[0].output_tokens, 8);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn turn_context_model_wins_over_token_payload_model() {
        let root = temp_root("turn-context");
        let cache = root.join("cache");
        write_session(
            &root,
            "2026-04-12",
            "session.jsonl",
            &[
                serde_json::json!({
                    "type": "turn_context",
                    "timestamp": "2026-04-12T09:59:59Z",
                    "payload": { "model": "openai/gpt-5.4" }
                }),
                token_total("2026-04-12T10:00:00Z", "gpt-5", 100, 20, 10),
            ],
        );

        let snapshot = load_cost_usage_snapshot_at(root.clone(), cache, now(), false).unwrap();
        let expected = codex_cost_usd("gpt-5.4", 100, 20, 10).unwrap();

        assert!((snapshot.today_cost_usd.unwrap() - expected).abs() < 0.000001);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cached_input_tokens_use_cache_read_pricing() {
        let priced = codex_cost_usd("gpt-5.4", 100, 20, 10).unwrap();
        let uncached = codex_cost_usd("gpt-5.4", 100, 0, 10).unwrap();

        assert!(priced < uncached);
    }

    #[test]
    fn scans_archived_flat_session_files() {
        let root = temp_root("archived");
        let cache = root.join("cache");
        write_archived(
            &root,
            "rollout-2026-04-12T10-00-00-session.jsonl",
            &[
                serde_json::json!({
                    "type": "session_meta",
                    "payload": { "session_id": "archived-session" }
                }),
                token_last("2026-04-12T10:00:00Z", "gpt-5.4", 33, 3, 4),
            ],
        );

        let snapshot = load_cost_usage_snapshot_at(root.clone(), cache, now(), false).unwrap();

        assert_eq!(snapshot.today_tokens, 37);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unknown_model_keeps_tokens_with_null_cost() {
        let root = temp_root("unknown-model");
        let cache = root.join("cache");
        write_session(
            &root,
            "2026-04-12",
            "session.jsonl",
            &[token_total(
                "2026-04-12T10:00:00Z",
                "unknown-model",
                100,
                0,
                10,
            )],
        );

        let snapshot = load_cost_usage_snapshot_at(root.clone(), cache, now(), false).unwrap();

        assert_eq!(snapshot.today_tokens, 110);
        assert_eq!(snapshot.today_cost_usd, None);
        assert_eq!(snapshot.daily[0].cost_usd, None);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn incremental_cache_reuses_previous_totals_after_append() {
        let root = temp_root("incremental");
        let cache = root.join("cache");
        let path = write_session(
            &root,
            "2026-04-12",
            "session.jsonl",
            &[token_total("2026-04-12T10:00:00Z", "gpt-5.4", 100, 20, 10)],
        );

        let first = load_cost_usage_snapshot_at(root.clone(), cache.clone(), now(), false).unwrap();
        assert_eq!(first.today_tokens, 110);

        fs::write(
            &path,
            jsonl(&[
                token_total("2026-04-12T10:00:00Z", "gpt-5.4", 100, 20, 10),
                token_total("2026-04-12T10:01:00Z", "gpt-5.4", 150, 30, 15),
            ]),
        )
        .unwrap();

        let second = load_cost_usage_snapshot_at(root.clone(), cache, now(), false).unwrap();

        assert_eq!(second.today_tokens, 165);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn incremental_cache_reparses_unterminated_tail_after_append() {
        let root = temp_root("incremental-partial-tail");
        let cache = root.join("cache");
        let path = write_session(
            &root,
            "2026-04-12",
            "session.jsonl",
            &[token_total("2026-04-12T10:00:00Z", "gpt-5.4", 100, 20, 10)],
        );
        let first_line = jsonl(&[token_total("2026-04-12T10:00:00Z", "gpt-5.4", 100, 20, 10)]);
        let second_line = token_total("2026-04-12T10:01:00Z", "gpt-5.4", 150, 30, 15).to_string();
        let split_at = second_line.len() / 2;

        fs::write(&path, format!("{first_line}{}", &second_line[..split_at])).unwrap();
        let first = load_cost_usage_snapshot_at(root.clone(), cache.clone(), now(), false).unwrap();
        assert_eq!(first.today_tokens, 110);

        fs::write(&path, format!("{first_line}{second_line}\n")).unwrap();
        let second = load_cost_usage_snapshot_at(root.clone(), cache, now(), false).unwrap();

        assert_eq!(second.today_tokens, 165);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn forked_last_token_usage_rows_are_counted_as_deltas() {
        let root = temp_root("forked-last-rows");
        let cache = root.join("cache");
        write_session(
            &root,
            "2026-04-12",
            "parent.jsonl",
            &[
                session_meta("parent-session", None, "2026-04-12T09:00:00Z"),
                token_total("2026-04-12T10:00:00Z", "gpt-5.4", 1000, 100, 100),
            ],
        );
        write_session(
            &root,
            "2026-04-12",
            "child.jsonl",
            &[
                session_meta(
                    "child-session",
                    Some("parent-session"),
                    "2026-04-12T10:30:00Z",
                ),
                token_last("2026-04-12T10:31:00Z", "gpt-5.4", 100, 10, 10),
            ],
        );

        let snapshot = load_cost_usage_snapshot_at(root.clone(), cache, now(), false).unwrap();

        assert_eq!(snapshot.today_tokens, 1210);
        assert_eq!(snapshot.daily[0].input_tokens, 1100);
        assert_eq!(snapshot.daily[0].cached_input_tokens, 110);
        assert_eq!(snapshot.daily[0].output_tokens, 110);
        let _ = fs::remove_dir_all(root);
    }
}
