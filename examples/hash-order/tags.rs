//! A post's tags, each once and lowercased. The test expects them in the
//! order its author saw, and a HashSet keeps none: Rust seeds each
//! HashSet's hasher from the system's randomness, so the order changes
//! from one run of the test to the next.

use std::collections::HashSet;

/// The tags `words` name, each once, lowercased.
pub fn tags(words: &[&str]) -> Vec<String> {
    let unique: HashSet<String> = words.iter().map(|w| w.to_lowercase()).collect();
    unique.into_iter().collect()
}

// The tags of a post that names one twice, in the order the test's
// author saw them.
#[test]
fn tags_are_lowercased_and_deduplicated() {
    assert_eq!(tags(&["Nix", "rust", "nix"]), ["nix", "rust"]);
}
