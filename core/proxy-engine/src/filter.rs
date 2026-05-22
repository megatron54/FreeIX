//! URL-level ad filtering engine.
//!
//! Handles patterns that DNS blocking cannot:
//! - YouTube video ads (same domain as content)
//! - In-page ad scripts loaded from first-party domains
//! - Tracking pixels with specific URL paths

use parking_lot::RwLock;
use regex::RegexSet;
use std::collections::HashSet;
use tracing::debug;

/// URL filter that blocks requests based on URL patterns.
pub struct UrlFilter {
    /// Exact URL path prefixes to block (fast lookup).
    blocked_paths: RwLock<HashSet<String>>,
    /// Regex patterns for complex URL matching.
    regex_patterns: RwLock<Option<RegexSet>>,
    /// Raw regex strings (kept for rebuilding the set).
    regex_strings: RwLock<Vec<String>>,
}

impl UrlFilter {
    pub fn new() -> Self {
        let filter = Self {
            blocked_paths: RwLock::new(HashSet::new()),
            regex_patterns: RwLock::new(None),
            regex_strings: RwLock::new(Vec::new()),
        };
        filter.load_default_rules();
        filter
    }

    /// Check if a request should be blocked.
    /// Returns true if the URL matches any blocking rule.
    pub fn should_block(&self, host: &str, path: &str, url: &str) -> bool {
        // Check exact path prefixes
        let paths = self.blocked_paths.read();
        for prefix in paths.iter() {
            if url.contains(prefix) || path.starts_with(prefix) {
                debug!(url, rule = prefix, "URL blocked by path rule");
                return true;
            }
        }

        // Check regex patterns
        if let Some(ref set) = *self.regex_patterns.read() {
            if set.is_match(url) {
                debug!(url, "URL blocked by regex rule");
                return true;
            }
        }

        false
    }

    /// Add a path-based blocking rule.
    pub fn add_path_rule(&self, path: &str) {
        self.blocked_paths.write().insert(path.to_string());
    }

    /// Add a regex-based blocking rule.
    pub fn add_regex_rule(&self, pattern: &str) {
        let mut strings = self.regex_strings.write();
        strings.push(pattern.to_string());
        // Rebuild regex set
        if let Ok(set) = RegexSet::new(strings.iter()) {
            *self.regex_patterns.write() = Some(set);
        }
    }

    /// Load default YouTube and general ad-blocking URL rules.
    fn load_default_rules(&self) {
        // ===== YOUTUBE AD BLOCKING =====
        // YouTube serves ads through specific API endpoints and URL patterns
        // that can be distinguished from regular video content.

        let youtube_paths = [
            // Ad-related API endpoints
            "/api/stats/ads",
            "/api/stats/atr",
            "/pagead/",
            "/ptracking",
            "/get_video_info?.*&ad_",
            "/youtubei/v1/player/ad_break",
            // YouTube ad tracking
            "/api/stats/playback?.*&ad_",
            "/api/stats/watchtime?.*&ad_",
            // Specific ad script paths
            "/s/player/*/player_ias.vflset/*/base.js",  // Will need content filtering
            // YouTube ad markers in manifest
            "/videoplayback?.*&oad=",
            "/videoplayback?.*&ctier=L",
            // Google ad infrastructure
            "/pagead/conversion/",
            "/pagead/viewthroughconversion/",
            "/pagead/adview",
            "/pagead/lvz",
            // DoubleClick/Google Ads served inline
            "/gpt/pubads_impl",
            "/tag/js/gpt.js",
            "/gampad/ads",
            "/adx/",
        ];

        let youtube_regex = [
            // YouTube ad video segments (distinguishable from content by URL params)
            r"googlevideo\.com/videoplayback\?.*&oad=",
            r"googlevideo\.com/videoplayback\?.*&ctier=L",
            r"googlevideo\.com/videoplayback\?.*&vprv=1.*&initcwndbps=",
            // YouTube ad initiation requests
            r"youtube\.com/api/stats/ads",
            r"youtube\.com/pagead/",
            r"youtube\.com/ptracking",
            r"youtube\.com/get_midroll_",
            r"youtube\.com/api/stats/atr",
            // Google ad platforms
            r"doubleclick\.net/",
            r"googlesyndication\.com/",
            r"googleadservices\.com/",
            r"google-analytics\.com/",
            r"googletagmanager\.com/",
            r"googletagservices\.com/",
            r"google\.com/ads/",
            r"google\.com/pagead/",
            // General ad networks
            r"adservice\.google\.",
            r"ads\.yahoo\.com",
            r"analytics\.yahoo\.com",
            r"advertising\.com",
            r"adnxs\.com",
            r"adsrvr\.org",
            r"amazon-adsystem\.com",
            r"facebook\.com/tr\?",
            r"facebook\.net/signals/",
            r"scorecardresearch\.com",
            r"outbrain\.com/outbrain",
            r"taboola\.com/libtrc",
            // Tracking pixels
            r"\.gif\?.*&tid=",
            r"/pixel\?",
            r"/track\?",
            r"/beacon\?",
            r"/collect\?.*&tid=",
        ];

        // Add path rules
        {
            let mut paths = self.blocked_paths.write();
            for p in &youtube_paths {
                paths.insert(p.to_string());
            }
        }

        // Add regex rules
        {
            let mut strings = self.regex_strings.write();
            for r in &youtube_regex {
                strings.push(r.to_string());
            }
            if let Ok(set) = RegexSet::new(strings.iter()) {
                *self.regex_patterns.write() = Some(set);
            }
        }
    }
}
