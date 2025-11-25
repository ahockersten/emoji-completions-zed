use nucleo_matcher::{Config, Matcher, Utf32Str};

#[derive(Debug, Clone, PartialEq)]
pub struct ScoredEmoji {
    pub emoji_char: String,
    pub name: String,
    pub shortcode: Option<String>,
    pub score: u32,
}

/// Finds and scores emojis matching the given query
pub fn find_matching_emojis(query: &str) -> Vec<ScoredEmoji> {
    if query.is_empty() {
        return vec![];
    }

    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut results = Vec::new();

    // Reusable buffers for UTF-32 conversion
    let mut haystack_buf = vec![];
    let mut needle_buf = vec![];

    for emoji in emojis::iter() {
        let emoji_char = emoji.as_str();
        let name = emoji.name();
        let shortcode = emoji.shortcode();

        // Try matching against shortcode first (higher priority), then name
        let (mut score, matched_field): (u32, &str) = if let Some(code) = shortcode {
            haystack_buf.clear();
            needle_buf.clear();
            let code_utf32 = Utf32Str::new(code, &mut haystack_buf);
            let query_utf32 = Utf32Str::new(query, &mut needle_buf);
            if let Some(s) = matcher.fuzzy_match(code_utf32, query_utf32) {
                (s as u32, code)
            } else {
                haystack_buf.clear();
                needle_buf.clear();
                let name_utf32 = Utf32Str::new(name, &mut haystack_buf);
                let query_utf32 = Utf32Str::new(query, &mut needle_buf);
                match matcher.fuzzy_match(name_utf32, query_utf32) {
                    Some(s) => (s as u32, name),
                    None => continue,
                }
            }
        } else {
            haystack_buf.clear();
            needle_buf.clear();
            let name_utf32 = Utf32Str::new(name, &mut haystack_buf);
            let query_utf32 = Utf32Str::new(query, &mut needle_buf);
            match matcher.fuzzy_match(name_utf32, query_utf32) {
                Some(s) => (s as u32, name),
                None => continue,
            }
        };

        // Boost score for exact matches and prefix matches
        let matched_field_lower = matched_field.to_lowercase();
        if matched_field_lower == query {
            // Exact match - huge boost
            score += 10000;
        } else if matched_field_lower.starts_with(query) {
            // Prefix match - significant boost
            score += 5000;
        }

        results.push(ScoredEmoji {
            emoji_char: emoji_char.to_string(),
            name: name.to_string(),
            shortcode: shortcode.map(|s| s.to_string()),
            score,
        });
    }

    // Sort by score (higher is better) and take top 100
    results.sort_by(|a, b| b.score.cmp(&a.score));
    results.truncate(100);

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match_ranks_first() {
        let results = find_matching_emojis("smile");

        assert!(!results.is_empty(), "Should find emojis matching 'smile'");

        // The first result should be the exact match ":smile"
        let first = &results[0];
        assert_eq!(
            first.shortcode.as_deref(),
            Some("smile"),
            "First result should be exact match ':smile', but got {:?}",
            first.shortcode
        );
    }

    #[test]
    fn test_prefix_match_ranks_before_substring() {
        let results = find_matching_emojis("smile");

        // Find indices of different types of matches
        let exact_idx = results
            .iter()
            .position(|e| e.shortcode.as_deref() == Some("smile"));
        let prefix_idx = results.iter().position(|e| {
            e.shortcode
                .as_ref()
                .map_or(false, |s| s.starts_with("smile") && s != "smile")
        });
        let substring_idx = results.iter().position(|e| {
            e.shortcode
                .as_ref()
                .map_or(false, |s| s.contains("smile") && !s.starts_with("smile"))
        });

        // Exact match should come first
        assert!(exact_idx.is_some(), "Should have exact match");

        // If we have both prefix and substring matches, prefix should come first
        if let (Some(prefix), Some(substring)) = (prefix_idx, substring_idx) {
            assert!(
                prefix < substring,
                "Prefix matches should rank before substring matches"
            );
        }
    }

    #[test]
    fn test_heart_exact_match() {
        let results = find_matching_emojis("heart");

        assert!(!results.is_empty(), "Should find emojis matching 'heart'");

        let first = &results[0];
        assert_eq!(
            first.shortcode.as_deref(),
            Some("heart"),
            "First result should be exact match ':heart', but got {:?}",
            first.shortcode
        );
    }

    #[test]
    fn test_cat_exact_match() {
        let results = find_matching_emojis("cat");

        assert!(!results.is_empty(), "Should find emojis matching 'cat'");

        let first = &results[0];
        assert_eq!(
            first.shortcode.as_deref(),
            Some("cat"),
            "First result should be exact match ':cat', but got {:?}",
            first.shortcode
        );
    }

    #[test]
    fn test_fire_exact_match() {
        let results = find_matching_emojis("fire");

        assert!(!results.is_empty(), "Should find emojis matching 'fire'");

        let first = &results[0];
        assert_eq!(
            first.shortcode.as_deref(),
            Some("fire"),
            "First result should be exact match ':fire', but got {:?}",
            first.shortcode
        );
    }

    #[test]
    fn test_empty_query_returns_nothing() {
        let results = find_matching_emojis("");
        assert!(results.is_empty(), "Empty query should return no results");
    }

    #[test]
    fn test_results_limited_to_100() {
        let results = find_matching_emojis("e");
        assert!(
            results.len() <= 100,
            "Results should be limited to 100, got {}",
            results.len()
        );
    }

    #[test]
    fn test_scores_are_ordered() {
        let results = find_matching_emojis("smile");

        // Verify that results are sorted by score (descending)
        for i in 1..results.len() {
            assert!(
                results[i - 1].score >= results[i].score,
                "Results should be sorted by score (descending), but item {} has score {} and item {} has score {}",
                i - 1,
                results[i - 1].score,
                i,
                results[i].score
            );
        }
    }

    #[test]
    fn test_smile_ranks_before_sweat_smile() {
        let results = find_matching_emojis("smile");

        let smile_idx = results
            .iter()
            .position(|e| e.shortcode.as_deref() == Some("smile"));
        let sweat_smile_idx = results
            .iter()
            .position(|e| e.shortcode.as_deref() == Some("sweat_smile"));
        let kissing_smile_eyes_idx = results
            .iter()
            .position(|e| e.shortcode.as_deref() == Some("kissing_smiling_eyes"));

        assert!(smile_idx.is_some(), "Should find ':smile' emoji");

        if let Some(smile_pos) = smile_idx {
            if let Some(sweat_pos) = sweat_smile_idx {
                assert!(
                    smile_pos < sweat_pos,
                    "':smile' (pos {}) should rank before ':sweat_smile' (pos {}), scores: {} vs {}",
                    smile_pos,
                    sweat_pos,
                    results[smile_pos].score,
                    results[sweat_pos].score
                );
            }

            if let Some(kissing_pos) = kissing_smile_eyes_idx {
                assert!(
                    smile_pos < kissing_pos,
                    "':smile' (pos {}) should rank before ':kissing_smiling_eyes' (pos {}), scores: {} vs {}",
                    smile_pos,
                    kissing_pos,
                    results[smile_pos].score,
                    results[kissing_pos].score
                );
            }
        }
    }

    #[test]
    fn test_case_insensitive_matching() {
        let lower_results = find_matching_emojis("smile");
        let upper_results = find_matching_emojis("SMILE");
        let mixed_results = find_matching_emojis("SmIlE");

        // All should return the same first result (exact match)
        assert!(!lower_results.is_empty());
        assert!(!upper_results.is_empty());
        assert!(!mixed_results.is_empty());

        assert_eq!(
            lower_results[0].shortcode, upper_results[0].shortcode,
            "Case should not affect matching"
        );
        assert_eq!(
            lower_results[0].shortcode, mixed_results[0].shortcode,
            "Case should not affect matching"
        );
    }
}
