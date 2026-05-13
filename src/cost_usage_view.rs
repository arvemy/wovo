use crate::codex_api::CostUsageDailyPoint;

#[derive(Clone, Copy, Debug)]
pub(crate) struct CostUsageBreakdown {
    pub(crate) input_tokens: i64,
    pub(crate) cached_input_tokens: i64,
    pub(crate) output_tokens: i64,
    pub(crate) total_tokens: i64,
}

impl CostUsageBreakdown {
    pub(crate) fn empty() -> Self {
        Self {
            input_tokens: 0,
            cached_input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
        }
    }

    pub(crate) fn from_daily_point(point: &CostUsageDailyPoint) -> Self {
        Self {
            input_tokens: point.input_tokens,
            cached_input_tokens: point.cached_input_tokens,
            output_tokens: point.output_tokens,
            total_tokens: point.total_tokens,
        }
    }

    pub(crate) fn from_daily_points(points: &[CostUsageDailyPoint]) -> Self {
        points.iter().fold(Self::empty(), |mut total, point| {
            total.input_tokens += point.input_tokens;
            total.cached_input_tokens += point.cached_input_tokens;
            total.output_tokens += point.output_tokens;
            total.total_tokens += point.total_tokens;
            total
        })
    }
}
