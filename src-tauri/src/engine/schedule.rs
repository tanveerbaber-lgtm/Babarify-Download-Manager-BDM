//! Queue schedules: when a queue's window opens and closes, in local time.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime};

use crate::db::queues::parse_time;
use crate::models::QueueSchedule;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    Start,
    Stop,
}

fn at(day: NaiveDate, minutes: u32) -> NaiveDateTime {
    day.and_hms_opt(minutes / 60, minutes % 60, 0).expect("valid time")
}

/// Windows opening on days `from..=to`: (start, stop). A stop at or before
/// the start time is on the next day (an overnight window).
fn windows(s: &QueueSchedule, from: NaiveDate, to: NaiveDate) -> Vec<(NaiveDateTime, Option<NaiveDateTime>)> {
    let Some(start) = parse_time(&s.start) else { return Vec::new() };
    let stop = s.stop.as_deref().and_then(parse_time);
    from.iter_days()
        .take_while(|d| *d <= to)
        .filter(|d| s.days.contains(&(d.weekday().num_days_from_monday() as u8)))
        .map(|d| {
            let end = stop.map(|m| if m <= start { at(d, m) + Duration::days(1) } else { at(d, m) });
            (at(d, start), end)
        })
        .collect()
}

/// Whether `t` falls inside a window that has a stop time.
pub fn inside_window(s: &QueueSchedule, t: NaiveDateTime) -> bool {
    s.enabled
        && windows(s, t.date() - Duration::days(1), t.date())
            .into_iter()
            .any(|(start, stop)| stop.is_some_and(|stop| start <= t && t < stop))
}

/// The latest start or stop that happened in `(from, to]`, if any.
pub fn last_transition(s: &QueueSchedule, from: NaiveDateTime, to: NaiveDateTime) -> Option<Transition> {
    if !s.enabled || to <= from {
        return None;
    }
    // After a long sleep only the most recent events matter.
    let from = from.max(to - Duration::days(2));
    windows(s, from.date() - Duration::days(1), to.date())
        .into_iter()
        .flat_map(|(start, stop)| [Some((start, Transition::Start)), stop.map(|t| (t, Transition::Stop))])
        .flatten()
        .filter(|(t, _)| from < *t && *t <= to)
        .max_by_key(|(t, _)| *t)
        .map(|(_, tr)| tr)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(day: u32, h: u32, m: u32) -> NaiveDateTime {
        // 2026-10-19 is a Monday.
        NaiveDate::from_ymd_opt(2026, 10, 19 + day).unwrap().and_hms_opt(h, m, 0).unwrap()
    }

    fn sched(start: &str, stop: Option<&str>, days: &[u8]) -> QueueSchedule {
        QueueSchedule { enabled: true, start: start.into(), stop: stop.map(Into::into), days: days.to_vec() }
    }

    #[test]
    fn starts_and_stops_at_the_right_minutes() {
        let s = sched("01:00", Some("06:00"), &[0, 1, 2, 3, 4, 5, 6]);
        assert_eq!(last_transition(&s, dt(0, 0, 59), dt(0, 1, 0)), Some(Transition::Start));
        assert_eq!(last_transition(&s, dt(0, 1, 0), dt(0, 1, 20)), None);
        assert_eq!(last_transition(&s, dt(0, 5, 59), dt(0, 6, 0)), Some(Transition::Stop));
        // Slept through the whole window: the stop is what counts.
        assert_eq!(last_transition(&s, dt(0, 0, 0), dt(0, 7, 0)), Some(Transition::Stop));
        assert!(inside_window(&s, dt(0, 3, 0)));
        assert!(!inside_window(&s, dt(0, 6, 0)));
    }

    #[test]
    fn overnight_window_belongs_to_its_start_day() {
        // Friday 23:00 → Saturday 07:00 only.
        let s = sched("23:00", Some("07:00"), &[4]);
        assert!(inside_window(&s, dt(4, 23, 30)));
        assert!(inside_window(&s, dt(5, 6, 59)));
        assert!(!inside_window(&s, dt(5, 23, 30)), "Saturday isn't a start day");
        assert_eq!(last_transition(&s, dt(5, 6, 50), dt(5, 7, 0)), Some(Transition::Stop));
        assert_eq!(last_transition(&s, dt(3, 22, 0), dt(3, 23, 30)), None, "Thursday isn't a start day");
    }

    #[test]
    fn without_stop_only_starts() {
        let s = sched("02:30", None, &[0]);
        assert_eq!(last_transition(&s, dt(0, 2, 0), dt(0, 3, 0)), Some(Transition::Start));
        assert_eq!(last_transition(&s, dt(1, 2, 0), dt(1, 3, 0)), None);
        assert!(!inside_window(&s, dt(0, 3, 0)));
    }

    #[test]
    fn disabled_schedule_never_fires() {
        let s = QueueSchedule { enabled: false, ..sched("01:00", Some("02:00"), &[0]) };
        assert_eq!(last_transition(&s, dt(0, 0, 0), dt(0, 1, 30)), None);
        assert!(!inside_window(&s, dt(0, 1, 30)));
    }
}
