//! Integration test: Blocklist loading and domain filtering.
//!
//! Tests that the filter engine correctly blocks and allows domains
//! based on loaded blocklists.

use freeix_filter::{FilterEngine, FilterResult, Source};
use std::io::Write;
use tempfile::NamedTempFile;

/// Create a temporary blocklist file with the given domains.
fn create_blocklist(domains: &[&str]) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    for domain in domains {
        writeln!(file, "0.0.0.0 {domain}").expect("Failed to write domain");
    }
    file.flush().expect("Failed to flush");
    file
}

/// Create a temporary blocklist in adblock-style format.
fn create_adblock_blocklist(rules: &[&str]) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    writeln!(file, "! Title: Test Blocklist").unwrap();
    writeln!(file, "! Last modified: 2025-01-01").unwrap();
    for rule in rules {
        writeln!(file, "{rule}").unwrap();
    }
    file.flush().expect("Failed to flush");
    file
}

#[tokio::test]
async fn test_load_hosts_blocklist_and_block_domains() {
    let blocklist = create_blocklist(&[
        "ads.example.com",
        "tracker.example.net",
        "malware.badsite.org",
        "analytics.site.io",
    ]);

    let engine = FilterEngine::new()
        .with_source(Source::hosts_file(blocklist.path()))
        .build()
        .await
        .expect("Failed to build filter engine");

    // Blocked domains
    assert_eq!(
        engine.check("ads.example.com"),
        FilterResult::Blocked,
        "ads.example.com should be blocked"
    );
    assert_eq!(
        engine.check("tracker.example.net"),
        FilterResult::Blocked,
        "tracker.example.net should be blocked"
    );
    assert_eq!(
        engine.check("malware.badsite.org"),
        FilterResult::Blocked,
        "malware.badsite.org should be blocked"
    );

    // Allowed domains (not in blocklist)
    assert_eq!(
        engine.check("example.com"),
        FilterResult::Allowed,
        "example.com should be allowed"
    );
    assert_eq!(
        engine.check("safe.example.net"),
        FilterResult::Allowed,
        "safe.example.net should be allowed"
    );
    assert_eq!(
        engine.check("google.com"),
        FilterResult::Allowed,
        "google.com should be allowed"
    );
}

#[tokio::test]
async fn test_subdomain_blocking() {
    let blocklist = create_blocklist(&["ads.example.com"]);

    let engine = FilterEngine::new()
        .with_source(Source::hosts_file(blocklist.path()))
        .with_subdomain_matching(true)
        .build()
        .await
        .expect("Failed to build filter engine");

    // The blocked domain and its subdomains
    assert_eq!(engine.check("ads.example.com"), FilterResult::Blocked);
    assert_eq!(engine.check("sub.ads.example.com"), FilterResult::Blocked);
    assert_eq!(
        engine.check("deep.sub.ads.example.com"),
        FilterResult::Blocked
    );

    // Parent domains should NOT be blocked
    assert_eq!(engine.check("example.com"), FilterResult::Allowed);
    assert_eq!(engine.check("other.example.com"), FilterResult::Allowed);
}

#[tokio::test]
async fn test_allowlist_overrides_blocklist() {
    let blocklist = create_blocklist(&[
        "ads.example.com",
        "tracker.example.com",
        "needed-tracker.example.com",
    ]);

    let engine = FilterEngine::new()
        .with_source(Source::hosts_file(blocklist.path()))
        .with_allowlist(vec!["needed-tracker.example.com".to_string()])
        .build()
        .await
        .expect("Failed to build filter engine");

    assert_eq!(engine.check("ads.example.com"), FilterResult::Blocked);
    assert_eq!(engine.check("tracker.example.com"), FilterResult::Blocked);
    assert_eq!(
        engine.check("needed-tracker.example.com"),
        FilterResult::Allowed,
        "Allowlisted domain should not be blocked"
    );
}

#[tokio::test]
async fn test_adblock_format_parsing() {
    let blocklist = create_adblock_blocklist(&[
        "||doubleclick.net^",
        "||ads.facebook.com^",
        "||tracking.google.com^",
    ]);

    let engine = FilterEngine::new()
        .with_source(Source::adblock_file(blocklist.path()))
        .build()
        .await
        .expect("Failed to build filter engine");

    assert_eq!(engine.check("doubleclick.net"), FilterResult::Blocked);
    assert_eq!(engine.check("ads.facebook.com"), FilterResult::Blocked);
    assert_eq!(engine.check("tracking.google.com"), FilterResult::Blocked);

    // Non-blocked
    assert_eq!(engine.check("facebook.com"), FilterResult::Allowed);
    assert_eq!(engine.check("google.com"), FilterResult::Allowed);
}

#[tokio::test]
async fn test_multiple_blocklists_merged() {
    let blocklist_a = create_blocklist(&["ads.site-a.com", "tracker.site-a.com"]);
    let blocklist_b = create_blocklist(&["ads.site-b.com", "malware.site-b.com"]);

    let engine = FilterEngine::new()
        .with_source(Source::hosts_file(blocklist_a.path()))
        .with_source(Source::hosts_file(blocklist_b.path()))
        .build()
        .await
        .expect("Failed to build filter engine");

    // All domains from both lists should be blocked
    assert_eq!(engine.check("ads.site-a.com"), FilterResult::Blocked);
    assert_eq!(engine.check("tracker.site-a.com"), FilterResult::Blocked);
    assert_eq!(engine.check("ads.site-b.com"), FilterResult::Blocked);
    assert_eq!(engine.check("malware.site-b.com"), FilterResult::Blocked);

    // Unrelated domains allowed
    assert_eq!(engine.check("site-a.com"), FilterResult::Allowed);
    assert_eq!(engine.check("site-b.com"), FilterResult::Allowed);
}

#[tokio::test]
async fn test_empty_blocklist() {
    let blocklist = create_blocklist(&[]);

    let engine = FilterEngine::new()
        .with_source(Source::hosts_file(blocklist.path()))
        .build()
        .await
        .expect("Failed to build filter engine");

    assert_eq!(engine.check("anything.com"), FilterResult::Allowed);
    assert_eq!(engine.check("ads.example.com"), FilterResult::Allowed);
}

#[tokio::test]
async fn test_filter_statistics() {
    let blocklist = create_blocklist(&["ads.example.com", "tracker.example.com"]);

    let engine = FilterEngine::new()
        .with_source(Source::hosts_file(blocklist.path()))
        .build()
        .await
        .expect("Failed to build filter engine");

    let stats = engine.stats();
    assert_eq!(stats.total_rules, 2, "Should have 2 rules loaded");
    assert_eq!(stats.sources_loaded, 1, "Should have 1 source loaded");
}
