//! Which notes and drawings are linked (QUERY-031): the one place of the rule behind the search
//! words `orphan` and `no-backlinks` (and the view of unlinked notes built on it). A link counts
//! when it resolves to a note or a drawing of the vault, as the backlinks and outgoing links of the vault read
//! it ([`crate::vault::Vault::backlinks`]); a link to the note itself, a red link, a link to an image or other non-note file and a link to
//! media outside the vault do not. Conflict copies that have an original are not part of it (NET-015).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Who links and who is linked, built once for a state of the vault.
#[derive(Debug, Default)]
pub struct Linkage {
    /// files with a resolved link to another file
    linking: HashSet<PathBuf>,
    /// files another file links to
    linked: HashSet<PathBuf>,
}

impl Linkage {
    /// The links of the vault as (source, target) pairs; a pair that names one file twice is no link.
    pub fn of_links<'a>(links: impl Iterator<Item = (&'a Path, &'a Path)>) -> Linkage {
        let mut out = Linkage::default();
        for (from, to) in links.filter(|(from, to)| from != to) {
            if !out.linking.contains(from) {
                out.linking.insert(from.to_path_buf());
            }
            if !out.linked.contains(to) {
                out.linked.insert(to.to_path_buf());
            }
        }
        out
    }

    /// No other file links to `rel`; it may link out.
    pub fn has_no_backlinks(&self, rel: &Path) -> bool {
        !self.linked.contains(rel)
    }

    /// `rel` has no resolved link in or out.
    pub fn is_orphan(&self, rel: &Path) -> bool {
        self.has_no_backlinks(rel) && !self.linking.contains(rel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> &Path {
        Path::new(s)
    }

    // spec: QUERY-031
    #[test]
    fn query_031_a_link_to_itself_is_no_link_and_a_one_way_link_leaves_the_target_without_backlinks_only() {
        let l = Linkage::of_links([(p("A.md"), p("A.md")), (p("B.md"), p("C.md"))].into_iter());
        assert!(l.is_orphan(p("A.md")) && l.has_no_backlinks(p("A.md")), "its own link does not count");
        assert!(!l.is_orphan(p("B.md")) && l.has_no_backlinks(p("B.md")), "B links out: no orphan, but nothing links to it");
        assert!(!l.is_orphan(p("C.md")) && !l.has_no_backlinks(p("C.md")), "C is linked to");
        assert!(l.is_orphan(p("D.md")), "a file in no link");
    }
}
