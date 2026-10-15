//! When a plan window resets, as the strip says it: the local clock time
//! (`3:40pm`, `Thu 9am`, `Oct 22 9am`) or the time left (`2h05m`, `3d4h`).

use jiff::Timestamp;
use jiff::tz::TimeZone;

use crate::config::AgentHudResets;

const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

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
}
