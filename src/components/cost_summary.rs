use crate::codex_api::{CostUsageDailyPoint, CostUsageSnapshot};
use crate::cost_usage_view::CostUsageBreakdown;
use crate::formatting::{format_cost, format_tokens, utc_day_key};
use crate::ui::tooltip::{Tooltip, TooltipAlign, TooltipContent, TooltipPosition};
use icons::{ChartColumnIncreasing, Info, X};
use leptos::prelude::*;
use std::collections::BTreeMap;

#[component]
pub fn CostSummary(usage: CostUsageSnapshot) -> impl IntoView {
    // Backend buckets `daily` in its configured timezone; use the snapshot's
    // today_key directly so a non-UTC user near midnight doesn't mismatch.
    // Older cached snapshots without today_key fall back to UTC.
    let today_key = usage
        .today_key
        .clone()
        .unwrap_or_else(|| utc_day_key(usage.updated_at));
    let today_detail = usage
        .daily
        .iter()
        .find(|point| point.day_key == today_key)
        .map(CostUsageBreakdown::from_daily_point)
        .unwrap_or_else(CostUsageBreakdown::empty);
    let last_30_days_detail = CostUsageBreakdown::from_daily_points(&usage.daily);
    let daily_for_chart = usage.daily.clone();
    let chart_today_key = today_key.clone();
    let range_label = format!("Last {}d", usage.range_days);

    view! {
        <div class="mb-3 border border-border bg-secondary p-2.5">
            <div class="flex items-end gap-3">
                <div class="grid min-w-0 flex-1 grid-cols-2 gap-3">
                    <CostMetric
                        label="Today".to_string()
                        tokens=usage.today_tokens
                        cost=usage.today_cost_usd
                        detail=today_detail
                    />
                    <CostMetric
                        label=range_label
                        tokens=usage.last_30_days_tokens
                        cost=usage.last_30_days_cost_usd
                        detail=last_30_days_detail
                    />
                </div>
                <UsageChartButton
                    daily=daily_for_chart
                    updated_at=usage.updated_at
                    today_key=chart_today_key
                />
            </div>
        </div>
    }
}

#[component]
fn CostMetric(
    label: String,
    tokens: i64,
    cost: Option<f64>,
    detail: CostUsageBreakdown,
) -> impl IntoView {
    let input_detail = format!(
        "{} input ({} cached)",
        format_tokens(detail.input_tokens),
        format_tokens(detail.cached_input_tokens),
    );
    let output_detail = format!("{} output", format_tokens(detail.output_tokens));
    let total_detail = format!("{} total", format_tokens(detail.total_tokens));
    let tooltip_label = format!("{label} pricing details");
    let display_label = label;

    view! {
        <div class="flex min-w-0 items-center gap-2 whitespace-nowrap">
            <p class="shrink-0 text-[11px] font-medium uppercase tracking-wide text-muted-foreground">{display_label}</p>
            <div class="flex min-w-0 items-baseline gap-2">
                <strong class="shrink-0 text-sm font-semibold leading-none">{format_cost(cost)}</strong>
                <span class="min-w-0 truncate text-xs text-muted-foreground">{format_tokens(tokens)}</span>
            </div>
            <Tooltip class="shrink-0">
                <button
                    class="inline-flex size-5 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:cursor-pointer hover:bg-accent hover:text-foreground focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
                    type="button"
                    aria-label=tooltip_label
                >
                    <Info class="size-3"/>
                </button>
                <TooltipContent
                    class="w-52 whitespace-normal text-left leading-5"
                    position=TooltipPosition::Bottom
                    align=TooltipAlign::Start
                >
                    <div class="grid gap-1">
                        <p class="font-medium">"Pricing details"</p>
                        <p>{input_detail.clone()}</p>
                        <p>{output_detail.clone()}</p>
                        <p>{total_detail.clone()}</p>
                    </div>
                </TooltipContent>
            </Tooltip>
        </div>
    }
}

#[component]
fn UsageChartButton(
    daily: Vec<CostUsageDailyPoint>,
    updated_at: i64,
    today_key: String,
) -> impl IntoView {
    let months = chart_months(daily, updated_at, &today_key);
    let month_count = months.len();
    if month_count == 0 {
        return view! {
            <button
                type="button"
                class="inline-flex h-8 shrink-0 items-center gap-1.5 self-end rounded-md border border-border bg-background px-2 text-xs font-medium text-muted-foreground opacity-60"
                disabled=true
                aria-label="No token chart data"
                title="No token chart data"
            >
                <ChartColumnIncreasing class="size-3.5"/>
                <span>"Chart"</span>
            </button>
        }.into_any();
    }

    let today_key = StoredValue::new(today_key);
    let months = StoredValue::new(months);
    let selected_month_index = RwSignal::new(month_count - 1);
    let modal_open = RwSignal::new(false);
    let selected_month = Memo::new(move |_| {
        months.with_value(|months| {
            let index = selected_month_index
                .get()
                .min(months.len().saturating_sub(1));
            months[index].clone()
        })
    });

    view! {
        <span class="shrink-0 self-end">
            <button
                type="button"
                class="inline-flex h-8 shrink-0 items-center gap-1.5 rounded-md border border-border bg-background px-2 text-xs font-medium text-muted-foreground transition-colors hover:cursor-pointer hover:bg-accent hover:text-foreground focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
                title="Open token usage chart"
                aria-label="Open token usage chart"
                on:click=move |_| modal_open.set(true)
            >
                <ChartColumnIncreasing class="size-3.5"/>
                <span>"Chart"</span>
            </button>
            {move || modal_open.get().then(|| view! {
                <div
                    class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 px-4"
                    on:click=move |_| modal_open.set(false)
                >
                    <div
                        class="relative w-full max-w-[920px] border border-border bg-background p-5 shadow-xl"
                        on:click=move |ev| ev.stop_propagation()
                    >
                        <div class="mb-4 flex items-start justify-between gap-4">
                            <div>
                                <p class="text-sm font-semibold">"Token usage"</p>
                                <p class="mt-1 text-[11px] text-muted-foreground">
                                    {move || selected_month.get().range_label}
                                    " · "
                                    {move || selected_month.get().active_days_label()}
                                </p>
                            </div>
                            <div class="ml-auto flex items-center gap-2">
                                <button
                                    type="button"
                                    class=move || {
                                        chart_nav_button_class(!has_previous_month(selected_month_index.get()))
                                    }
                                    disabled=move || !has_previous_month(selected_month_index.get())
                                    on:click=move |_| {
                                        if has_previous_month(selected_month_index.get_untracked()) {
                                            selected_month_index.update(|index| *index -= 1);
                                        }
                                    }
                                >
                                    "Prev month"
                                </button>
                                <p class="min-w-28 text-center text-xs font-semibold">
                                    {move || selected_month.get().label}
                                </p>
                                <button
                                    type="button"
                                    class=move || {
                                        chart_nav_button_class(!has_next_month(
                                            selected_month_index.get(),
                                            month_count,
                                        ))
                                    }
                                    disabled=move || {
                                        !has_next_month(selected_month_index.get(), month_count)
                                    }
                                    on:click=move |_| {
                                        if has_next_month(
                                            selected_month_index.get_untracked(),
                                            month_count,
                                        ) {
                                            selected_month_index.update(|index| *index += 1);
                                        }
                                    }
                                >
                                    "Next month"
                                </button>
                            </div>
                            <button
                                type="button"
                                class="flex size-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:cursor-pointer hover:bg-accent hover:text-foreground"
                                aria-label="Close chart"
                                on:click=move |_| modal_open.set(false)
                            >
                                <X class="size-4"/>
                            </button>
                        </div>
                        <div class="mb-4 grid grid-cols-3 gap-2">
                            <ChartStat label="Total" value=move || selected_month.get().total_label()/>
                            <ChartStat label="Average" value=move || selected_month.get().average_label()/>
                            <ChartStat label="Peak" value=move || selected_month.get().peak_label.clone()/>
                        </div>
                        <div class="border border-border bg-secondary p-4">
                            <div class="grid grid-cols-[4.5rem_1fr] gap-3">
                                <div class="flex h-72 flex-col justify-between py-1 text-right text-[10px] text-muted-foreground">
                                    <span>{move || format_tokens(selected_month.get().max_tokens)}</span>
                                    <span>{move || format_tokens(selected_month.get().max_tokens / 2)}</span>
                                    <span>"0"</span>
                                </div>
                                <div class="min-w-0">
                                    <div
                                        class="relative h-72 border-b border-border"
                                        role="img"
                                        aria-label=move || format!("{} token usage bar chart", selected_month.get().label)
                                    >
                                        <div class="pointer-events-none absolute inset-0 grid grid-rows-4">
                                            <div class="border-t border-border/70"></div>
                                            <div class="border-t border-border/50"></div>
                                            <div class="border-t border-border/40"></div>
                                            <div class="border-t border-border/30"></div>
                                        </div>
                                        <div class="relative z-10 flex h-full items-end gap-1 px-1">
                                            <For
                                                each=move || selected_month.get().days
                                                key=|day| day.day_key.clone()
                                                children=move |day| {
                                                    let aria_label = day.detail_label();
                                                    let bar_class = today_key.with_value(|today_key| {
                                                        day.bar_class(today_key.as_str())
                                                    });
                                                    let popover_class = day.popover_class();
                                                    let popover_day = day.day_key.clone();
                                                    let popover_detail = day.detail_label();
                                                    let popover_breakdown = day.breakdown_label();
                                                    view! {
                                                        <div class="flex h-full min-w-0 flex-1 flex-col justify-end">
                                                            <button
                                                                type="button"
                                                                class=bar_class
                                                                style=move || bar_height_style(day.total_tokens, selected_month.get().max_tokens)
                                                                aria-label=aria_label.clone()
                                                            >
                                                                <span class=popover_class aria-hidden="true">
                                                                    <span class="block font-semibold text-foreground">
                                                                        {popover_day.clone()}
                                                                    </span>
                                                                    <span class="mt-1 block text-muted-foreground">
                                                                        {popover_detail.clone()}
                                                                    </span>
                                                                    <span class="mt-2 block font-mono text-[11px] text-muted-foreground">
                                                                        {popover_breakdown.clone()}
                                                                    </span>
                                                                </span>
                                                            </button>
                                                        </div>
                                                    }
                                                }
                                            />
                                        </div>
                                    </div>
                                    <div class="mt-2 flex gap-1 px-1 text-center text-[10px] text-muted-foreground">
                                        <For
                                            each=move || selected_month.get().days
                                            key=|day| format!("{}-label", day.day_key)
                                            children=move |day| {
                                                view! {
                                                    <span class="min-w-0 flex-1">
                                                        {day.axis_label()}
                                                    </span>
                                                }
                                            }
                                        />
                                    </div>
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
            })}
        </span>
    }.into_any()
}

#[component]
fn ChartStat<V>(label: &'static str, value: V) -> impl IntoView
where
    V: Fn() -> String + Send + Sync + 'static,
{
    let value = StoredValue::new(value);
    view! {
        <div class="border border-border bg-background px-3 py-2">
            <p class="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">{label}</p>
            <p class="mt-1 truncate text-xs font-semibold text-foreground">
                {move || value.with_value(|value| value())}
            </p>
        </div>
    }
}

#[derive(Clone, Debug, PartialEq)]
struct ChartMonth {
    label: String,
    range_label: String,
    days: Vec<ChartDay>,
    active_days: usize,
    total_tokens: i64,
    max_tokens: i64,
    peak_label: String,
}

impl ChartMonth {
    fn total_label(&self) -> String {
        format_tokens(self.total_tokens)
    }

    fn average_label(&self) -> String {
        if self.active_days == 0 {
            format_tokens(0)
        } else {
            format_tokens(self.total_tokens / self.active_days as i64)
        }
    }

    fn active_days_label(&self) -> String {
        match self.active_days {
            1 => "1 active day".to_string(),
            days => format!("{days} active days"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct ChartDay {
    day_key: String,
    day: u32,
    days_in_month: u32,
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    cost_usd: Option<f64>,
    scope_label: Option<String>,
}

impl ChartDay {
    fn is_active(&self) -> bool {
        self.total_tokens > 0
            || self.input_tokens > 0
            || self.cached_input_tokens > 0
            || self.output_tokens > 0
    }

    fn bar_class(&self, today_key: &str) -> &'static str {
        if self.day_key == today_key {
            "group relative w-full min-w-[3px] rounded-t-sm bg-primary transition-colors hover:bg-primary/80 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        } else if self.is_active() {
            "group relative w-full min-w-[3px] rounded-t-sm bg-muted-foreground/65 transition-colors hover:bg-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        } else {
            "group relative w-full min-w-[3px] rounded-t-sm bg-border/35 transition-colors hover:bg-muted-foreground/35 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        }
    }

    fn popover_class(&self) -> &'static str {
        if self.day <= 2 {
            "pointer-events-none absolute bottom-[calc(100%+0.5rem)] left-0 z-30 w-64 border border-border bg-background px-3 py-2 text-left text-xs leading-5 text-foreground opacity-0 shadow-xl transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100"
        } else if self.day.saturating_add(1) >= self.days_in_month {
            "pointer-events-none absolute bottom-[calc(100%+0.5rem)] right-0 z-30 w-64 border border-border bg-background px-3 py-2 text-left text-xs leading-5 text-foreground opacity-0 shadow-xl transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100"
        } else {
            "pointer-events-none absolute bottom-[calc(100%+0.5rem)] left-1/2 z-30 w-64 -translate-x-1/2 border border-border bg-background px-3 py-2 text-left text-xs leading-5 text-foreground opacity-0 shadow-xl transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100"
        }
    }

    fn axis_label(&self) -> String {
        if self.day == 1 || self.day.is_multiple_of(5) || self.day == self.days_in_month {
            self.day.to_string()
        } else {
            String::new()
        }
    }

    fn detail_label(&self) -> String {
        format!(
            "{} total · {} input · {} cached · {} output",
            format_tokens(self.total_tokens.max(0)),
            format_tokens(self.input_tokens.max(0)),
            format_tokens(self.cached_input_tokens.max(0)),
            format_tokens(self.output_tokens.max(0)),
        )
    }

    fn breakdown_label(&self) -> String {
        let mut label = format!(
            "in {} / cached {} / out {}",
            format_tokens(self.input_tokens.max(0)),
            format_tokens(self.cached_input_tokens.max(0)),
            format_tokens(self.output_tokens.max(0)),
        );
        if let Some(cost) = self.cost_usd {
            label.push_str(&format!(" / {}", format_cost(Some(cost))));
        }
        if let Some(scope) = self.scope_label.as_deref() {
            label.push_str(&format!(" / {scope}"));
        }
        label
    }
}

fn chart_months(
    points: Vec<CostUsageDailyPoint>,
    updated_at: i64,
    today_key: &str,
) -> Vec<ChartMonth> {
    let mut grouped: BTreeMap<(i32, u32), BTreeMap<u32, CostUsageDailyPoint>> = BTreeMap::new();
    for point in points {
        let Some((year, month, day)) = parse_day_key(&point.day_key) else {
            continue;
        };
        let entry = grouped
            .entry((year, month))
            .or_default()
            .entry(day)
            .or_insert_with(|| CostUsageDailyPoint {
                day_key: point.day_key.clone(),
                model: None,
                session_id: None,
                project: None,
                input_tokens: 0,
                cached_input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
                cost_usd: None,
            });
        entry.input_tokens += point.input_tokens;
        entry.cached_input_tokens += point.cached_input_tokens;
        entry.output_tokens += point.output_tokens;
        entry.total_tokens += point.total_tokens;
        if entry.model.is_none() {
            entry.model.clone_from(&point.model);
        }
        if entry.session_id.is_none() {
            entry.session_id.clone_from(&point.session_id);
        }
        if entry.project.is_none() {
            entry.project.clone_from(&point.project);
        }
        entry.cost_usd = merge_cost(entry.cost_usd, point.cost_usd);
    }
    seed_recent_usage_months(&mut grouped, updated_at, today_key);

    grouped
        .into_iter()
        .map(|((year, month), points_by_day)| {
            let days_in_month = days_in_month(year, month);
            let days = (1..=days_in_month)
                .map(|day| {
                    points_by_day
                        .get(&day)
                        .map(|point| ChartDay {
                            day_key: point.day_key.clone(),
                            day,
                            days_in_month,
                            input_tokens: point.input_tokens,
                            cached_input_tokens: point.cached_input_tokens,
                            output_tokens: point.output_tokens,
                            total_tokens: point.total_tokens,
                            cost_usd: point.cost_usd,
                            scope_label: point_scope_label(point),
                        })
                        .unwrap_or_else(|| ChartDay {
                            day_key: format!("{year:04}-{month:02}-{day:02}"),
                            day,
                            days_in_month,
                            input_tokens: 0,
                            cached_input_tokens: 0,
                            output_tokens: 0,
                            total_tokens: 0,
                            cost_usd: None,
                            scope_label: None,
                        })
                })
                .collect::<Vec<_>>();
            let active_days = days.iter().filter(|day| day.is_active()).count();
            let total_tokens = days.iter().map(|day| day.total_tokens.max(0)).sum();
            let max_tokens = days
                .iter()
                .map(|day| day.total_tokens.max(0))
                .max()
                .unwrap_or(1)
                .max(1);
            let peak = if active_days == 0 {
                format_tokens(0)
            } else {
                days.iter()
                    .filter(|day| day.is_active())
                    .max_by_key(|day| day.total_tokens)
                    .map(|day| {
                        format!(
                            "{} on {}",
                            format_tokens(day.total_tokens.max(0)),
                            day.day_key
                        )
                    })
                    .unwrap_or_else(|| format_tokens(0))
            };
            let range_label = if active_days == 0 {
                format!("{year:04}-{month:02}")
            } else {
                let first = days
                    .iter()
                    .find(|day| day.is_active())
                    .map(|day| day.day_key.clone())
                    .unwrap_or_default();
                let last = days
                    .iter()
                    .rev()
                    .find(|day| day.is_active())
                    .map(|day| day.day_key.clone())
                    .unwrap_or_default();
                format!("{first} - {last}")
            };

            ChartMonth {
                label: format!("{} {year}", month_name(month)),
                range_label,
                days,
                active_days,
                total_tokens,
                max_tokens,
                peak_label: peak,
            }
        })
        .collect()
}

fn merge_cost(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left + right),
        (None, Some(right)) => Some(right),
        (Some(left), None) => Some(left),
        (None, None) => None,
    }
}

fn point_scope_label(point: &CostUsageDailyPoint) -> Option<String> {
    [
        point.model.as_deref(),
        point.session_id.as_deref(),
        point.project.as_deref(),
    ]
    .into_iter()
    .flatten()
    .next()
    .map(str::to_string)
}

fn seed_recent_usage_months(
    grouped: &mut BTreeMap<(i32, u32), BTreeMap<u32, CostUsageDailyPoint>>,
    updated_at: i64,
    today_key: &str,
) {
    if updated_at <= 0 {
        return;
    }

    const RECENT_USAGE_DAYS: i64 = 30;
    const SECONDS_PER_DAY: i64 = 86_400;

    // Anchor the trailing bracket on the backend's local today_key so the
    // modal defaults to a month that contains data. In negative-offset
    // timezones near a UTC month boundary, utc_day_key(updated_at) lands a
    // day ahead and would seed an empty trailing month otherwise.
    if let Some((year, month, _)) = parse_day_key(today_key) {
        grouped.entry((year, month)).or_default();
    }

    let start_at = updated_at
        .saturating_sub((RECENT_USAGE_DAYS - 1) * SECONDS_PER_DAY)
        .max(0);
    if let Some((year, month, _)) = parse_day_key(&utc_day_key(start_at)) {
        grouped.entry((year, month)).or_default();
    }
}

fn bar_height_style(tokens: i64, max_tokens: i64) -> String {
    let height = if tokens <= 0 || max_tokens <= 0 {
        1.0
    } else {
        ((tokens as f64 / max_tokens as f64) * 100.0).clamp(4.0, 100.0)
    };
    format!("height: {height:.1}%;")
}

fn has_previous_month(index: usize) -> bool {
    index > 0
}

fn has_next_month(index: usize, month_count: usize) -> bool {
    index.saturating_add(1) < month_count
}

fn chart_nav_button_class(disabled: bool) -> &'static str {
    if disabled {
        "inline-flex h-8 items-center rounded-md border border-border bg-background px-2 text-xs font-medium text-muted-foreground opacity-45"
    } else {
        "inline-flex h-8 items-center rounded-md border border-border bg-background px-2 text-xs font-medium text-muted-foreground transition-colors hover:cursor-pointer hover:bg-accent hover:text-foreground focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
    }
}

fn parse_day_key(day_key: &str) -> Option<(i32, u32, u32)> {
    if day_key.len() < 10 {
        return None;
    }
    let year = day_key.get(0..4)?.parse().ok()?;
    let month = day_key.get(5..7)?.parse().ok()?;
    let day = day_key.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }
    if !(1..=days_in_month(year, month)).contains(&day) {
        return None;
    }
    Some((year, month, day))
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 30,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn month_name(month: u32) -> &'static str {
    match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => "Month",
    }
}
