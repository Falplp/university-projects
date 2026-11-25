//! Run this file with `cargo test --test 02_case_insensitive_cmp`.

//! TODO: Implement a struct `CaseInsensitive`, which will allow comparing (=, <, >, etc.)
//! two (ASCII) string slices in a case insensitive way, without performing any reallocations
//! and without modifying the original strings.
//! The comparison should be performed in a lexicographic order.

struct CaseInsensitive<'a>(&'a str);
impl PartialEq for CaseInsensitive<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(other.0)
    }
}
impl Eq for CaseInsensitive<'_> {}
impl PartialOrd for CaseInsensitive<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for CaseInsensitive<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .bytes()
            .map(|b| b.to_ascii_lowercase())
            .cmp(other.0.bytes().map(|b| b.to_ascii_lowercase()))
    }
}

/// Below you can find a set of unit tests.
#[cfg(test)]
mod tests {
    use crate::CaseInsensitive;

    #[test]
    fn case_insensitive_same() {
        assert!(CaseInsensitive("") == CaseInsensitive(""));
        assert!(CaseInsensitive("a") == CaseInsensitive("A"));
        assert!(CaseInsensitive("a") == CaseInsensitive("a"));
        assert!(CaseInsensitive("Foo") == CaseInsensitive(&String::from("fOo")));
        assert!(
            CaseInsensitive("12ABBBcLPQusdaweliAS2") == CaseInsensitive("12AbbbclpQUSdawelias2")
        );
    }

    #[test]
    fn case_insensitive_smaller() {
        assert!(CaseInsensitive("") < CaseInsensitive("a"));
        assert!(CaseInsensitive("aaa") < CaseInsensitive("b"));
        assert!(CaseInsensitive("aaa") < CaseInsensitive("aaab"));
        assert!(CaseInsensitive("a") < CaseInsensitive("B"));
        assert!(CaseInsensitive("aZa") < CaseInsensitive("Zac"));
        assert!(CaseInsensitive("aZ") < CaseInsensitive("Zac"));
        assert!(CaseInsensitive("PWEasUDsx") < CaseInsensitive("PWEaszDsx"));
        assert!(CaseInsensitive("PWEasuDsx") < CaseInsensitive("PWEasZDsx"));
    }

    #[test]
    fn case_insensitive_larger() {
        assert!(CaseInsensitive("a") > CaseInsensitive(""));
        assert!(CaseInsensitive("b") > CaseInsensitive("aaa"));
        assert!(CaseInsensitive("aaab") > CaseInsensitive("aaa"));
        assert!(CaseInsensitive("B") > CaseInsensitive("a"));
        assert!(CaseInsensitive("Zac") > CaseInsensitive("aZa"));
        assert!(CaseInsensitive("Zac") > CaseInsensitive("aZ"));
        assert!(CaseInsensitive("PWEaszDsx") > CaseInsensitive("PWEasUDsx"));
        assert!(CaseInsensitive("PWEasZDsx") > CaseInsensitive("PWEasuDsx"));
    }
}
