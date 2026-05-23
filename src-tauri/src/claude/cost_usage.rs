use crate::claude::account_store::default_wovo_claude_root;
use crate::codex::atomic_file::{replace_file, temporary_file_path, write_new_file};
use crate::domain::usage::{CostUsageDailyPoint, CostUsageScanStats, CostUsageSnapshot};
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime, UtcOffset};

mod pricing;

use pricing::{claude_cost_usd, normalize_claude_model};

const CACHE_FILE_NAME: &str = "claude-v2.json";
const CACHE_VERSION: u16 = 3;
const DEFAULT_RANGE_DAYS: u16 = 30;

#[derive(Debug, Clone)]
struct ClaudeUsageRow {
    timestamp_unix: i64,
    model: String,
    session_id: Option<String>,
    project: Option<String>,
    dedupe_key: Option<String>,
    input_tokens: i64,
    cache_read_tokens: i64,
    cache_create_tokens: i64,
    output_tokens: i64,
    cost_usd: Option<f64>,
    naive_day_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeCostUsageCache {
    version: u16,
    last_scan_unix_ms: i64,
    retention_days: u16,
    files: BTreeMap<String, ClaudeCostUsageFile>,
    roots: Option<BTreeMap<String, i64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeCostUsageFile {
    mtime_unix_ms: i64,
    size: i64,
    parsed_bytes: i64,
    events: Vec<ClaudeCostUsageEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeCostUsageEvent {
    timestamp_unix: i64,
    model: String,
    session_id: Option<String>,
    project: Option<String>,
    dedupe_key: Option<String>,
    input_tokens: i64,
    cache_read_tokens: i64,
    cache_create_tokens: i64,
    output_tokens: i64,
    cost_usd: Option<f64>,
    // Set only when the source row lacked an offset/Z and we synthesized
    // timestamp_unix from the parsed YYYY-MM-DD prefix. The string anchors
    // the bucket on the wire-format day so a later local-offset rebucket
    // cannot shift the row off its original calendar day.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    naive_day_key: Option<String>,
}

#[derive(Debug, Clone)]
struct FileParseResult {
    parsed_bytes: i64,
    events: Vec<ClaudeCostUsageEvent>,
}

#[derive(Debug, Clone)]
struct DayRange {
    since_key: String,
    until_key: String,
    scan_since_key: String,
    scan_until_key: String,
    retention_days: u16,
}

#[derive(Debug, Clone, Default)]
struct ScanStatsAccumulator {
    files_scanned: usize,
    files_reused: usize,
    files_removed: usize,
}

pub fn local_claude_logs_exist(claude_home: &Path) -> bool {
    local_claude_logs_exist_in_project_roots(&claude_projects_roots(claude_home))
}

pub fn load_cost_usage_snapshot_with_range(
    source_root: PathBuf,
    force_rescan: bool,
    range_days: u16,
) -> Result<CostUsageSnapshot, AppError> {
    load_cost_usage_snapshot_at(
        source_root,
        default_wovo_claude_root().join("cache"),
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
    let offset = local_utc_offset();
    let range = DayRange::new(now, range_days, offset);
    let roots = claude_projects_roots(&source_root);
    load_cost_usage_snapshot_for_roots_at(roots, cache_root, now, force_rescan, range, offset)
}

fn load_cost_usage_snapshot_for_roots_at(
    roots: Vec<PathBuf>,
    cache_root: PathBuf,
    now: OffsetDateTime,
    force_rescan: bool,
    range: DayRange,
    offset: UtcOffset,
) -> Result<CostUsageSnapshot, AppError> {
    let mut cache = if force_rescan {
        ClaudeCostUsageCache::new()
    } else {
        load_cache(&cache_root).unwrap_or_default()
    };
    if cache.retention_days < range.retention_days {
        cache = ClaudeCostUsageCache::new();
    }
    cache.retention_days = range.retention_days;
    let stats = scan_roots(&roots, &range, &mut cache, now)?;
    save_cache(&cache_root, &cache)?;
    Ok(build_snapshot_from_cache(
        &cache, roots, &range, now, offset, stats,
    ))
}

impl ClaudeCostUsageCache {
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

impl Default for ClaudeCostUsageCache {
    fn default() -> Self {
        Self::new()
    }
}

impl DayRange {
    fn new(now: OffsetDateTime, range_days: u16, offset: UtcOffset) -> Self {
        let retention_days = normalize_range_days(range_days);
        let local_now = now.to_offset(offset);
        let since = local_now - Duration::days(i64::from(retention_days.saturating_sub(1)));
        Self {
            since_key: day_key_from_datetime(since),
            until_key: day_key_from_datetime(local_now),
            scan_since_key: day_key_from_datetime(since - Duration::days(1)),
            scan_until_key: day_key_from_datetime(local_now + Duration::days(1)),
            retention_days,
        }
    }

    fn contains_output_day(&self, day_key: &str) -> bool {
        day_key >= self.since_key.as_str() && day_key <= self.until_key.as_str()
    }

    fn contains_scan_day(&self, day_key: &str) -> bool {
        day_key >= self.scan_since_key.as_str() && day_key <= self.scan_until_key.as_str()
    }
}

fn claude_projects_roots(source_root: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if source_root.exists() {
        roots.push(projects_root_for_config_dir(source_root));
    }
    for root in default_claude_projects_roots() {
        if !roots.iter().any(|existing| existing == &root) {
            roots.push(root);
        }
    }
    roots
}

fn local_claude_logs_exist_in_project_roots(roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| first_jsonl_file(root).is_some())
}

fn default_claude_projects_roots() -> Vec<PathBuf> {
    if let Ok(raw) = env::var("CLAUDE_CONFIG_DIR") {
        let roots: Vec<PathBuf> = raw
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(PathBuf::from)
            .map(|root| projects_root_for_config_dir(&root))
            .collect();
        if !roots.is_empty() {
            return roots;
        }
    }

    let home = dirs_home();
    vec![
        home.join(".config").join("claude").join("projects"),
        home.join(".claude").join("projects"),
    ]
}

fn projects_root_for_config_dir(root: &Path) -> PathBuf {
    if root.file_name().and_then(|name| name.to_str()) == Some("projects") {
        root.to_path_buf()
    } else {
        root.join("projects")
    }
}

fn scan_roots(
    roots: &[PathBuf],
    range: &DayRange,
    cache: &mut ClaudeCostUsageCache,
    now: OffsetDateTime,
) -> Result<ScanStatsAccumulator, AppError> {
    let mut files = Vec::new();
    let mut file_paths_in_scan = HashSet::new();
    for root in roots {
        for file in list_jsonl_files(root)? {
            file_paths_in_scan.insert(file.to_string_lossy().to_string());
            files.push(file);
        }
    }
    let mut stats = ScanStatsAccumulator::default();
    for file in files {
        scan_file_into_cache(&file, range, cache, &mut stats)?;
    }

    let stale_paths = cache
        .files
        .keys()
        .filter(|path| !file_paths_in_scan.contains(*path))
        .cloned()
        .collect::<Vec<_>>();
    for path in stale_paths {
        cache.files.remove(&path);
        stats.files_removed += 1;
    }
    for usage in cache.files.values_mut() {
        prune_file_usage(usage, range);
    }
    cache.roots = Some(roots_fingerprint(roots));
    cache.last_scan_unix_ms = now.unix_timestamp() * 1000;
    Ok(stats)
}

fn scan_file_into_cache(
    file: &Path,
    range: &DayRange,
    cache: &mut ClaudeCostUsageCache,
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
        Err(error) => return Err(AppError::ClaudeAccountStore(error.to_string())),
    };
    let mtime_unix_ms = metadata_mtime_ms(&metadata);
    let size = i64::try_from(metadata.len()).unwrap_or(i64::MAX);
    let cached = cache.files.get(&path).cloned();
    if let Some(cached) = cached.as_ref() {
        if cached.mtime_unix_ms == mtime_unix_ms && cached.size == size {
            stats.files_reused += 1;
            return Ok(());
        }
    }

    if let Some(cached) = cached.as_ref() {
        let can_incremental =
            size > cached.size && cached.parsed_bytes > 0 && cached.parsed_bytes <= size;
        if can_incremental {
            let parsed = parse_file(file, range, cached.parsed_bytes)?;
            let mut events = cached.events.clone();
            events.extend(parsed.events);
            cache.files.insert(
                path,
                ClaudeCostUsageFile {
                    mtime_unix_ms,
                    size,
                    parsed_bytes: parsed.parsed_bytes,
                    events,
                },
            );
            stats.files_scanned += 1;
            return Ok(());
        }
    }

    let parsed = parse_file(file, range, 0)?;
    cache.files.insert(
        path,
        ClaudeCostUsageFile {
            mtime_unix_ms,
            size,
            parsed_bytes: parsed.parsed_bytes,
            events: parsed.events,
        },
    );
    stats.files_scanned += 1;
    Ok(())
}

fn parse_file(
    file: &Path,
    range: &DayRange,
    start_offset: i64,
) -> Result<FileParseResult, AppError> {
    let project = project_label_for_file(file);
    let mut events = Vec::new();
    let parsed_bytes = scan_jsonl_file(file, start_offset, 512 * 1024, |line| {
        if !contains_bytes(line, br#""type":"assistant""#)
            && !contains_bytes(line, br#""type": "assistant""#)
        {
            return;
        }
        if !contains_bytes(line, br#""usage""#) {
            return;
        }
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        let Some(row) = parse_usage_row(&value, range) else {
            return;
        };
        events.push(ClaudeCostUsageEvent {
            timestamp_unix: row.timestamp_unix,
            model: row.model,
            session_id: row.session_id,
            project: row.project.or_else(|| project.clone()),
            dedupe_key: row.dedupe_key,
            input_tokens: row.input_tokens,
            cache_read_tokens: row.cache_read_tokens,
            cache_create_tokens: row.cache_create_tokens,
            output_tokens: row.output_tokens,
            cost_usd: row.cost_usd,
            naive_day_key: row.naive_day_key,
        });
    })?;
    Ok(FileParseResult {
        parsed_bytes,
        events,
    })
}

fn parse_usage_row(value: &Value, range: &DayRange) -> Option<ClaudeUsageRow> {
    if value.get("type")?.as_str()? != "assistant" {
        return None;
    }
    let timestamp = value.get("timestamp")?.as_str()?;
    let day_key = day_key_from_timestamp(timestamp)?;
    if !range.contains_scan_day(&day_key) {
        return None;
    }
    let (timestamp_unix, naive_day_key) = match timestamp_unix(timestamp) {
        Some(unix) => (unix, None),
        None => (unix_from_day_key(&day_key)?, Some(day_key.clone())),
    };

    let message = value.get("message")?;
    let model = message.get("model")?.as_str()?;
    let usage = message.get("usage")?;
    let input_tokens = token_value(usage, "input_tokens");
    let cache_read_tokens = token_value(usage, "cache_read_input_tokens");
    let cache_create_tokens = token_value(usage, "cache_creation_input_tokens");
    let output_tokens = token_value(usage, "output_tokens");
    if input_tokens == 0 && cache_read_tokens == 0 && cache_create_tokens == 0 && output_tokens == 0
    {
        return None;
    }
    let normalized_model = normalize_claude_model(model);
    let cost_usd = claude_cost_usd(
        &normalized_model,
        input_tokens,
        cache_read_tokens,
        cache_create_tokens,
        output_tokens,
    );
    let message_id = value
        .pointer("/message/id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let request_id = value
        .get("requestId")
        .and_then(Value::as_str)
        .map(str::to_string);
    let dedupe_key = match (message_id.as_deref(), request_id.as_deref()) {
        (Some(message_id), Some(request_id)) => Some(format!("{message_id}:{request_id}")),
        _ => None,
    };
    let session_id = value
        .get("sessionId")
        .or_else(|| value.get("session_id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let project = value
        .get("cwd")
        .or_else(|| value.get("project"))
        .and_then(Value::as_str)
        .and_then(|path| {
            Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        });

    Some(ClaudeUsageRow {
        timestamp_unix,
        model: normalized_model,
        session_id,
        project,
        dedupe_key,
        input_tokens,
        cache_read_tokens,
        cache_create_tokens,
        output_tokens,
        cost_usd,
        naive_day_key,
    })
}

fn token_value(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0).max(0)
}

fn build_snapshot_from_cache(
    cache: &ClaudeCostUsageCache,
    roots: Vec<PathBuf>,
    range: &DayRange,
    now: OffsetDateTime,
    offset: UtcOffset,
    stats: ScanStatsAccumulator,
) -> CostUsageSnapshot {
    let mut by_day = BTreeMap::<String, DailyTotals>::new();
    let mut keyed_events = HashMap::<String, ClaudeCostUsageEvent>::new();
    let mut unkeyed_events = Vec::new();
    for event in cache.files.values().flat_map(|file| file.events.iter()) {
        if let Some(key) = event.dedupe_key.as_ref() {
            keyed_events.insert(key.clone(), event.clone());
        } else {
            unkeyed_events.push(event.clone());
        }
    }
    let mut events_retained = 0;
    for event in keyed_events.into_values().chain(unkeyed_events) {
        let Some(day_key) = event_day_key(&event, offset) else {
            continue;
        };
        if !range.contains_output_day(&day_key) {
            continue;
        }
        events_retained += 1;
        let day = by_day.entry(day_key).or_default();
        day.input_tokens += event.input_tokens;
        day.cached_input_tokens += event.cache_read_tokens + event.cache_create_tokens;
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
            day.model = Some(event.model);
        }
        if day.session_id.is_none() {
            day.session_id = event.session_id;
        }
        if day.project.is_none() {
            day.project = event.project;
        }
    }

    let mut daily = Vec::new();
    let mut cursor = range.since_key.clone();
    while cursor <= range.until_key {
        let totals = by_day.remove(&cursor).unwrap_or_default();
        let cost_usd = totals.cost_usd();
        daily.push(CostUsageDailyPoint {
            day_key: cursor.clone(),
            model: totals.model,
            session_id: totals.session_id,
            project: totals.project,
            input_tokens: totals.input_tokens,
            cached_input_tokens: totals.cached_input_tokens,
            output_tokens: totals.output_tokens,
            total_tokens: totals.input_tokens + totals.cached_input_tokens + totals.output_tokens,
            cost_usd,
        });
        let Some(next) = next_day_key(&cursor) else {
            break;
        };
        cursor = next;
    }

    let today_key = day_key_from_datetime(now.to_offset(offset));
    let today = daily
        .iter()
        .find(|point| point.day_key == today_key)
        .cloned()
        .unwrap_or(CostUsageDailyPoint {
            day_key: today_key.clone(),
            model: None,
            session_id: None,
            project: None,
            input_tokens: 0,
            cached_input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            cost_usd: None,
        });
    let mut total_tokens = 0;
    let mut total_cost = 0.0;
    let mut cost_seen = false;
    let mut unpriced_cost_seen = false;
    for point in &daily {
        total_tokens += point.total_tokens;
        if let Some(cost) = point.cost_usd {
            total_cost += cost;
            cost_seen = true;
        } else if point.total_tokens > 0 {
            unpriced_cost_seen = true;
        }
    }

    CostUsageSnapshot {
        today_tokens: today.total_tokens,
        today_cost_usd: today.cost_usd,
        last_30_days_tokens: total_tokens,
        last_30_days_cost_usd: (!unpriced_cost_seen && cost_seen).then_some(total_cost),
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
        source_root: roots
            .into_iter()
            .map(|root| root.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(","),
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

impl DailyTotals {
    fn cost_usd(&self) -> Option<f64> {
        if self.has_unpriced_cost {
            None
        } else {
            self.has_priced_cost.then_some(self.cost_usd)
        }
    }
}

fn list_jsonl_files(root: &Path) -> Result<Vec<PathBuf>, AppError> {
    let mut files = Vec::new();
    collect_jsonl_files(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_jsonl_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<(), AppError> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(AppError::ClaudeAccountStore(error.to_string())),
    };
    for entry in entries {
        let entry = entry.map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
        if file_type.is_dir() {
            collect_jsonl_files(&path, files)?;
        } else if file_type.is_file()
            && path.extension().and_then(|value| value.to_str()) == Some("jsonl")
        {
            files.push(path);
        }
    }
    Ok(())
}

fn first_jsonl_file(root: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            if let Some(found) = first_jsonl_file(&path) {
                return Some(found);
            }
        } else if file_type.is_file()
            && path.extension().and_then(|value| value.to_str()) == Some("jsonl")
        {
            return Some(path);
        }
    }
    None
}

fn prune_file_usage(usage: &mut ClaudeCostUsageFile, range: &DayRange) {
    usage.events.retain(|event| {
        event_day_key(event, UtcOffset::UTC)
            .map(|day_key| range.contains_scan_day(&day_key))
            .unwrap_or(false)
    });
}

fn event_day_key(event: &ClaudeCostUsageEvent, offset: UtcOffset) -> Option<String> {
    if let Some(naive) = event.naive_day_key.as_deref() {
        return Some(naive.to_string());
    }
    day_key_from_unix_with_offset(event.timestamp_unix, offset)
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
    let file = File::open(path).map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
    let mut reader = BufReader::with_capacity(256 * 1024, file);
    let start_offset = offset.max(0) as u64;
    if start_offset > 0 {
        reader
            .seek(SeekFrom::Start(start_offset))
            .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
    }

    let mut bytes_read = 0_i64;
    let mut line = Vec::new();
    loop {
        line.clear();
        let count = reader
            .read_until(b'\n', &mut line)
            .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
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

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn load_cache(cache_root: &Path) -> Option<ClaudeCostUsageCache> {
    let path = cache_path(cache_root);
    let contents = fs::read_to_string(path).ok()?;
    let decoded: ClaudeCostUsageCache = serde_json::from_str(&contents).ok()?;
    (decoded.version == CACHE_VERSION).then_some(decoded)
}

fn save_cache(cache_root: &Path, cache: &ClaudeCostUsageCache) -> Result<(), AppError> {
    let path = cache_path(cache_root);
    let parent = path.parent().ok_or_else(|| {
        AppError::ClaudeAccountStore(format!(
            "cost usage cache path has no parent: {}",
            path.to_string_lossy()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
    let contents = serde_json::to_vec(cache)
        .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
    let tmp = temporary_file_path(parent, CACHE_FILE_NAME);
    write_new_file(&tmp, &contents)
        .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
    if let Err(error) = apply_secure_file_permissions(&tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }
    replace_file(&tmp, &path).map_err(|error| AppError::ClaudeAccountStore(error.to_string()))?;
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
        .map_err(|error| AppError::ClaudeAccountStore(error.to_string()))
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

fn roots_fingerprint(roots: &[PathBuf]) -> BTreeMap<String, i64> {
    roots
        .iter()
        .map(|root| (root.to_string_lossy().to_string(), 0))
        .collect()
}

fn project_label_for_file(file: &Path) -> Option<String> {
    file.parent()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .map(str::to_string)
}

fn day_key_from_timestamp(value: &str) -> Option<String> {
    if value.len() >= 10 && value.as_bytes().get(4) == Some(&b'-') {
        return Some(value[..10].to_string());
    }
    OffsetDateTime::parse(value, &Rfc3339)
        .ok()
        .map(day_key_from_datetime)
}

fn timestamp_unix(value: &str) -> Option<i64> {
    OffsetDateTime::parse(value, &Rfc3339)
        .ok()
        .map(|timestamp| timestamp.unix_timestamp())
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

fn day_key_from_datetime(value: OffsetDateTime) -> String {
    let date = value.date();
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

fn next_day_key(day_key: &str) -> Option<String> {
    let mut parts = day_key.split('-');
    let year = parts.next()?.parse::<i32>().ok()?;
    let month = parts.next()?.parse::<u8>().ok()?;
    let day = parts.next()?.parse::<u8>().ok()?;
    let date = time::Date::from_calendar_date(year, month.try_into().ok()?, day).ok()?;
    let next = date.next_day()?;
    Some(format!(
        "{:04}-{:02}-{:02}",
        next.year(),
        u8::from(next.month()),
        next.day()
    ))
}

fn dirs_home() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .unwrap_or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn dedupes_streaming_chunks_by_message_and_request() {
        let root = std::env::temp_dir().join(format!("wovo-claude-cost-{}", Uuid::new_v4()));
        let projects = root.join("projects").join("session");
        fs::create_dir_all(&projects).unwrap();
        fs::write(
            projects.join("session.jsonl"),
            r#"{"type":"assistant","timestamp":"2026-05-19T08:00:00Z","requestId":"req_1","message":{"id":"msg_1","model":"claude-sonnet-4-5","usage":{"input_tokens":10,"cache_read_input_tokens":1,"cache_creation_input_tokens":2,"output_tokens":3}}}
{"type":"assistant","timestamp":"2026-05-19T08:00:01Z","requestId":"req_1","message":{"id":"msg_1","model":"claude-sonnet-4-5","usage":{"input_tokens":20,"cache_read_input_tokens":2,"cache_creation_input_tokens":4,"output_tokens":6}}}
"#,
        )
        .unwrap();

        let now = OffsetDateTime::parse("2026-05-19T12:00:00Z", &Rfc3339).unwrap();
        let snapshot = load_cost_usage_snapshot_for_roots_at(
            vec![root.join("projects")],
            root.join("cache"),
            now,
            true,
            DayRange::new(now, DEFAULT_RANGE_DAYS, UtcOffset::UTC),
            UtcOffset::UTC,
        )
        .unwrap();

        let total = snapshot
            .daily
            .iter()
            .find(|point| point.day_key == "2026-05-19")
            .unwrap();
        assert_eq!(total.input_tokens, 20);
        assert_eq!(total.cached_input_tokens, 6);
        assert_eq!(total.output_tokens, 6);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn mixed_known_and_unknown_models_keep_day_unpriced() {
        let range = DayRange {
            since_key: "2026-05-19".to_string(),
            until_key: "2026-05-19".to_string(),
            scan_since_key: "2026-05-18".to_string(),
            scan_until_key: "2026-05-20".to_string(),
            retention_days: 30,
        };
        let now = OffsetDateTime::parse("2026-05-19T12:00:00Z", &Rfc3339).unwrap();
        let priced_cost = claude_cost_usd("claude-sonnet-4-5", 100, 0, 0, 10).unwrap();
        let cache = ClaudeCostUsageCache {
            version: CACHE_VERSION,
            last_scan_unix_ms: 0,
            retention_days: 30,
            files: BTreeMap::from([(
                "session.jsonl".to_string(),
                ClaudeCostUsageFile {
                    mtime_unix_ms: 0,
                    size: 0,
                    parsed_bytes: 0,
                    events: vec![
                        ClaudeCostUsageEvent {
                            timestamp_unix: unix_from_day_key("2026-05-19").unwrap(),
                            model: "claude-sonnet-4-5".to_string(),
                            session_id: None,
                            project: None,
                            dedupe_key: None,
                            input_tokens: 100,
                            cache_read_tokens: 0,
                            cache_create_tokens: 0,
                            output_tokens: 10,
                            cost_usd: Some(priced_cost),
                            naive_day_key: None,
                        },
                        ClaudeCostUsageEvent {
                            timestamp_unix: unix_from_day_key("2026-05-19").unwrap(),
                            model: "claude-future-6".to_string(),
                            session_id: None,
                            project: None,
                            dedupe_key: None,
                            input_tokens: 50,
                            cache_read_tokens: 0,
                            cache_create_tokens: 0,
                            output_tokens: 5,
                            cost_usd: None,
                            naive_day_key: None,
                        },
                    ],
                },
            )]),
            roots: None,
        };
        let snapshot = build_snapshot_from_cache(
            &cache,
            Vec::new(),
            &range,
            now,
            UtcOffset::UTC,
            ScanStatsAccumulator::default(),
        );

        assert_eq!(snapshot.today_tokens, 165);
        assert_eq!(snapshot.daily[0].cost_usd, None);
        assert_eq!(snapshot.today_cost_usd, None);
        assert_eq!(snapshot.last_30_days_cost_usd, None);
    }

    #[test]
    fn naive_prefix_event_stays_on_its_day_under_negative_offset() {
        // A Claude log row whose timestamp lacks an offset/Z falls back to
        // the day-prefix; encoded as UTC midnight, a negative-offset rebucket
        // would shift it backward a day. The naive_day_key anchor must keep
        // it on the prefix day for today/range totals.
        let now = OffsetDateTime::parse("2026-05-19T18:00:00Z", &Rfc3339).unwrap();
        let offset = UtcOffset::from_hms(-5, 0, 0).unwrap();
        let range = DayRange::new(now, DEFAULT_RANGE_DAYS, offset);
        let cache = ClaudeCostUsageCache {
            version: CACHE_VERSION,
            last_scan_unix_ms: 0,
            retention_days: 30,
            files: BTreeMap::from([(
                "session.jsonl".to_string(),
                ClaudeCostUsageFile {
                    mtime_unix_ms: 0,
                    size: 0,
                    parsed_bytes: 0,
                    events: vec![ClaudeCostUsageEvent {
                        timestamp_unix: unix_from_day_key("2026-05-19").unwrap(),
                        model: "claude-sonnet-4-5".to_string(),
                        session_id: None,
                        project: None,
                        dedupe_key: None,
                        input_tokens: 100,
                        cache_read_tokens: 0,
                        cache_create_tokens: 0,
                        output_tokens: 10,
                        cost_usd: Some(0.05),
                        naive_day_key: Some("2026-05-19".to_string()),
                    }],
                },
            )]),
            roots: None,
        };

        let snapshot = build_snapshot_from_cache(
            &cache,
            Vec::new(),
            &range,
            now,
            offset,
            ScanStatsAccumulator::default(),
        );

        let bucket = snapshot
            .daily
            .iter()
            .find(|point| point.day_key == "2026-05-19")
            .expect("prefix-day event must bucket on 2026-05-19");
        assert_eq!(bucket.total_tokens, 110);
        assert!(
            snapshot
                .daily
                .iter()
                .all(|point| !(point.day_key == "2026-05-18" && point.total_tokens > 0)),
            "prefix-day event must not leak onto the previous day in a negative offset",
        );
    }

    #[test]
    fn incremental_cache_reuses_parsed_offset_after_append() {
        let root = std::env::temp_dir().join(format!("wovo-claude-cost-{}", Uuid::new_v4()));
        let projects = root.join("projects").join("session");
        let cache = root.join("cache");
        fs::create_dir_all(&projects).unwrap();
        let path = projects.join("session.jsonl");
        fs::write(
            &path,
            r#"{"type":"assistant","timestamp":"2026-05-19T08:00:00Z","requestId":"req_1","message":{"id":"msg_1","model":"claude-sonnet-4-5","usage":{"input_tokens":10,"cache_read_input_tokens":1,"cache_creation_input_tokens":2,"output_tokens":3}}}
"#,
        )
        .unwrap();
        let now = OffsetDateTime::parse("2026-05-19T12:00:00Z", &Rfc3339).unwrap();
        let range = DayRange::new(now, DEFAULT_RANGE_DAYS, UtcOffset::UTC);

        let first = load_cost_usage_snapshot_for_roots_at(
            vec![root.join("projects")],
            cache.clone(),
            now,
            false,
            range.clone(),
            UtcOffset::UTC,
        )
        .unwrap();
        assert_eq!(first.last_30_days_tokens, 16);

        fs::write(
            &path,
            r#"{"type":"assistant","timestamp":"2026-05-19T08:00:00Z","requestId":"req_1","message":{"id":"msg_1","model":"claude-sonnet-4-5","usage":{"input_tokens":10,"cache_read_input_tokens":1,"cache_creation_input_tokens":2,"output_tokens":3}}}
{"type":"assistant","timestamp":"2026-05-19T09:00:00Z","requestId":"req_2","message":{"id":"msg_2","model":"claude-sonnet-4-5","usage":{"input_tokens":20,"cache_read_input_tokens":2,"cache_creation_input_tokens":3,"output_tokens":4}}}
"#,
        )
        .unwrap();

        let second = load_cost_usage_snapshot_for_roots_at(
            vec![root.join("projects")],
            cache,
            now,
            false,
            range,
            UtcOffset::UTC,
        )
        .unwrap();
        assert_eq!(second.last_30_days_tokens, 45);
        assert_eq!(second.scan_stats.unwrap().files_scanned, 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn stale_file_removal_drops_cached_events() {
        let root = std::env::temp_dir().join(format!("wovo-claude-cost-{}", Uuid::new_v4()));
        let projects = root.join("projects").join("session");
        let cache = root.join("cache");
        fs::create_dir_all(&projects).unwrap();
        let path = projects.join("session.jsonl");
        fs::write(
            &path,
            r#"{"type":"assistant","timestamp":"2026-05-19T08:00:00Z","requestId":"req_1","message":{"id":"msg_1","model":"claude-sonnet-4-5","usage":{"input_tokens":10,"cache_read_input_tokens":1,"cache_creation_input_tokens":2,"output_tokens":3}}}
"#,
        )
        .unwrap();
        let now = OffsetDateTime::parse("2026-05-19T12:00:00Z", &Rfc3339).unwrap();
        let range = DayRange::new(now, DEFAULT_RANGE_DAYS, UtcOffset::UTC);

        let first = load_cost_usage_snapshot_for_roots_at(
            vec![root.join("projects")],
            cache.clone(),
            now,
            false,
            range.clone(),
            UtcOffset::UTC,
        )
        .unwrap();
        assert_eq!(first.last_30_days_tokens, 16);

        fs::remove_file(path).unwrap();
        let second = load_cost_usage_snapshot_for_roots_at(
            vec![root.join("projects")],
            cache,
            now,
            false,
            range,
            UtcOffset::UTC,
        )
        .unwrap();
        assert_eq!(second.last_30_days_tokens, 0);
        assert_eq!(second.scan_stats.unwrap().files_removed, 1);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn local_claude_logs_exist_ignores_symlinked_project_directories() {
        let root = std::env::temp_dir().join(format!("wovo-claude-cost-{}", Uuid::new_v4()));
        let projects = root.join("projects");
        let linked_target =
            std::env::temp_dir().join(format!("wovo-claude-cost-target-{}", Uuid::new_v4()));
        fs::create_dir_all(&projects).unwrap();
        fs::create_dir_all(&linked_target).unwrap();
        fs::write(linked_target.join("session.jsonl"), "{}\n").unwrap();
        std::os::unix::fs::symlink(&linked_target, projects.join("linked")).unwrap();

        assert!(!local_claude_logs_exist_in_project_roots(&[projects]));

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(linked_target);
    }

    #[test]
    fn local_claude_logs_exist_checks_all_project_roots() {
        let empty_root =
            std::env::temp_dir().join(format!("wovo-claude-cost-empty-{}", Uuid::new_v4()));
        let populated_root =
            std::env::temp_dir().join(format!("wovo-claude-cost-populated-{}", Uuid::new_v4()));
        fs::create_dir_all(&empty_root).unwrap();
        fs::create_dir_all(populated_root.join("session")).unwrap();
        fs::write(populated_root.join("session").join("session.jsonl"), "{}\n").unwrap();

        assert!(local_claude_logs_exist_in_project_roots(&[
            empty_root.clone(),
            populated_root.clone(),
        ]));

        let _ = fs::remove_dir_all(empty_root);
        let _ = fs::remove_dir_all(populated_root);
    }

    #[cfg(unix)]
    #[test]
    fn list_jsonl_files_ignores_symlinked_directories() {
        let root = std::env::temp_dir().join(format!("wovo-claude-cost-{}", Uuid::new_v4()));
        let linked_target =
            std::env::temp_dir().join(format!("wovo-claude-cost-target-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&linked_target).unwrap();
        fs::write(linked_target.join("session.jsonl"), "{}\n").unwrap();
        std::os::unix::fs::symlink(&linked_target, root.join("linked")).unwrap();

        assert!(list_jsonl_files(&root).unwrap().is_empty());

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(linked_target);
    }
}
