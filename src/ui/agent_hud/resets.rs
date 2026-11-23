//! When a plan window resets, as the strip says it: the local clock time
//! (`3:40pm`, `Thu 9am`, `Oct 22 9am`) or the time left (`2h05m`, `3d4h`).

use jiff::Timestamp;
use jiff::tz::TimeZone;

use crate::config::AgentHudResets;

const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
/// Suppress noisy pace projections during the first 10 minutes of a window.
const MIN_PACE_ELAPSED_SECS: i64 = 10 * MINUTE;

/// Format basis points (`0..=10_000`) to two decimal places (e.g. `24.95%`).
#[must_use]
pub fn format_pct_2dp(x100: u16) -> String {
    let clamped = x100.min(10_000);
    format!("{}.{:02}%", clamped / 100, clamped % 100)
}

/// Whole percentage points ahead of (`> 0`) or behind (`< 0`) linear window
/// pace: `used% - elapsed%`. `None` before [`MIN_PACE_ELAPSED_SECS`] or when
/// the window bounds don't match.
#[must_use]
pub fn pace_delta_points(
    used_percent_x100: u16,
    resets_at: i64,
    window_secs: i64,
    now: Timestamp,
) -> Option<i16> {
    let left = resets_at.checked_sub(now.as_second())?;
    if left <= 0 || left >= window_secs || window_secs <= 0 {
        return None;
    }
    let elapsed = window_secs - left;
    if elapsed < MIN_PACE_ELAPSED_SECS {
        return None;
    }
    let elapsed_x100 = elapsed.checked_mul(10_000)? / window_secs;
    let diff = i64::from(used_percent_x100.min(10_000)) - elapsed_x100;
    let rounded = if diff >= 0 {
        (diff + 50) / 100
    } else {
        (diff - 50) / 100
    };
    i16::try_from(rounded).ok()
}

/// Estimated time until a quota window runs out (`~1h12m`) at the current
/// window burn rate. `None` when on or under pace, already empty, or too early
/// in the window.
#[must_use]
pub fn pace_runout(
    used_percent_x100: u16,
    resets_at: i64,
    window_secs: i64,
    now: Timestamp,
) -> Option<String> {
    let used = i64::from(used_percent_x100.min(10_000));
    if used <= 0 || used >= 10_000 {
        return None;
    }
    let left = resets_at.checked_sub(now.as_second())?;
    if left <= 0 || left >= window_secs || window_secs <= 0 {
        return None;
    }
    let elapsed = window_secs - left;
    if elapsed < MIN_PACE_ELAPSED_SECS {
        return None;
    }
    let elapsed_x100 = elapsed.checked_mul(10_000)? / window_secs;
    if used <= elapsed_x100 {
        return None;
    }
    let rem_x100 = 10_000 - used;
    let run_out_secs = elapsed.checked_mul(rem_x100)? / used;
    if run_out_secs <= 0 || run_out_secs >= left {
        return None;
    }
    Some(format!("~{}", countdown(run_out_secs)))
}

/// The next instant [`pace_runout`] changes on its own: appearing past the
/// 10-minute warm-up, ticking to a new minute/hour, or disappearing once
/// elapsed time catches up to linear pace. Called only in
/// [`AgentHudResets::Countdown`], so `clock` mode wakes only at window reset.
#[must_use]
pub fn next_pace_change(
    used_percent_x100: u16,
    resets_at: i64,
    window_secs: i64,
    now: Timestamp,
) -> Option<i64> {
    let used = i64::from(used_percent_x100.min(10_000));
    if used <= 0 || used >= 10_000 || window_secs <= 0 {
        return None;
    }
    let now_s = now.as_second();
    let left = resets_at.checked_sub(now_s)?;
    if left <= 0 || left >= window_secs {
        return None;
    }
    let start = resets_at.checked_sub(window_secs)?;
    let elapsed = now_s - start;
    // Exact second where `elapsed * 10_000 / window_secs >= used`, which is
    // also where `run_out_secs >= left` and the estimate disappears.
    let on_pace_elapsed = (window_secs.checked_mul(used)? + 9_999) / 10_000;
    let on_pace_at = start.checked_add(on_pace_elapsed)?;
    if now_s >= on_pace_at {
        return None;
    }
    let rem = 10_000 - used;
    let first_positive_elapsed = (used + rem - 1) / rem;
    if elapsed < MIN_PACE_ELAPSED_SECS {
        let appear_at = start.checked_add(MIN_PACE_ELAPSED_SECS.max(first_positive_elapsed))?;
        return (appear_at < on_pace_at && appear_at < resets_at).then_some(appear_at);
    }
    let run_out = elapsed.checked_mul(rem)? / used;
    if run_out <= 0 {
        let appear_at = start.checked_add(first_positive_elapsed)?;
        return (appear_at < on_pace_at && appear_at < resets_at).then_some(appear_at);
    }
    if run_out >= left {
        return None;
    }
    let step = if run_out >= DAY { HOUR } else { MINUTE };
    let next_run_out = (run_out / step + 1).checked_mul(step)?;
    let next_elapsed = (next_run_out.checked_mul(used)? + rem - 1) / rem;
    let tick_at = start.checked_add(next_elapsed)?;
    Some(tick_at.min(on_pace_at).min(resets_at).max(now_s + 1))
}

/// The label for a window resetting at `resets_at` (Unix seconds), or `None`
/// once that has passed: the report then describes a window that's gone.
pub(super) fn label(
    resets_at: i64,
    now: Timestamp,
    tz: &TimeZone,
    style: AgentHudResets,
) -> Option<String> {
    let left = resets_at.checked_sub(now.as_second()).filter(|s| *s > 0)?;
    match style {
        AgentHudResets::Clock => clock(resets_at, now, tz),
        AgentHudResets::Countdown => Some(countdown(left)),
    }
}

/// The next instant `label` must be redrawn: the reset itself, and for a
/// countdown each time it ticks over. A clock label also gains or drops its
/// weekday at midnight, but stays true either way, so that needs no wake.
/// `None` once the reset has passed.
pub fn next_change(resets_at: i64, now: Timestamp, style: AgentHudResets) -> Option<i64> {
    let now = now.as_second();
    let left = resets_at.checked_sub(now).filter(|s| *s > 0)?;
    let tick = match style {
        // Floored labels change one second after the remainder runs out.
        AgentHudResets::Countdown if left >= DAY => left % HOUR + 1,
        AgentHudResets::Countdown if left >= MINUTE => left % MINUTE + 1,
        // A clock label, or a countdown's last minute, changes only at the reset.
        AgentHudResets::Clock | AgentHudResets::Countdown => left,
    };
    now.checked_add(tick.min(left))
}

/// Today's reset is a time; this week's adds the weekday; later, the date.
fn clock(resets_at: i64, now: Timestamp, tz: &TimeZone) -> Option<String> {
    let at = Timestamp::from_second(resets_at).ok()?.to_zoned(tz.clone());
    let today = now.to_zoned(tz.clone()).date();
    let days = today.until(at.date()).ok()?.get_days();
    let time = time_of_day(at.hour(), at.minute());
    Some(match days {
        0 => time,
        1..=6 => format!("{} {time}", weekday(at.weekday())),
        _ => format!("{} {} {time}", month(at.month()), at.day()),
    })
}

/// `9am`, `3:40pm`, `12am` for midnight.
fn time_of_day(hour: i8, minute: i8) -> String {
    let suffix = if hour < 12 { "am" } else { "pm" };
    let hour = match hour % 12 {
        0 => 12,
        h => h,
    };
    if minute == 0 {
        format!("{hour}{suffix}")
    } else {
        format!("{hour}:{minute:02}{suffix}")
    }
}

const fn weekday(day: jiff::civil::Weekday) -> &'static str {
    use jiff::civil::Weekday::{Friday, Monday, Saturday, Sunday, Thursday, Tuesday, Wednesday};
    match day {
        Monday => "Mon",
        Tuesday => "Tue",
        Wednesday => "Wed",
        Thursday => "Thu",
        Friday => "Fri",
        Saturday => "Sat",
        Sunday => "Sun",
    }
}

fn month(month: i8) -> &'static str {
    const NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    usize::try_from(month - 1)
        .ok()
        .and_then(|i| NAMES.get(i))
        .copied()
        .unwrap_or("")
}

/// The time left, floored: `<1m`, `14m`, `2h05m`, `3d4h`.
fn countdown(left: i64) -> String {
    if left < MINUTE {
        "<1m".to_owned()
    } else if left < HOUR {
        format!("{}m", left / MINUTE)
    } else if left < DAY {
        format!("{}h{:02}m", left / HOUR, left % HOUR / MINUTE)
    } else {
        format!("{}d{}h", left / DAY, left % DAY / HOUR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::tz::Offset;

    /// Toronto in October: four hours behind UTC.
    fn toronto() -> TimeZone {
        TimeZone::fixed(Offset::from_hours(-4).expect("a valid offset"))
    }

    /// `2026-10-15T<hh>:<mm>Z`, a Thursday.
    fn utc(hour: i64, minute: i64) -> i64 {
        1_792_022_400 + hour * HOUR + minute * MINUTE
    }

    fn at(second: i64) -> Timestamp {
        Timestamp::from_second(second).expect("in range")
    }

    fn clock_label(resets_at: i64, now: i64) -> Option<String> {
        label(resets_at, at(now), &toronto(), AgentHudResets::Clock)
    }

    #[test]
    fn the_fixture_is_a_thursday_at_utc_midnight() {
        let zoned = at(utc(0, 0)).to_zoned(TimeZone::UTC);
        assert_eq!(zoned.to_string(), "2026-10-15T00:00:00+00:00[UTC]");
        assert_eq!(zoned.weekday(), jiff::civil::Weekday::Thursday);
    }

    /// The point of the clock form: the reader's own time, not UTC.
    #[test]
    fn a_clock_reset_reads_in_local_time() {
        let now = utc(16, 0); // noon in Toronto
        assert_eq!(clock_label(utc(19, 40), now).as_deref(), Some("3:40pm"));
        assert_eq!(clock_label(utc(21, 0), now).as_deref(), Some("5pm"));
    }

    #[test]
    fn a_clock_reset_names_the_day_once_it_is_not_today() {
        let now = utc(16, 0); // Thursday noon in Toronto
        // 1am Friday in Toronto, five hours out: a 5h window crossing midnight.
        assert_eq!(clock_label(utc(29, 0), now).as_deref(), Some("Fri 1am"));
        assert_eq!(
            clock_label(utc(16, 0) + 5 * DAY, now).as_deref(),
            Some("Tue 12pm")
        );
        // A week out has today's weekday, so it takes the date instead.
        assert_eq!(
            clock_label(utc(13, 0) + 7 * DAY, now).as_deref(),
            Some("Oct 22 9am")
        );
    }

    #[test]
    fn midnight_and_noon_read_as_twelve() {
        assert_eq!(time_of_day(0, 0), "12am");
        assert_eq!(time_of_day(12, 0), "12pm");
        assert_eq!(time_of_day(23, 5), "11:05pm");
    }

    #[test]
    fn a_countdown_floors_to_its_coarsest_unit() {
        let countdown_label = |left| {
            label(
                utc(0, 0) + left,
                at(utc(0, 0)),
                &TimeZone::UTC,
                AgentHudResets::Countdown,
            )
        };
        assert_eq!(countdown_label(59).as_deref(), Some("<1m"));
        assert_eq!(countdown_label(14 * MINUTE + 59).as_deref(), Some("14m"));
        assert_eq!(
            countdown_label(2 * HOUR + 5 * MINUTE).as_deref(),
            Some("2h05m")
        );
        assert_eq!(countdown_label(DAY - 1).as_deref(), Some("23h59m"));
        assert_eq!(
            countdown_label(3 * DAY + 4 * HOUR + 59 * MINUTE).as_deref(),
            Some("3d4h")
        );
    }

    /// A report whose window has already reset describes a window that's gone.
    #[test]
    fn a_reset_that_has_passed_has_no_label_in_either_form() {
        for style in [AgentHudResets::Clock, AgentHudResets::Countdown] {
            let now = at(utc(16, 0));
            assert_eq!(label(utc(16, 0), now, &toronto(), style), None, "{style:?}");
            assert_eq!(label(utc(15, 0), now, &toronto(), style), None, "{style:?}");
            assert_eq!(next_change(utc(16, 0), now, style), None, "{style:?}");
        }
    }

    /// A clock label only changes when the window resets, so that's the one
    /// wake it asks for.
    #[test]
    fn a_clock_label_changes_only_when_its_window_resets() {
        let now = at(utc(16, 0));
        assert_eq!(
            next_change(utc(19, 40), now, AgentHudResets::Clock),
            Some(utc(19, 40))
        );
    }

    /// Each wake a countdown asks for lands where its label really changes, so
    /// it wakes no more often than the label ticks over.
    #[test]
    fn a_countdown_wakes_exactly_when_its_label_changes() {
        let reset = utc(0, 0) + 5 * DAY;
        let style = AgentHudResets::Countdown;
        let label_at = |s: i64| label(reset, at(s), &TimeZone::UTC, style);
        for start in [
            reset - 3 * DAY - 1234,
            reset - DAY,
            reset - 2 * HOUR - 17,
            reset - 61,
            reset - 30,
        ] {
            let next = next_change(reset, at(start), style).expect("before the reset");
            assert!(next > start, "{start}");
            assert_eq!(
                label_at(next - 1),
                label_at(start),
                "unchanged before {next}"
            );
            assert_ne!(label_at(next), label_at(start), "changed at {next}");
        }
    }

    #[test]
    fn pace_helpers_format_two_decimals_and_forecast_only_when_burning_fast() {
        assert_eq!(format_pct_2dp(0), "0.00%");
        assert_eq!(format_pct_2dp(2495), "24.95%");
        assert_eq!(format_pct_2dp(10_000), "100.00%");

        let five_h = 5 * HOUR;
        let now = at(utc(12, 0));
        // Too early (only 5m elapsed): suppressed.
        let early_reset = now.as_second() + five_h - 5 * MINUTE;
        assert_eq!(pace_delta_points(5000, early_reset, five_h, now), None);
        assert_eq!(pace_runout(5000, early_reset, five_h, now), None);

        // 2h elapsed (40% of 5h), 25% used -> under pace (-15%), no run-out.
        let reset = now.as_second() + 3 * HOUR;
        assert_eq!(pace_delta_points(2500, reset, five_h, now), Some(-15));
        assert_eq!(pace_runout(2500, reset, five_h, now), None);

        // 2h elapsed (40% of 5h), 75% used -> +35% over pace; 25% left at 37.5%/h = 40m.
        assert_eq!(pace_delta_points(7500, reset, five_h, now), Some(35));
        assert_eq!(
            pace_runout(7500, reset, five_h, now).as_deref(),
            Some("~40m")
        );

        // Every wake `next_pace_change` asks for lands on the exact second
        // `pace_runout`'s label changes (warm-up, minute tick, hour tick, or
        // crossing back under pace).
        let seven_d = 7 * DAY;
        for (used, win, elapsed_offset) in [
            (7500, five_h, 5 * MINUTE),
            (7500, five_h, 2 * HOUR),
            (7500, five_h, 3 * HOUR + 44 * MINUTE),
            (8000, seven_d, 2 * DAY),
            (9999, five_h, 15 * MINUTE),
        ] {
            let win_start = utc(0, 0);
            let resets_at = win_start + win;
            let start = win_start + elapsed_offset;
            let next =
                next_pace_change(used, resets_at, win, at(start)).expect("expected a pace change");
            assert!(next > start, "{used}/{win}/{elapsed_offset}");
            assert_eq!(
                pace_runout(used, resets_at, win, at(next - 1)),
                pace_runout(used, resets_at, win, at(start)),
                "unchanged before {next}"
            );
            assert_ne!(
                pace_runout(used, resets_at, win, at(next)),
                pace_runout(used, resets_at, win, at(start)),
                "changed at {next}"
            );
        }
    }
}
