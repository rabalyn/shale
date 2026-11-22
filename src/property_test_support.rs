//! The shared setup of Shale's property tests (DEV-042): one `proptest` configuration and the
//! generators the properties share. A module of the unit tests only.
//!
//! A failure must repeat, so the generator starts from a fixed seed ([`SEED`]) and no
//! regression file is written: the same cases run on every machine and every run, a failing one
//! fails every time, and `proptest` still shrinks it to the smallest failing input and prints it.
//! `PROPTEST_RNG_SEED` does not change it either: to look for more cases, change [`SEED`] for
//! a try and do not commit that.
#![cfg(test)]

use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed};

/// The seed every property starts from.
pub const SEED: u64 = 0x5AA1E;

/// Cases per property: about 100 ms to 1 s in a debug build for the cheap pure logic these tests
/// are for; a property that is slower than that takes fewer cases through [`config_with`].
pub const CASES: u32 = 512;

/// The configuration of a property test: `#![proptest_config(property_test_support::config())]`.
pub fn config() -> ProptestConfig {
    config_with(CASES)
}

/// [`config`] with another number of cases, for a property whose one case is costly.
pub fn config_with(cases: u32) -> ProptestConfig {
    Config { cases, rng_seed: RngSeed::Fixed(SEED), failure_persistence: None, ..Config::default() }
}

/// Text that looks like something a person types as a path: pieces of the awkward kind (`/`,
/// `\`, `..`, `.`, a drive letter, `//server/share`, `~`, empty and blank parts, device names,
/// non-ASCII) mixed with arbitrary text, up to eight of them.
pub fn path_text() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        3 => Just("/".to_string()),
        3 => Just("\\".to_string()),
        3 => Just("..".to_string()),
        2 => Just(".".to_string()),
        1 => Just("...".to_string()),
        1 => Just("C:".to_string()),
        1 => Just("c:\\".to_string()),
        1 => Just("//server/share".to_string()),
        1 => Just("\\\\?\\C:\\".to_string()),
        1 => Just("~".to_string()),
        1 => Just(" ".to_string()),
        1 => Just("".to_string()),
        1 => Just("con".to_string()),
        2 => Just("é".to_string()),
        1 => Just("日本/語".to_string()),
        4 => "[a-zA-Z0-9 ._:-]{0,6}",
        2 => "\\PC{0,4}",
    ];
    prop::collection::vec(piece, 0..8).prop_map(|parts| parts.concat())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::strategy::ValueTree;
    use proptest::test_runner::{TestError, TestRunner};

    fn draw() -> Vec<String> {
        let mut runner = TestRunner::new(config());
        (0..30).map(|_| path_text().new_tree(&mut runner).unwrap().current()).collect()
    }

    // spec: DEV-042
    #[test]
    fn dev_042_a_property_starts_from_the_same_cases_every_time() {
        let first = draw();
        assert_eq!(first, draw(), "the generator starts from the fixed seed");
        assert!(first.iter().any(|t| t.contains("..")) && first.iter().any(|t| t.contains('\\')), "the awkward pieces are among the cases: {first:?}");
    }

    // spec: DEV-042
    #[test]
    fn dev_042_a_failing_property_fails_again_and_shows_the_smallest_input() {
        let fails = || match TestRunner::new(config()).run(&path_text(), |t| if t.contains("..") { Err(TestCaseError::fail("goes up")) } else { Ok(()) }) {
            Err(TestError::Fail(_, input)) => input,
            other => panic!("the property must fail: {other:?}"),
        };
        let input = fails();
        assert_eq!(input, fails(), "the same input every time");
        assert_eq!(input, "..", "shrunk to the smallest input");
    }
}
