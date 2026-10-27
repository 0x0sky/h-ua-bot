// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! The live board: what the sources report for one person's zone, kept as two messages instead of
//! one per report.
//!
//! - The **board** lists every report that is still active. It is edited in place and never rings.
//! - The **status** says in one line how close the danger is. It is sent anew, and so rings, only
//!   when the danger to the person grows; otherwise it is edited quietly.
//!
//! Nothing here says a place is safe. When every report has been called off or has lapsed, the
//! status says the sources went quiet and that this is not an all-clear.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// How many reports the board lists before it summarises the rest.
const BOARD_MAX_LINES: usize = 20;

/// A position this old is worth a reminder, in seconds.
pub const POSITION_STALE_SECS: i64 = 3 * 24 * 3600;

/// Unofficial, and silence is not safety, in one line.
const STATUS_NOTE: &str = "Неофіційні джерела. Орієнтуйтеся на повітряну тривогу.";

/// What is flying, as specific as the report allows.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Hazard {
    /// Ballistic missiles.
    Ballistic,
    /// Any other or unspecified missile.
    Missile,
    /// Jet-powered attack drones.
    JetDrone,
    /// Attack drones, such as Shaheds.
    Drone,
    /// Guided aerial bombs.
    Bomb,
}

impl Hazard {
    /// The most specific hazard among a report's kinds (`air.ballistic_missile`, …), else the
    /// one its class (`drone`, `bomb`, `missile`) names.
    pub fn of(class: &str, kinds: &[String]) -> Option<Self> {
        let named = |kind: &str| match kind {
            "air.ballistic_missile" => Some(Self::Ballistic),
            "air.missile" => Some(Self::Missile),
            "air.jet_drone" => Some(Self::JetDrone),
            "air.attack_drone" => Some(Self::Drone),
            "air.guided_bomb" => Some(Self::Bomb),
            _ => None,
        };
        kinds
            .iter()
            .filter_map(|kind| named(kind))
            .min()
            .or(match class {
                "drone" => Some(Self::Drone),
                "bomb" => Some(Self::Bomb),
                "missile" => Some(Self::Missile),
                _ => None,
            })
    }

    /// An emoji and a short name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Ballistic => "☄️ балістика",
            Self::Missile => "🚀 ракети",
            Self::JetDrone => "✈️ реактивні шахеди",
            Self::Drone => "🛵 шахеди",
            Self::Bomb => "💣 КАБи",
        }
    }
}

/// One active report about the person's zone.
#[derive(Clone, Debug, PartialEq)]
pub struct Threat {
    /// What.
    pub hazard: Hazard,
    /// Where the source said, if it named a place.
    pub place: Option<String>,
    /// The report is about something passing near the zone, not at it.
    pub nearby: bool,
    /// When the newest source said so, in Unix seconds.
    pub reported_at: i64,
    /// Who said so, such as `@kpszsu`.
    pub source: Option<String>,
    /// When the report lapses, in Unix seconds.
    pub valid_until: i64,
}

/// What the hub told about one report.
#[derive(Clone, Debug, PartialEq)]
pub enum Update {
    /// A report that concerns the person.
    Alert {
        /// The hub's assessment id.
        id: String,
        /// The report.
        threat: Threat,
    },
    /// The source called a report off.
    Retraction {
        /// The hub's assessment id.
        id: String,
    },
}

/// How close the danger is.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum Level {
    /// No active report.
    #[default]
    Quiet,
    /// Reports of something near the zone.
    Nearby,
    /// Reports of something at the zone.
    Target,
}

/// The level and what makes it: compared to decide whether the status rings.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Alarm {
    /// How close.
    pub level: Level,
    /// What is reported at that level.
    pub hazards: BTreeSet<Hazard>,
}

impl Alarm {
    /// The danger grew since `before`: closer, or something new at the same distance.
    pub fn grew_since(&self, before: &Alarm) -> bool {
        self.level > before.level
            || (self.level == before.level
                && self.level != Level::Quiet
                && !self.hazards.is_subset(&before.hazards))
    }
}

/// Every active report for one person, during one attack.
#[derive(Clone, Debug, Default)]
pub struct Board {
    active: BTreeMap<String, Threat>,
    ended: BTreeSet<String>,
    since: Option<i64>,
    last_change: Option<i64>,
}

impl Board {
    /// An empty board.
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies what the hub said and tells whether the board changed. A report already lapsed
    /// is history, not news, and a retraction of a report this board never showed changes nothing.
    pub fn apply(&mut self, update: Update, now: i64) -> bool {
        match update {
            Update::Alert { id, threat } => {
                if threat.valid_until <= now || self.ended.contains(&id) {
                    return false;
                }
                if self.active.get(&id) == Some(&threat) {
                    return false;
                }
                self.since = Some(
                    self.since
                        .map_or(threat.reported_at, |since| since.min(threat.reported_at)),
                );
                self.active.insert(id, threat);
            }
            Update::Retraction { id } => {
                if self.active.remove(&id).is_none() {
                    return false;
                }
                self.ended.insert(id);
            }
        }
        self.last_change = Some(now);
        true
    }

    /// Drops reports that lapsed by `now` and tells whether any did.
    pub fn expire(&mut self, now: i64) -> bool {
        let lapsed: Vec<String> = self
            .active
            .iter()
            .filter(|(_, threat)| threat.valid_until <= now)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &lapsed {
            self.active.remove(id);
            self.ended.insert(id.clone());
        }
        if !lapsed.is_empty() {
            self.last_change = Some(now);
        }
        !lapsed.is_empty()
    }

    /// No report is active.
    pub fn is_quiet(&self) -> bool {
        self.active.is_empty()
    }

    /// How close the danger is, and what.
    pub fn alarm(&self) -> Alarm {
        let level = if self.active.values().any(|t| !t.nearby) {
            Level::Target
        } else if self.active.is_empty() {
            Level::Quiet
        } else {
            Level::Nearby
        };
        let hazards = self
            .active
            .values()
            .filter(|t| level != Level::Target || !t.nearby)
            .map(|t| t.hazard)
            .collect();
        Alarm { level, hazards }
    }

    /// The board message.
    pub fn render(&self) -> String {
        let since = self.since.map(clock).unwrap_or_default();
        if self.is_quiet() {
            let until = self.last_change.map(clock).unwrap_or_default();
            return format!(
                "📡 Повідомлення джерел · {since}–{until}\n\
                 Активних немає · завершились: {}",
                self.ended.len()
            );
        }
        let mut threats: Vec<&Threat> = self.active.values().collect();
        threats.sort_by(|a, b| {
            a.nearby
                .cmp(&b.nearby)
                .then(b.reported_at.cmp(&a.reported_at))
        });
        let mut text = format!("📡 Повідомлення джерел · з {since}\n");
        for threat in threats.iter().take(BOARD_MAX_LINES) {
            let mark = if threat.nearby { "🟡" } else { "🔴" };
            let _ = write!(text, "\n{mark} {}", threat.hazard.label());
            if let Some(place) = &threat.place {
                let _ = write!(text, " — {place}");
                if threat.nearby {
                    text.push_str(" (поруч)");
                }
            }
            let _ = write!(text, " · {}", clock(threat.reported_at));
            if let Some(source) = &threat.source {
                let _ = write!(text, " {source}");
            }
        }
        if threats.len() > BOARD_MAX_LINES {
            let _ = write!(text, "\n…і ще {}", threats.len() - BOARD_MAX_LINES);
        }
        if !self.ended.is_empty() {
            let _ = write!(text, "\n\n✔️ Завершились: {}", self.ended.len());
        }
        if let Some(at) = self.last_change {
            let _ = write!(text, "\nОновлено {}", clock(at));
        }
        text
    }

    /// The status message. `reminder` is added only when the board is quiet.
    pub fn status(&self, reminder: Option<&str>) -> String {
        let alarm = self.alarm();
        let mut text = match alarm.level {
            Level::Quiet => {
                let mut text = "⚪ Джерела затихли\nАктивних повідомлень для вашої зони немає. Це не відбій: орієнтуйтеся на повітряну тривогу.".to_owned();
                if let Some(reminder) = reminder {
                    let _ = write!(text, "\n\n{reminder}");
                }
                return text;
            }
            Level::Target => "🔴 Загроза у вашій зоні\n".to_owned(),
            Level::Nearby => "🟡 Загроза поруч із вашою зоною\n".to_owned(),
        };
        let at_level = |nearby: bool| {
            let mut counts: BTreeMap<Hazard, usize> = BTreeMap::new();
            for threat in self.active.values().filter(|t| t.nearby == nearby) {
                *counts.entry(threat.hazard).or_default() += 1;
            }
            counts
        };
        for (hazard, reports) in at_level(alarm.level == Level::Nearby) {
            let _ = write!(text, "\n{}", hazard.label());
            if reports > 1 {
                let _ = write!(
                    text,
                    " — {}",
                    count(reports, "повідомлення", "повідомлення", "повідомлень")
                );
            }
        }
        if alarm.level == Level::Target {
            let near: Vec<&str> = at_level(true).keys().map(|h| h.label()).collect();
            if !near.is_empty() {
                let _ = write!(text, "\n🟡 поруч: {}", near.join(", "));
            }
        }
        let _ = write!(text, "\n\n{STATUS_NOTE}");
        text
    }
}

/// The friendly nudge to refresh a position shared at `at`, or `None` while it is fresh.
pub fn position_reminder(at: i64, now: i64) -> Option<String> {
    (now - at >= POSITION_STALE_SECS).then(|| {
        format!(
            "📍 Ви досі там само? Позицію оновлено {} — /location",
            when(at, now)
        )
    })
}

/// `5 повідомлень`, with the noun agreeing with the number.
fn count(n: usize, one: &str, few: &str, many: &str) -> String {
    let word = match (n % 10, n % 100) {
        (1, 11) => many,
        (1, _) => one,
        (2..=4, 12..=14) => many,
        (2..=4, _) => few,
        _ => many,
    };
    format!("{n} {word}")
}

/// Kyiv's offset from UTC at an instant: EET, and EEST from the last Sunday of March to the last
/// Sunday of October, switching at 01:00 UTC.
fn kyiv_offset(utc: i64) -> i64 {
    let (year, _, _) = civil(utc.div_euclid(86_400));
    let last_sunday = |month: u32| {
        let next = if month == 12 {
            days(year + 1, 1, 1)
        } else {
            days(year, month + 1, 1)
        };
        let last = next - 1;
        last - (last + 4).rem_euclid(7)
    };
    let summer = last_sunday(3) * 86_400 + 3600..last_sunday(10) * 86_400 + 3600;
    if summer.contains(&utc) {
        3 * 3600
    } else {
        2 * 3600
    }
}

/// `03:31`, Kyiv time.
pub fn clock(utc: i64) -> String {
    let local = utc + kyiv_offset(utc);
    let minutes = local.rem_euclid(86_400) / 60;
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// `в суботу о 12:34`, `вчора о 12:34`, or `24.10 о 12:34` when more than a week ago.
fn when(at: i64, now: i64) -> String {
    let day = |utc: i64| (utc + kyiv_offset(utc)).div_euclid(86_400);
    let (then, today) = (day(at), day(now));
    let date = match today - then {
        0 => "сьогодні".to_owned(),
        1 => "вчора".to_owned(),
        2..=6 => [
            "в четвер",
            "в пʼятницю",
            "в суботу",
            "в неділю",
            "в понеділок",
            "у вівторок",
            "в середу",
        ][then.rem_euclid(7) as usize]
            .to_owned(),
        _ => {
            let (_, month, day) = civil(then);
            format!("{day:02}.{month:02}")
        }
    };
    format!("{date} о {}", clock(at))
}

/// Days since 1970-01-01 of a date.
fn days(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let month = i64::from(month);
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date of a day since 1970-01-01.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Reads an RFC 3339 timestamp, such as `2026-10-27T03:31:00Z` or `…00.5+03:00`, into Unix
/// seconds.
pub fn parse_instant(text: &str) -> Option<i64> {
    let number = |range: std::ops::Range<usize>| -> Option<i64> {
        let part = text.get(range)?;
        part.bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    let bytes = text.as_bytes();
    if bytes.len() < 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !matches!(bytes[10], b'T' | b't' | b' ')
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let mut rest = &text[19..];
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        rest = &fraction[digits..];
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ if rest.len() == 6 && matches!(&rest[3..4], ":") => {
            let sign = match &rest[..1] {
                "+" => 1,
                "-" => -1,
                _ => return None,
            };
            let hours: i64 = rest[1..3].parse().ok()?;
            let minutes: i64 = rest[4..6].parse().ok()?;
            sign * (hours * 3600 + minutes * 60)
        }
        _ => return None,
    };
    let date = days(year, month as u32, day as u32);
    Some(date * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: i64 = 1_793_073_060; // 2026-10-27T03:51:00Z, 05:51 in Kyiv

    fn threat(hazard: Hazard, place: &str, nearby: bool, at: i64) -> Threat {
        Threat {
            hazard,
            place: Some(place.to_owned()),
            nearby,
            reported_at: at,
            source: Some("@kpszsu".to_owned()),
            valid_until: at + 1800,
        }
    }

    fn alert(id: &str, threat: Threat) -> Update {
        Update::Alert {
            id: id.to_owned(),
            threat,
        }
    }

    #[test]
    fn timestamps_read_the_way_the_hub_writes_them() {
        assert_eq!(parse_instant("2026-10-27T03:51:00Z"), Some(T));
        assert_eq!(parse_instant("2026-10-27T06:51:00.250+03:00"), Some(T));
        assert_eq!(parse_instant("1970-01-01T00:00:00Z"), Some(0));
        for bad in [
            "",
            "2026-10-27",
            "2026-13-27T03:51:00Z",
            "2026-10-27T03:51:00",
            "2026-10-27T03:51:00+0300",
        ] {
            assert_eq!(parse_instant(bad), None, "{bad}");
        }
    }

    #[test]
    fn kyiv_time_follows_daylight_saving() {
        // Winter time began on 2026-10-25 at 01:00 UTC.
        assert_eq!(clock(T), "05:51");
        assert_eq!(
            clock(parse_instant("2026-10-24T22:00:00Z").unwrap()),
            "01:00"
        );
        assert_eq!(
            clock(parse_instant("2026-10-25T00:59:00Z").unwrap()),
            "03:59"
        );
        assert_eq!(
            clock(parse_instant("2026-10-25T01:00:00Z").unwrap()),
            "03:00"
        );
        assert_eq!(
            clock(parse_instant("2026-03-29T01:00:00Z").unwrap()),
            "04:00"
        );
    }

    #[test]
    fn a_reminder_names_the_day_and_comes_only_when_the_position_is_old() {
        let saturday = parse_instant("2026-10-24T09:34:00Z").unwrap();
        let now = parse_instant("2026-10-27T10:00:00Z").unwrap();
        assert_eq!(
            position_reminder(saturday, now).unwrap(),
            "📍 Ви досі там само? Позицію оновлено в суботу о 12:34 — /location"
        );
        assert_eq!(position_reminder(now - 3600, now), None);
        let long_ago = parse_instant("2026-10-10T09:34:00Z").unwrap();
        assert!(
            position_reminder(long_ago, now)
                .unwrap()
                .contains("10.10 о 12:34")
        );
        assert_eq!(when(now - 86_400, now), "вчора о 12:00");
    }

    #[test]
    fn the_most_specific_kind_names_the_hazard() {
        let kinds = |list: &[&str]| list.iter().map(|k| (*k).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            Hazard::of("missile", &kinds(&["air.missile", "air.ballistic_missile"])),
            Some(Hazard::Ballistic)
        );
        assert_eq!(
            Hazard::of("drone", &kinds(&["air.jet_drone"])),
            Some(Hazard::JetDrone)
        );
        assert_eq!(Hazard::of("drone", &[]), Some(Hazard::Drone));
        assert_eq!(
            Hazard::of("bomb", &kinds(&["air.unknown"])),
            Some(Hazard::Bomb)
        );
        assert_eq!(Hazard::of("balloon", &[]), None);
    }

    #[test]
    fn the_status_rings_only_when_the_danger_grows() {
        let mut board = Board::new();
        let quiet = board.alarm();
        board.apply(alert("a", threat(Hazard::Drone, "Бровари", true, T)), T);
        let nearby = board.alarm();
        assert!(nearby.grew_since(&quiet));
        board.apply(alert("b", threat(Hazard::Drone, "Київ", false, T)), T);
        let target = board.alarm();
        assert!(target.grew_since(&nearby));
        board.apply(
            alert("c", threat(Hazard::Drone, "Київ", false, T + 60)),
            T + 60,
        );
        assert!(
            !board.alarm().grew_since(&target),
            "one more of the same does not ring"
        );
        board.apply(
            alert("d", threat(Hazard::Ballistic, "Київ", false, T + 60)),
            T + 60,
        );
        assert!(board.alarm().grew_since(&target), "something new does");
        let before = board.alarm();
        board.apply(Update::Retraction { id: "d".into() }, T + 90);
        assert!(!board.alarm().grew_since(&before));
        assert!(!Alarm::default().grew_since(&before));
    }

    #[test]
    fn stale_reports_and_unknown_retractions_change_nothing() {
        let mut board = Board::new();
        assert!(!board.apply(
            alert("old", threat(Hazard::Missile, "Київ", false, T - 7200)),
            T
        ));
        assert!(!board.apply(Update::Retraction { id: "x".into() }, T));
        assert!(board.is_quiet());
        assert!(board.apply(alert("a", threat(Hazard::Missile, "Київ", false, T)), T));
        assert!(
            !board.apply(alert("a", threat(Hazard::Missile, "Київ", false, T)), T),
            "a retry"
        );
        board.apply(Update::Retraction { id: "a".into() }, T + 10);
        assert!(
            !board.apply(
                alert("a", threat(Hazard::Missile, "Київ", false, T)),
                T + 20
            ),
            "called off stays off"
        );
    }

    #[test]
    fn reports_lapse_on_their_own_time() {
        let mut board = Board::new();
        board.apply(alert("a", threat(Hazard::Drone, "Київ", false, T)), T);
        assert!(!board.expire(T + 1799));
        assert!(board.expire(T + 1800));
        assert!(board.is_quiet());
        assert_eq!(board.alarm(), Alarm::default());
    }

    #[test]
    fn the_board_lists_reports_at_the_zone_first_then_near_it() {
        let mut board = Board::new();
        board.apply(alert("a", threat(Hazard::Drone, "Бровари", true, T)), T);
        board.apply(
            alert("b", threat(Hazard::Ballistic, "Київ", false, T + 60)),
            T + 60,
        );
        board.apply(
            alert("c", threat(Hazard::Drone, "Київ", false, T + 120)),
            T + 120,
        );
        board.apply(
            alert("d", threat(Hazard::Bomb, "Київ", false, T + 130)),
            T + 130,
        );
        board.apply(Update::Retraction { id: "d".into() }, T + 180);
        assert_eq!(
            board.render(),
            "📡 Повідомлення джерел · з 05:51\n\
             \n🔴 🛵 шахеди — Київ · 05:53 @kpszsu\
             \n🔴 ☄️ балістика — Київ · 05:52 @kpszsu\
             \n🟡 🛵 шахеди — Бровари (поруч) · 05:51 @kpszsu\
             \n\n✔️ Завершились: 1\
             \nОновлено 05:54"
        );
    }

    #[test]
    fn the_status_counts_reports_and_never_calls_the_sky_clear() {
        let mut board = Board::new();
        for (id, at) in [("a", T), ("b", T + 60), ("c", T + 120)] {
            board.apply(alert(id, threat(Hazard::JetDrone, "Київ", false, at)), at);
        }
        board.apply(alert("d", threat(Hazard::Ballistic, "Київ", false, T)), T);
        board.apply(alert("e", threat(Hazard::Drone, "Бровари", true, T)), T);
        assert_eq!(
            board.status(Some("ignored while active")),
            "🔴 Загроза у вашій зоні\n\
             \n☄️ балістика\
             \n✈️ реактивні шахеди — 3 повідомлення\
             \n🟡 поруч: 🛵 шахеди\
             \n\nНеофіційні джерела. Орієнтуйтеся на повітряну тривогу."
        );
        board.expire(T + 10_000);
        let quiet = board.status(Some("📍 нагадування"));
        assert!(quiet.starts_with("⚪ Джерела затихли"));
        assert!(quiet.contains("Це не відбій"));
        assert!(quiet.ends_with("📍 нагадування"));
        assert!(!quiet.contains("чист") && !quiet.contains("безпечн"));
        assert_eq!(
            board.render(),
            "📡 Повідомлення джерел · 05:51–08:37\nАктивних немає · завершились: 5"
        );
    }

    #[test]
    fn nouns_agree_with_numbers() {
        let n = |n| count(n, "повідомлення", "повідомлення", "повідомлень");
        assert_eq!(n(1), "1 повідомлення");
        assert_eq!(n(3), "3 повідомлення");
        assert_eq!(n(5), "5 повідомлень");
        assert_eq!(n(11), "11 повідомлень");
        assert_eq!(n(21), "21 повідомлення");
    }
}
