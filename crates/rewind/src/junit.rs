//! Test results from the JUnit XML reports a job writes, for `rewind
//! check --property junit=GLOB`.
//!
//! Maven Surefire, Gradle, `pytest --junitxml`, `cargo nextest` and
//! go-junit-report all write the same shape: `testcase` elements, each
//! with a `classname` and a `name`, holding a `failure` or `error` when the
//! test failed and a `skipped` when it did not run.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use rewind_trace::sdk::TestResult;

const TESTCASE: &str = "testcase";
const CLASSNAME: &str = "classname";
const NAME: &str = "name";
const FAILURE: &str = "failure";
const ERROR: &str = "error";
const SKIPPED: &str = "skipped";

/// Every test case in a report, by `classname.name`, with how it ended.
/// A name a report repeats keeps its last result.
pub(crate) fn results(xml: &str) -> Result<BTreeMap<String, TestResult>> {
    let doc = roxmltree::Document::parse(xml).context("parsing a JUnit XML report")?;
    let mut found = BTreeMap::new();
    for case in doc.descendants().filter(|n| n.has_tag_name(TESTCASE)) {
        let name = case.attribute(NAME).unwrap_or_default();
        let full = match case.attribute(CLASSNAME) {
            Some(class) if !class.is_empty() => format!("{class}.{name}"),
            _ => name.to_string(),
        };
        let child = |tag: &str| case.children().any(|c| c.has_tag_name(tag));
        let result = if child(FAILURE) || child(ERROR) {
            TestResult::Fail
        } else if child(SKIPPED) {
            TestResult::Skip
        } else {
            TestResult::Pass
        };
        found.insert(full, result);
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    // Reports in the shapes the common tools write parse to their tests'
    // results.
    use super::*;

    // Maven Surefire's report: a failure, an error, a skip and a pass.
    #[test]
    fn a_surefire_report_parses() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuite name="com.shop.InventoryTest" tests="4" failures="1" errors="1" skipped="1">
  <testcase name="neverOversells" classname="com.shop.InventoryTest" time="0.012">
    <failure message="expected: &lt;true&gt;" type="org.opentest4j.AssertionFailedError">trace</failure>
  </testcase>
  <testcase name="restocks" classname="com.shop.InventoryTest"><error type="java.lang.NullPointerException"/></testcase>
  <testcase name="later" classname="com.shop.InventoryTest"><skipped/></testcase>
  <testcase name="counts" classname="com.shop.InventoryTest" time="0.001"/>
</testsuite>"#;
        let found = results(xml).unwrap();
        assert_eq!(
            found["com.shop.InventoryTest.neverOversells"],
            TestResult::Fail
        );
        assert_eq!(found["com.shop.InventoryTest.restocks"], TestResult::Fail);
        assert_eq!(found["com.shop.InventoryTest.later"], TestResult::Skip);
        assert_eq!(found["com.shop.InventoryTest.counts"], TestResult::Pass);
    }

    // pytest's report nests its suite in testsuites.
    #[test]
    fn a_pytest_report_parses() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?><testsuites><testsuite name="pytest">
<testcase classname="test_cache" name="test_get" time="0.001"/>
<testcase classname="test_cache" name="test_stale" time="0.002"><failure message="assert 1 == 2">E</failure></testcase>
</testsuite></testsuites>"#;
        let found = results(xml).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found["test_cache.test_get"], TestResult::Pass);
        assert_eq!(found["test_cache.test_stale"], TestResult::Fail);
    }

    // A report that is not XML is an error, not an empty list of tests.
    #[test]
    fn a_broken_report_is_an_error() {
        assert!(results("<testsuite><testcase").is_err());
    }
}
