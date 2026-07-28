use anyhow::{bail, Result};
use ts_core::{Bar, Timeframe};

pub fn resample(bars: &[Bar], target: Timeframe, include_incomplete: bool) -> Result<Vec<Bar>> {
    if bars.is_empty() {
        return Ok(Vec::new());
    }

    let dst_secs = target.seconds();

    let mut src_secs = i64::MAX;
    let mut sorted = bars.to_vec();
    sorted.sort_by_key(|b| b.time);
    for i in 1..sorted.len() {
        let delta = sorted[i].time - sorted[i - 1].time;
        if delta > 0 && delta < src_secs {
            src_secs = delta;
        }
    }

    if src_secs == i64::MAX {
        bail!("Cannot determine source timeframe: fewer than two distinct timestamps in input");
    }

    if src_secs > dst_secs {
        bail!(
            "Cannot resample to a finer timeframe than the source ({}s → {}s)",
            src_secs,
            dst_secs
        );
    }

    if dst_secs == src_secs {
        if !include_incomplete {
            if let Some(last_ts) = sorted.last().map(|b| b.time) {
                sorted.retain(|b| b.time + dst_secs <= last_ts);
            }
        }
        return Ok(sorted);
    }

    if dst_secs % src_secs != 0 {
        bail!(
            "Target timeframe must be a multiple of source timeframe ({}s → {}s invalid)",
            src_secs,
            dst_secs
        );
    }

    let anchor = sorted
        .iter()
        .find(|b| b.time % dst_secs == 0)
        .map(|b| b.time)
        .unwrap_or_else(|| (sorted.first().unwrap().time / dst_secs) * dst_secs);

    let bars = sorted
        .into_iter()
        .filter(|b| b.time >= anchor)
        .collect::<Vec<_>>();
    if bars.is_empty() {
        return Ok(Vec::new());
    }

    let mut out: Vec<Bar> = Vec::new();
    for bar in &bars {
        let bucket = ((bar.time - anchor) / dst_secs) * dst_secs + anchor;

        if let Some(last) = out.last_mut() {
            if last.time == bucket {
                last.high = last.high.max(bar.high);
                last.low = last.low.min(bar.low);
                last.close = bar.close;
                last.volume += bar.volume;
            } else {
                out.push(Bar::new(
                    bucket, bar.open, bar.high, bar.low, bar.close, bar.volume,
                ));
            }
        } else {
            out.push(Bar::new(
                bucket, bar.open, bar.high, bar.low, bar.close, bar.volume,
            ));
        }
    }

    if !include_incomplete {
        let last_ts = bars.last().map(|b| b.time).unwrap_or(0);
        out.retain(|b| b.time + dst_secs <= last_ts);
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resamples_1m_to_5m_and_drops_incomplete_bucket() {
        // 10 one-minute bars: bucket [0,300) is complete, [300,600) is not.
        let bars: Vec<Bar> = (0..10)
            .map(|i| {
                let c = 100.0 + i as f64;
                Bar::new(i as i64 * 60, c, c + 1.0, c - 1.0, c, 1.0)
            })
            .collect();

        let out = resample(&bars, Timeframe::M5, false).unwrap();
        assert_eq!(out.len(), 1, "incomplete final bucket must be dropped");
        let b = &out[0];
        assert_eq!(b.time, 0);
        assert_eq!(b.open, bars[0].open);
        assert_eq!(b.close, bars[4].close); // last bar in the bucket
        assert_eq!(b.high, bars[4].high); // highest high in window
        assert_eq!(b.low, bars[0].low); // lowest low in window
        assert_eq!(b.volume, 5.0); // summed
    }

    #[test]
    fn errors_when_target_is_finer_than_source() {
        // 5-minute bars, asking to resample down to 1-minute — impossible, must error.
        let bars: Vec<Bar> = (0..3)
            .map(|i| {
                let t = i as i64 * 300;
                Bar::new(t, 100.0, 101.0, 99.0, 100.5, 10.0)
            })
            .collect();
        let err = resample(&bars, Timeframe::M1, false).unwrap_err();
        assert!(
            err.to_string().contains("finer"),
            "unexpected error message: {err}"
        );
    }

    #[test]
    fn equal_timeframe_passthrough_trims_incomplete_when_requested() {
        // Source granularity == target granularity (5m -> 5m).
        // With include_incomplete = false, the last bar should be dropped
        // because there's nothing after it to prove it's "closed".
        let bars: Vec<Bar> = (0..3)
            .map(|i| {
                let t = i as i64 * 300;
                let c = 100.0 + i as f64;
                Bar::new(t, c, c + 1.0, c - 1.0, c, 1.0)
            })
            .collect();
        let out = resample(&bars, Timeframe::M5, false).unwrap();
        assert_eq!(out.len(), 2, "last bar should be trimmed as incomplete");
        assert_eq!(out[0].time, 0);
        assert_eq!(out[1].time, 300);
    }

    #[test]
    fn equal_timeframe_passthrough_keeps_incomplete_when_requested() {
        let bars: Vec<Bar> = (0..3)
            .map(|i| {
                let t = i as i64 * 300;
                let c = 100.0 + i as f64;
                Bar::new(t, c, c + 1.0, c - 1.0, c, 1.0)
            })
            .collect();
        let out = resample(&bars, Timeframe::M5, true).unwrap();
        assert_eq!(out.len(), 3, "include_incomplete=true should keep all bars");
    }

    #[test]
    fn errors_on_single_bar_input() {
        let bars = vec![Bar::new(0, 100.0, 101.0, 99.0, 100.0, 1.0)];
        let err = resample(&bars, Timeframe::M5, false).unwrap_err();
        assert!(
            err.to_string()
                .contains("Cannot determine source timeframe"),
            "unexpected error message: {err}"
        );
    }

    #[test]
    fn errors_when_all_timestamps_are_identical() {
        // Duplicate timestamps mean every delta is filtered out by `delta > 0`,
        // so src_secs never updates from i64::MAX.
        let bars = vec![
            Bar::new(0, 100.0, 101.0, 99.0, 100.0, 1.0),
            Bar::new(0, 100.0, 101.0, 99.0, 100.5, 1.0),
            Bar::new(0, 100.0, 101.0, 99.0, 100.2, 1.0),
        ];
        let err = resample(&bars, Timeframe::M5, false).unwrap_err();
        assert!(
            err.to_string()
                .contains("Cannot determine source timeframe"),
            "unexpected error message: {err}"
        );
    }

    #[test]
    fn errors_when_target_is_not_a_multiple_of_source() {
        // 1-minute source bars resampled to a 7-minute target — 420 % 60 == 0 actually,
        // so use a target that genuinely doesn't divide evenly, e.g. assuming a
        // non-standard Timeframe variant exists. If only standard variants are
        // available, simulate via irregular source spacing instead:
        // bars spaced 90s apart (1.5 min), target = 5m (300s). 300 % 90 != 0.
        let bars: Vec<Bar> = (0..4)
            .map(|i| {
                let t = i as i64 * 90;
                Bar::new(t, 100.0, 101.0, 99.0, 100.0, 1.0)
            })
            .collect();
        let err = resample(&bars, Timeframe::M5, false).unwrap_err();
        assert!(
            err.to_string().contains("multiple"),
            "unexpected error message: {err}"
        );
    }

    #[test]
    fn empty_input_returns_empty_output() {
        let bars: Vec<Bar> = Vec::new();
        let out = resample(&bars, Timeframe::M5, false).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn handles_unsorted_input() {
        // Bars passed in out of order; result should be identical to the
        // sorted case (1m -> 5m, complete bucket only).
        let mut bars: Vec<Bar> = (0..10)
            .map(|i| {
                let c = 100.0 + i as f64;
                Bar::new(i as i64 * 60, c, c + 1.0, c - 1.0, c, 1.0)
            })
            .collect();
        bars.reverse(); // deliberately unsorted

        let out = resample(&bars, Timeframe::M5, false).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].time, 0);
        assert_eq!(out[0].volume, 5.0);
    }
}
