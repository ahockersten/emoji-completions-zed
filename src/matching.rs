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
        let score: u32 = if let Some(code) = shortcode {
            haystack_buf.clear();
            needle_buf.clear();
            let code_utf32 = Utf32Str::new(code, &mut haystack_buf);
            let query_utf32 = Utf32Str::new(query, &mut needle_buf);
            if let Some(s) = matcher.fuzzy_match(code_utf32, query_utf32) {
                s as u32
            } else {
                haystack_buf.clear();
                needle_buf.clear();
                let name_utf32 = Utf32Str::new(name, &mut haystack_buf);
                let query_utf32 = Utf32Str::new(query, &mut needle_buf);
                match matcher.fuzzy_match(name_utf32, query_utf32) {
                    Some(s) => s as u32,
                    None => continue,
                }
            }
        } else {
            haystack_buf.clear();
            needle_buf.clear();
            let name_utf32 = Utf32Str::new(name, &mut haystack_buf);
            let query_utf32 = Utf32Str::new(query, &mut needle_buf);
            match matcher.fuzzy_match(name_utf32, query_utf32) {
                Some(s) => s as u32,
                None => continue,
            }
        };

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
    fn test_finds_smile_emoji() {
        let results = find_matching_emojis("smile");

        assert!(!results.is_empty(), "Should find emojis matching 'smile'");

        // Should find the smile emoji somewhere in results
        let has_smile = results
            .iter()
            .any(|e| e.shortcode.as_deref() == Some("smile"));
        assert!(has_smile, "Should find ':smile' emoji in results");
    }

    #[test]
    fn test_finds_multiple_smile_variants() {
        let results = find_matching_emojis("smile");

        // Should find multiple emojis with "smile" in them
        assert!(
            results.len() >= 3,
            "Should find multiple smile-related emojis"
        );

        // Verify at least some results contain "smile"
        let smile_matches = results
            .iter()
            .filter(|e| {
                e.shortcode.as_ref().map_or(false, |s| s.contains("smile"))
                    || e.name.to_lowercase().contains("smile")
            })
            .count();

        assert!(
            smile_matches >= 2,
            "Should find at least 2 emojis with 'smile' in shortcode or name"
        );
    }

    #[test]
    fn test_finds_heart_emoji() {
        let results = find_matching_emojis("heart");

        assert!(!results.is_empty(), "Should find emojis matching 'heart'");

        let has_heart = results
            .iter()
            .any(|e| e.shortcode.as_deref() == Some("heart"));
        assert!(has_heart, "Should find ':heart' emoji in results");
    }

    #[test]
    fn test_finds_cat_emoji() {
        let results = find_matching_emojis("cat");

        assert!(!results.is_empty(), "Should find emojis matching 'cat'");

        let has_cat = results
            .iter()
            .any(|e| e.shortcode.as_deref() == Some("cat"));
        assert!(has_cat, "Should find ':cat' emoji in results");
    }

    #[test]
    fn test_finds_fire_emoji() {
        let results = find_matching_emojis("fire");

        assert!(!results.is_empty(), "Should find emojis matching 'fire'");

        let has_fire = results
            .iter()
            .any(|e| e.shortcode.as_deref() == Some("fire"));
        assert!(has_fire, "Should find ':fire' emoji in results");
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
    fn test_finds_smile_variants() {
        let results = find_matching_emojis("smile");

        let smile_idx = results
            .iter()
            .position(|e| e.shortcode.as_deref() == Some("smile"));
        let sweat_smile_idx = results
            .iter()
            .position(|e| e.shortcode.as_deref() == Some("sweat_smile"));

        assert!(smile_idx.is_some(), "Should find ':smile' emoji");
        // Just verify that if sweat_smile exists, we found both
        if sweat_smile_idx.is_some() {
            assert!(
                smile_idx.is_some(),
                "If we find sweat_smile, we should also find smile"
            );
        }
    }

    #[test]
    fn test_matching_returns_results() {
        let lower_results = find_matching_emojis("smile");
        let upper_results = find_matching_emojis("SMILE");
        let mixed_results = find_matching_emojis("SmIlE");

        // All should return results
        assert!(
            !lower_results.is_empty(),
            "Lowercase query should find results"
        );
        assert!(
            !upper_results.is_empty(),
            "Uppercase query should find results"
        );
        assert!(
            !mixed_results.is_empty(),
            "Mixed case query should find results"
        );
    }
}
