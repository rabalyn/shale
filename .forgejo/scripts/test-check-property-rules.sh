#!/bin/sh
# Tests .forgejo/scripts/check-property-rules.sh on a throwaway git repository (spec DEV-043).
#   sh .forgejo/scripts/test-check-property-rules.sh
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
CHECK="$HERE/check-property-rules.sh"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
FAILED=0
N=0

cd "$TMP" || exit 1
git init -q -b main .
git config user.email t@example.com
git config user.name test
git config commit.gpgsign false
mkdir -p src docs/spec

cat > docs/spec/vault.md <<'SPEC'
| VAULT-018 | Typed paths never leave the vault. | IS NOW | `src/p.rs` | `p::vault_018_a` |
| VAULT-019 | Other rule. | IS NOW | `src/p.rs` | `p::marked` |
SPEC
cat > src/p.rs <<'RS'
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(8))]

    #[test]
    fn vault_018_a(x in 0u8..10) {
        prop_assert!(x < 10);
    }

    #[test]
    fn unrelated_property(x in 0u8..10) {
        prop_assert!(x < 11);
    }
}

// spec: VAULT-019
#[test]
fn marked() {
    let mut runner = proptest::test_runner::TestRunner::default();
    runner.run(&(0u8..4), |x| { prop_assert!(x < 4); Ok(()) }).unwrap();
}

#[test]
fn vault_018_example() {
    assert_eq!(1 + 1, 2);
}
RS
cat > src/other.rs <<'RS'
#[test]
fn vault_018_no_proptest_here() {
    assert!(true);
}
RS
git add -A
git commit -q -m "base"
BASE=$(git rev-parse HEAD)

# edit src/p.rs (and the spec) with sed, commit on top of the base and check; each case starts from the base
# try <name> <pass|fail> <needle> <commit message> <sed script for src/p.rs> [spec sed script]
try() {
  N=$((N + 1))
  git reset -q --hard "$BASE"
  [ -z "$5" ] || sed -i "$5" src/p.rs
  [ -z "${6:-}" ] || sed -i "$6" docs/spec/vault.md
  git add -A
  git commit -q --allow-empty -m "$4"
  OUT=$(sh "$CHECK" "$BASE" HEAD 2>&1)
  CODE=$?
  if [ "$2" = pass ] && [ "$CODE" = 0 ] && printf '%s' "$OUT" | grep -Fq -- "$3"; then
    echo "ok $N - $1"
  elif [ "$2" = fail ] && [ "$CODE" != 0 ] && printf '%s' "$OUT" | grep -Fq -- "$3"; then
    echo "ok $N - $1"
  else
    echo "not ok $N - $1 (exit $CODE): $OUT"
    FAILED=1
  fi
}

ADD='$a\
\
proptest! {\
    #[test]\
    fn vault_018_new(x in 0u8..10) {\
        prop_assert!(x < 10);\
    }\
}'
try "an added property passes" pass "no property test" "Add a property" "$ADD"
try "nothing changed passes" pass "no property test" "Empty" ""
try "a changed property without a spec change fails" fail "property vault_018_a (src/p.rs) of VAULT-018" "Weaken" 's/prop_assert!(x < 10)/prop_assert!(x < 100)/'
try "a changed property with its spec row changed passes" pass "Property check passed (1" "Weaken and say so" 's/prop_assert!(x < 10)/prop_assert!(x < 100)/' 's/never leave the vault/never leave the vault, ever/'
try "a changed property with a rule-unchanged line passes" pass "Property check passed (1" "Faster generator

rule-unchanged: VAULT-018 only a faster generator" 's/0u8..10) {/0u8..9) {/'
try "a rule-unchanged line for another ID does not help" fail "VAULT-018" "Faster

rule-unchanged: VAULT-019 other rule" 's/0u8..10) {/0u8..9) {/'
try "a rule-unchanged line without a reason does not help" fail "VAULT-018" "Faster

rule-unchanged: VAULT-018" 's/0u8..10) {/0u8..9) {/'
try "a spec change of another row does not help" fail "VAULT-018" "Weaken" 's/prop_assert!(x < 10)/prop_assert!(x < 100)/' 's/Other rule/Other rules/'
try "a removed property fails" fail "property vault_018_a" "Drop it" '/fn vault_018_a/,/^    }$/d'
try "a line inserted into a property fails" fail "VAULT-018" "Extra line" '/prop_assert!(x < 10)/i\        let _ = x;'
try "a changed property without an ID in its name passes" pass "no property test" "Other" 's/prop_assert!(x < 11)/prop_assert!(x < 12)/'
try "a changed example test passes" pass "no property test" "Example" 's/1 + 1, 2/2 + 2, 4/'
git reset -q --hard "$BASE"
sed -i 's/assert!(true)/assert!(1 < 2)/' src/other.rs
git commit -q -a -m "Other file edit"
N=$((N + 1))
if sh "$CHECK" "$BASE" HEAD >/dev/null 2>&1; then echo "ok $N - a changed test in a file without proptest passes"; else echo "not ok $N - file without proptest"; FAILED=1; fi
try "a changed // spec: marked property fails" fail "property marked (src/p.rs) of VAULT-019" "Weaken" 's/x < 4)/x < 40)/'
try "removing the // spec: marker fails" fail "VAULT-019" "Unmark" '/^\/\/ spec: VAULT-019$/d'
try "a marked property with its spec row changed passes" pass "Property check passed (1" "Weaken" 's/x < 4)/x < 40)/' 's/Other rule/Other rule, changed/'

# an unknown base is an error, not a pass
N=$((N + 1))
if sh "$CHECK" 1234567890123456789012345678901234567890 HEAD >/dev/null 2>&1; then
  echo "not ok $N - an unknown base must fail"; FAILED=1
else
  echo "ok $N - an unknown base fails"
fi

[ "$FAILED" = 0 ] && echo "all $N checks passed"
exit "$FAILED"
