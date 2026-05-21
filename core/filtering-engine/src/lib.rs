use std::collections::HashSet;
use std::sync::Arc;

use parking_lot::RwLock;
use regex::RegexSet;
use tracing::{debug, warn};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FilterError {
    #[error("invalid regex pattern: {0}")]
    InvalidRegex(#[from] regex::Error),
    #[error("invalid rule format: {0}")]
    InvalidRule(String),
}

#[derive(Debug, Clone)]
pub enum FilterResult {
    Blocked { rule: String },
    Allowed { rule: String },
    NotMatched,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuleType {
    ExactBlock(String),
    WildcardBlock(String), // pattern without leading *.
    RegexBlock(String),
    ExactAllow(String),
    WildcardAllow(String),
}

/// Simple rule for dynamic add/remove operations.
#[derive(Debug, Clone)]
pub struct Rule {
    pub pattern: String,
    pub rule_type: RuleType2,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuleType2 {
    Block,
    Allow,
}

struct FilterState {
    exact_blocks: HashSet<String>,
    exact_allows: HashSet<String>,
    wildcard_blocks: Vec<String>,
    wildcard_allows: Vec<String>,
    regex_blocks: Option<RegexSet>,
    regex_patterns: Vec<String>,
}

pub struct FilterEngine {
    state: RwLock<Arc<FilterState>>,
}

impl FilterEngine {
    pub fn new() -> Self {
        Self {
            state: RwLock::new(Arc::new(FilterState {
                exact_blocks: HashSet::new(),
                exact_allows: HashSet::new(),
                wildcard_blocks: Vec::new(),
                wildcard_allows: Vec::new(),
                regex_blocks: None,
                regex_patterns: Vec::new(),
            })),
        }
    }

    /// Load rules from an iterator of rule strings.
    /// Supported formats:
    /// - `domain.com` — exact block
    /// - `*.domain.com` — wildcard block
    /// - `/regex/` — regex block
    /// - `@@domain.com` — exact allow
    /// - `@@*.domain.com` — wildcard allow
    pub fn load_rules<I, S>(&self, rules: I) -> Result<usize, FilterError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut exact_blocks = HashSet::new();
        let mut exact_allows = HashSet::new();
        let mut wildcard_blocks = Vec::new();
        let mut wildcard_allows = Vec::new();
        let mut regex_patterns = Vec::new();
        let mut count = 0;

        for rule in rules {
            let rule = rule.as_ref().trim();
            if rule.is_empty() || rule.starts_with('#') || rule.starts_with('!') {
                continue;
            }

            let parsed = Self::parse_rule(rule)?;
            match parsed {
                RuleType::ExactBlock(d) => { exact_blocks.insert(d); }
                RuleType::WildcardBlock(d) => { wildcard_blocks.push(d); }
                RuleType::RegexBlock(p) => { regex_patterns.push(p); }
                RuleType::ExactAllow(d) => { exact_allows.insert(d); }
                RuleType::WildcardAllow(d) => { wildcard_allows.push(d); }
            }
            count += 1;
        }

        let regex_blocks = if regex_patterns.is_empty() {
            None
        } else {
            Some(RegexSet::new(&regex_patterns)?)
        };

        let new_state = Arc::new(FilterState {
            exact_blocks,
            exact_allows,
            wildcard_blocks,
            wildcard_allows,
            regex_blocks,
            regex_patterns,
        });

        *self.state.write() = new_state;
        debug!(count, "loaded filter rules");
        Ok(count)
    }

    /// Add rules incrementally (merges with existing).
    pub fn add_rules<I, S>(&self, rules: I) -> Result<usize, FilterError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let current = self.state.read().clone();

        let mut exact_blocks = current.exact_blocks.clone();
        let mut exact_allows = current.exact_allows.clone();
        let mut wildcard_blocks = current.wildcard_blocks.clone();
        let mut wildcard_allows = current.wildcard_allows.clone();
        let mut regex_patterns = current.regex_patterns.clone();
        let mut count = 0;

        for rule in rules {
            let rule = rule.as_ref().trim();
            if rule.is_empty() || rule.starts_with('#') || rule.starts_with('!') {
                continue;
            }

            let parsed = Self::parse_rule(rule)?;
            match parsed {
                RuleType::ExactBlock(d) => { exact_blocks.insert(d); }
                RuleType::WildcardBlock(d) => { wildcard_blocks.push(d); }
                RuleType::RegexBlock(p) => { regex_patterns.push(p); }
                RuleType::ExactAllow(d) => { exact_allows.insert(d); }
                RuleType::WildcardAllow(d) => { wildcard_allows.push(d); }
            }
            count += 1;
        }

        let regex_blocks = if regex_patterns.is_empty() {
            None
        } else {
            Some(RegexSet::new(&regex_patterns)?)
        };

        let new_state = Arc::new(FilterState {
            exact_blocks,
            exact_allows,
            wildcard_blocks,
            wildcard_allows,
            regex_blocks,
            regex_patterns,
        });

        *self.state.write() = new_state;
        Ok(count)
    }

    /// Check if a domain should be blocked.
    /// Priority: Allow > Block
    pub fn is_blocked(&self, domain: &str) -> FilterResult {
        let domain = domain.to_lowercase();
        let state = self.state.read().clone();

        // Check exact allow
        if state.exact_allows.contains(&domain) {
            return FilterResult::Allowed {
                rule: format!("@@{}", domain),
            };
        }

        // Check wildcard allow
        for pattern in &state.wildcard_allows {
            if domain_matches_wildcard(&domain, pattern) {
                return FilterResult::Allowed {
                    rule: format!("@@*.{}", pattern),
                };
            }
        }

        // Check exact block
        if state.exact_blocks.contains(&domain) {
            return FilterResult::Blocked {
                rule: domain.clone(),
            };
        }

        // Check wildcard block
        for pattern in &state.wildcard_blocks {
            if domain_matches_wildcard(&domain, pattern) {
                return FilterResult::Blocked {
                    rule: format!("*.{}", pattern),
                };
            }
        }

        // Check regex block
        if let Some(ref regex_set) = state.regex_blocks {
            if let Some(idx) = regex_set.matches(&domain).iter().next() {
                return FilterResult::Blocked {
                    rule: state.regex_patterns[idx].clone(),
                };
            }
        }

        FilterResult::NotMatched
    }

    pub fn rule_count(&self) -> usize {
        let state = self.state.read();
        state.exact_blocks.len()
            + state.exact_allows.len()
            + state.wildcard_blocks.len()
            + state.wildcard_allows.len()
            + state.regex_patterns.len()
    }

    pub fn clear(&self) {
        *self.state.write() = Arc::new(FilterState {
            exact_blocks: HashSet::new(),
            exact_allows: HashSet::new(),
            wildcard_blocks: Vec::new(),
            wildcard_allows: Vec::new(),
            regex_blocks: None,
            regex_patterns: Vec::new(),
        });
    }

    /// Add a single rule dynamically.
    pub fn add_rule(&self, rule: Rule) {
        let current = self.state.read().clone();
        let mut exact_blocks = current.exact_blocks.clone();
        let mut exact_allows = current.exact_allows.clone();
        let mut wildcard_blocks = current.wildcard_blocks.clone();
        let mut wildcard_allows = current.wildcard_allows.clone();
        let mut regex_patterns = current.regex_patterns.clone();

        match rule.rule_type {
            RuleType2::Block => { exact_blocks.insert(rule.pattern); }
            RuleType2::Allow => { exact_allows.insert(rule.pattern); }
        }

        let regex_blocks = if regex_patterns.is_empty() {
            None
        } else {
            RegexSet::new(&regex_patterns).ok()
        };

        *self.state.write() = Arc::new(FilterState {
            exact_blocks,
            exact_allows,
            wildcard_blocks,
            wildcard_allows,
            regex_blocks,
            regex_patterns,
        });
    }

    /// Remove a rule by domain pattern.
    pub fn remove_rule(&self, domain: &str) {
        let current = self.state.read().clone();
        let mut exact_blocks = current.exact_blocks.clone();
        let mut exact_allows = current.exact_allows.clone();
        exact_blocks.remove(domain);
        exact_allows.remove(domain);

        *self.state.write() = Arc::new(FilterState {
            exact_blocks,
            exact_allows,
            wildcard_blocks: current.wildcard_blocks.clone(),
            wildcard_allows: current.wildcard_allows.clone(),
            regex_blocks: current.regex_blocks.clone(),
            regex_patterns: current.regex_patterns.clone(),
        });
    }

    fn parse_rule(rule: &str) -> Result<RuleType, FilterError> {
        let (is_allow, rule) = if let Some(r) = rule.strip_prefix("@@") {
            (true, r)
        } else {
            (false, rule)
        };

        // Regex rule
        if rule.starts_with('/') && rule.ends_with('/') && rule.len() > 2 {
            let pattern = &rule[1..rule.len() - 1];
            // Validate
            let _ = regex::Regex::new(pattern)?;
            return Ok(RuleType::RegexBlock(pattern.to_string()));
        }

        // Wildcard
        if let Some(suffix) = rule.strip_prefix("*.") {
            let domain = suffix.to_lowercase();
            return if is_allow {
                Ok(RuleType::WildcardAllow(domain))
            } else {
                Ok(RuleType::WildcardBlock(domain))
            };
        }

        // Exact domain
        let domain = rule.to_lowercase();
        if is_allow {
            Ok(RuleType::ExactAllow(domain))
        } else {
            Ok(RuleType::ExactBlock(domain))
        }
    }
}

impl Default for FilterEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if domain matches a wildcard pattern.
/// Pattern "example.com" matches "sub.example.com", "a.b.example.com", etc.
fn domain_matches_wildcard(domain: &str, pattern: &str) -> bool {
    domain == pattern || domain.ends_with(&format!(".{}", pattern))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_block() {
        let engine = FilterEngine::new();
        engine.load_rules(vec!["ads.example.com"]).unwrap();
        assert!(matches!(engine.is_blocked("ads.example.com"), FilterResult::Blocked { .. }));
        assert!(matches!(engine.is_blocked("example.com"), FilterResult::NotMatched));
    }

    #[test]
    fn test_wildcard_block() {
        let engine = FilterEngine::new();
        engine.load_rules(vec!["*.doubleclick.net"]).unwrap();
        assert!(matches!(engine.is_blocked("ad.doubleclick.net"), FilterResult::Blocked { .. }));
        assert!(matches!(engine.is_blocked("a.b.doubleclick.net"), FilterResult::Blocked { .. }));
        assert!(matches!(engine.is_blocked("doubleclick.net"), FilterResult::Blocked { .. }));
    }

    #[test]
    fn test_allow_overrides_block() {
        let engine = FilterEngine::new();
        engine.load_rules(vec!["*.example.com", "@@safe.example.com"]).unwrap();
        assert!(matches!(engine.is_blocked("ads.example.com"), FilterResult::Blocked { .. }));
        assert!(matches!(engine.is_blocked("safe.example.com"), FilterResult::Allowed { .. }));
    }

    #[test]
    fn test_regex_block() {
        let engine = FilterEngine::new();
        engine.load_rules(vec!["/^ad[0-9]+\\./"]).unwrap();
        assert!(matches!(engine.is_blocked("ad123.example.com"), FilterResult::Blocked { .. }));
        assert!(matches!(engine.is_blocked("content.example.com"), FilterResult::NotMatched));
    }
}
