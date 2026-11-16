//! The POSIX TZ rule in the footer of a TZif file ("CET-1CEST,M3.5.0,M10.5.0/3"): it describes
//! the offsets after the last listed transition. "Slim" zoneinfo files (the bundled database, and
//! the system's on some distributions) list transitions only up to the last rule change and leave
//! all later years to this rule.

use super::time::{civil_from_days, days_from_civil, days_in_month, weekday};

/// The day of a change in a year.
#[derive(Debug, PartialEq)]
enum Day {
    /// `Mm.w.d`: weekday d (0 = Sunday) of week w (5 = the last) of month m.
    Month(u32, u32, u32),
    /// `Jn`: day 1 … 365 of the year, February 29 never counted.
    Julian(i64),
    /// `n`: day 0 … 365 of the year, February 29 counted.
    Zero(i64),
}

#[derive(Debug, PartialEq)]
struct Change {
    day: Day,
    /// Seconds after local midnight; may be negative or more than a day.
    time: i64,
}

impl Change {
    /// The day number of the change in the year `y`.
    fn day_number(&self, y: i32) -> i64 {
        match self.day {
            Day::Month(m, w, d) => {
                let first = days_from_civil(y, m, 1);
                let sunday_first = (weekday(first) as i64 + 1) % 7;
                let mut day = first + (d as i64 - sunday_first).rem_euclid(7) + 7 * (w as i64 - 1);
                if day >= first + days_in_month(y, m) as i64 {
                    day -= 7;
                }
                day
            }
            Day::Julian(n) => days_from_civil(y, 1, 1) + n - 1 + i64::from(n >= 60 && days_in_month(y, 2) == 29),
            Day::Zero(n) => days_from_civil(y, 1, 1) + n,
        }
    }
}

/// Standard time and, in many zones, summer time with the two changes of every year.
#[derive(Debug, PartialEq)]
pub struct Rule {
    /// Seconds east of UTC.
    std: i64,
    /// Offset in summer time, when it starts, when it ends (the rule's own order: in the southern
    /// hemisphere the start is the later one in the calendar year).
    dst: Option<(i64, Change, Change)>,
}

impl Rule {
    /// Reads the TZ string; None for anything that is not a complete rule (the default US rules
    /// of a string without any are not supported either).
    pub fn parse(s: &str) -> Option<Rule> {
        let mut p = Parser { s: s.as_bytes(), i: 0 };
        p.name()?;
        let std = -p.hms()?;
        if p.done() {
            return Some(Rule { std, dst: None });
        }
        p.name()?;
        let dst = if p.peek() == Some(b',') { std + 3600 } else { -p.hms()? };
        p.eat(b',')?;
        let start = p.change()?;
        p.eat(b',')?;
        let end = p.change()?;
        p.done().then_some(Rule { std, dst: Some((dst, start, end)) })
    }

    /// The two changes of the year `y` as (UTC instant, offset after), in time order.
    pub fn changes_in_year(&self, y: i32) -> Vec<(i64, i64)> {
        let Some((dst, start, end)) = &self.dst else { return vec![] };
        // the start is told in standard time, the end in summer time
        let mut out = vec![(start.day_number(y) * 86_400 + start.time - self.std, *dst), (end.day_number(y) * 86_400 + end.time - dst, self.std)];
        out.sort();
        out
    }

    /// Seconds east of UTC at a UTC instant.
    pub fn offset(&self, utc: i64) -> i64 {
        if self.dst.is_none() {
            return self.std;
        }
        let year = civil_from_days((utc + self.std).div_euclid(86_400)).0;
        // a change can fall into the neighbouring year by its time of day, so look at three years
        (year - 1..=year + 1).flat_map(|y| self.changes_in_year(y)).filter(|c| c.0 <= utc).max_by_key(|c| c.0).map_or(self.std, |c| c.1)
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn done(&self) -> bool {
        self.i == self.s.len()
    }

    fn eat(&mut self, c: u8) -> Option<()> {
        (self.peek() == Some(c)).then(|| self.i += 1)
    }

    /// A zone abbreviation: three or more letters, or anything in `<…>`.
    fn name(&mut self) -> Option<()> {
        let start = self.i;
        if self.eat(b'<').is_some() {
            while self.peek()? != b'>' {
                self.i += 1;
            }
            self.i += 1;
            return (self.i - start > 2).then_some(());
        }
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.i += 1;
        }
        (self.i - start >= 3).then_some(())
    }

    fn number(&mut self, max_digits: usize) -> Option<i64> {
        let start = self.i;
        while self.i - start < max_digits && self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i]).ok()?.parse().ok()
    }

    /// `[+-]hh[:mm[:ss]]` in seconds.
    fn hms(&mut self) -> Option<i64> {
        let sign = if self.eat(b'-').is_some() {
            -1
        } else {
            self.eat(b'+');
            1
        };
        let mut secs = self.number(3)? * 3600;
        for unit in [60, 1] {
            if self.eat(b':').is_none() {
                break;
            }
            secs += self.number(2).filter(|n| *n < 60)? * unit;
        }
        Some(sign * secs)
    }

    /// `Mm.w.d`, `Jn` or `n`, then `/time` (default 02:00:00).
    fn change(&mut self) -> Option<Change> {
        let day = if self.eat(b'M').is_some() {
            let m = self.number(2)? as u32;
            self.eat(b'.')?;
            let w = self.number(1)? as u32;
            self.eat(b'.')?;
            let d = self.number(1)? as u32;
            if !(1..=12).contains(&m) || !(1..=5).contains(&w) || d > 6 {
                return None;
            }
            Day::Month(m, w, d)
        } else if self.eat(b'J').is_some() {
            Day::Julian(self.number(3).filter(|n| (1..=365).contains(n))?)
        } else {
            Day::Zero(self.number(3).filter(|n| *n <= 365)?)
        };
        let time = if self.eat(b'/').is_some() { self.hms()? } else { 7200 };
        Some(Change { day, time })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BERLIN: &str = "CET-1CEST,M3.5.0,M10.5.0/3";

    // spec: CAL-023
    #[test]
    fn rules_are_read_in_all_their_forms() {
        let berlin = Rule::parse(BERLIN).unwrap();
        assert_eq!(berlin.std, 3600);
        let (dst, start, end) = berlin.dst.as_ref().unwrap();
        assert_eq!(*dst, 7200, "summer time is one hour more when the string does not say");
        assert_eq!((start, end), (&Change { day: Day::Month(3, 5, 0), time: 7200 }, &Change { day: Day::Month(10, 5, 0), time: 10_800 }));
        // no summer time, a name in <…>, minutes in the offset
        assert_eq!(Rule::parse("JST-9"), Some(Rule { std: 32_400, dst: None }));
        assert_eq!(Rule::parse("<-03>3"), Some(Rule { std: -10_800, dst: None }));
        assert_eq!(Rule::parse("<+0530>-5:30").unwrap().std, 19_800);
        // an own summer time offset and the day forms J and n, times beyond a day
        let iran = Rule::parse("<+0330>-3:30<+0430>,J79/24,J263/24").unwrap();
        assert_eq!(iran.dst, Some((16_200, Change { day: Day::Julian(79), time: 86_400 }, Change { day: Day::Julian(263), time: 86_400 })));
        assert_eq!(Rule::parse("AAA0BBB1,0/-1:30,364").unwrap().dst, Some((-3600, Change { day: Day::Zero(0), time: -5400 }, Change { day: Day::Zero(364), time: 7200 })));
        // not a rule: nothing, a name only, no rules after the summer time name, rubbish after it, bad numbers
        for bad in [
            "",
            "CET",
            "CET-1CEST",
            "CET-1CEST,M3.5.0",
            "CET-1CEST,M3.5.0,M10.5.0,",
            "CET-1CEST,M13.1.0,M10.5.0",
            "CET-1CEST,M3.6.0,M10.5.0",
            "CET-1CEST,M3.5.7,M10.5.0",
            "CET-1CEST,J0,J10",
            "CET-1CEST,366,J10",
            "CET-1:60",
            "<>-1",
            "<CET-1",
            "CE-1",
        ] {
            assert_eq!(Rule::parse(bad), None, "{bad:?}");
        }
    }

    // spec: CAL-023
    #[test]
    fn the_days_of_a_rule_are_found_in_every_form() {
        let ym = |y, m, d| days_from_civil(y, m, d);
        let rule = |s: &str| Rule::parse(s).unwrap().dst.unwrap();
        // the last Sunday of March 2030 and 2045, the second Sunday of March 2030, the first Sunday of November 2030
        assert_eq!(rule(BERLIN).1.day_number(2030), ym(2030, 3, 31));
        assert_eq!(rule(BERLIN).1.day_number(2045), ym(2045, 3, 26));
        assert_eq!(rule("EST5EDT,M3.2.0,M11.1.0").1.day_number(2030), ym(2030, 3, 10));
        assert_eq!(rule("EST5EDT,M3.2.0,M11.1.0").2.day_number(2030), ym(2030, 11, 3));
        // the 5th week is the last when the month has only four of that weekday (February 2026: four Sundays)
        assert_eq!(rule("AAA0BBB,M2.5.0,M3.1.1").1.day_number(2026), ym(2026, 2, 22));
        // J skips February 29 (J60 is March 1 in every year), a plain number counts it
        assert_eq!(rule("AAA0BBB,J60,J1").1.day_number(2028), ym(2028, 3, 1));
        assert_eq!(rule("AAA0BBB,J60,J1").1.day_number(2026), ym(2026, 3, 1));
        assert_eq!(rule("AAA0BBB,59,J1").1.day_number(2028), ym(2028, 2, 29));
        assert_eq!(rule("AAA0BBB,59,J1").1.day_number(2026), ym(2026, 3, 1));
    }

    // spec: CAL-023
    #[test]
    fn the_offsets_follow_the_rule_for_every_hemisphere_and_sign() {
        let at = |y: i32, m: u32, d: u32, h: i64, mi: i64| days_from_civil(y, m, d) * 86_400 + h * 3600 + mi * 60;
        // (rule, instant just before a change, offset before, offset after)
        let berlin = Rule::parse(BERLIN).unwrap();
        assert_eq!((berlin.offset(1_901_149_199), berlin.offset(1_901_149_200)), (3600, 7200), "2030-03-31 01:00 UTC");
        assert_eq!((berlin.offset(1_919_293_199), berlin.offset(1_919_293_200)), (7200, 3600), "2030-10-27 01:00 UTC");
        // New York: -5, -4 from the second Sunday of March 02:00 local = 07:00 UTC
        let ny = Rule::parse("EST5EDT,M3.2.0,M11.1.0").unwrap();
        assert_eq!((ny.offset(at(2030, 3, 10, 7, 0) - 1), ny.offset(at(2030, 3, 10, 7, 0))), (-18_000, -14_400));
        assert_eq!((ny.offset(at(2030, 11, 3, 6, 0) - 1), ny.offset(at(2030, 11, 3, 6, 0))), (-14_400, -18_000));
        // Sydney: summer time in the southern summer, from the first Sunday of October to the first Sunday of April
        let sydney = Rule::parse("AEST-10AEDT,M10.1.0,M4.1.0/3").unwrap();
        assert_eq!(sydney.offset(at(2030, 1, 15, 0, 0)), 39_600);
        assert_eq!(sydney.offset(at(2030, 7, 15, 0, 0)), 36_000);
        assert_eq!(sydney.offset(at(2030, 12, 31, 23, 0)), 39_600, "still summer time at the turn of the year");
        assert_eq!((sydney.offset(1_901_721_599), sydney.offset(1_901_721_600)), (39_600, 36_000), "2030-04-07 16:00 UTC");
        // Dublin: "summer time" (GMT) in winter has the smaller offset, the standard time IST is +1
        let dublin = Rule::parse("IST-1GMT0,M10.5.0,M3.5.0/1").unwrap();
        assert_eq!(dublin.offset(at(2030, 7, 15, 12, 0)), 3600);
        assert_eq!(dublin.offset(at(2030, 1, 15, 12, 0)), 0);
        // Nuuk: a negative time of change (the day before at 23:00 local) and offsets below zero
        let nuuk = Rule::parse("<-02>2<-01>,M3.5.0/-1,M10.5.0/0").unwrap();
        assert_eq!((nuuk.offset(1_901_149_199), nuuk.offset(1_901_149_200)), (-7200, -3600), "2030-03-31 01:00 UTC");
        assert_eq!((nuuk.offset(1_919_293_199), nuuk.offset(1_919_293_200)), (-3600, -7200), "2030-10-27 01:00 UTC");
        // no summer time
        assert_eq!(Rule::parse("JST-9").unwrap().offset(at(2045, 7, 1, 0, 0)), 32_400);
        assert_eq!(Rule::parse("JST-9").unwrap().changes_in_year(2045), vec![]);
        // a change on New Year's Eve at 22:00 starts summer time that lasts into the next year
        let edge = Rule::parse("AAA0BBB-1,J365/22,J180").unwrap();
        assert_eq!(edge.offset(at(2030, 6, 1, 0, 0)), 3600);
        assert_eq!(edge.offset(at(2030, 12, 1, 0, 0)), 0);
        assert_eq!(edge.offset(at(2030, 12, 31, 21, 59)), 0);
        assert_eq!(edge.offset(at(2030, 12, 31, 22, 0)), 3600);
        assert_eq!(edge.offset(at(2031, 1, 1, 0, 0)), 3600);
    }
}
