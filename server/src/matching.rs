use emojis::SkinTone;
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

#[derive(Debug, Clone, PartialEq)]
pub struct ScoredEmoji {
    pub emoji_char: String,
    pub name: String,
    pub shortcode: Option<String>,
    pub score: u32,
}

/// Finds and scores emojis matching the given query, giving each the skin tone where it has one
pub fn find_matching_emojis(query: &str, skin_tone: SkinTone) -> Vec<ScoredEmoji> {
    // Each word of the query is matched separately, so `thumbs up` matches `thumbsup`.
    let pattern = Pattern::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    if pattern.atoms.is_empty() {
        return vec![];
    }

    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut results = Vec::new();

    // Reusable buffer for UTF-32 conversion
    let mut haystack_buf = vec![];

    let mut score = |haystack: &str| {
        haystack_buf.clear();
        pattern.score(Utf32Str::new(haystack, &mut haystack_buf), &mut matcher)
    };

    for emoji in emojis::iter() {
        let name = emoji.name();

        // Shortcodes take priority over the name; the best matching shortcode is the one shown.
        let best_shortcode = emoji
            .shortcodes()
            .filter_map(|code| score(code).map(|s| (code, s)))
            .max_by_key(|&(_, s)| s);
        let (shortcode, score) = match best_shortcode {
            Some((code, s)) => (Some(code), s),
            None => match score(name) {
                Some(s) => (emoji.shortcode(), s),
                None => continue,
            },
        };

        let toned = emoji.with_skin_tone(skin_tone).unwrap_or(emoji);
        results.push(ScoredEmoji {
            emoji_char: toned.as_str().to_string(),
            name: name.to_string(),
            shortcode: shortcode.map(|s| s.to_string()),
            score,
        });
    }

    // Sort by score (higher is better) and take top 100
    results.sort_by_key(|e| std::cmp::Reverse(e.score));
    results.truncate(100);

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_finds_smile_emoji() {
        let results = find_matching_emojis("smile", SkinTone::Default);

        assert!(!results.is_empty(), "Should find emojis matching 'smile'");

        // Should find the smile emoji somewhere in results
        let has_smile = results
            .iter()
            .any(|e| e.shortcode.as_deref() == Some("smile"));
        assert!(has_smile, "Should find ':smile' emoji in results");
    }

    #[test]
    fn test_finds_multiple_smile_variants() {
        let results = find_matching_emojis("smile", SkinTone::Default);

        // Should find multiple emojis with "smile" in them
        assert!(
            results.len() >= 3,
            "Should find multiple smile-related emojis"
        );

        // Verify at least some results contain "smile"
        let smile_matches = results
            .iter()
            .filter(|e| {
                e.shortcode.as_ref().is_some_and(|s| s.contains("smile"))
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
        let results = find_matching_emojis("heart", SkinTone::Default);

        assert!(!results.is_empty(), "Should find emojis matching 'heart'");

        let has_heart = results
            .iter()
            .any(|e| e.shortcode.as_deref() == Some("heart"));
        assert!(has_heart, "Should find ':heart' emoji in results");
    }

    #[test]
    fn test_finds_cat_emoji() {
        let results = find_matching_emojis("cat", SkinTone::Default);

        assert!(!results.is_empty(), "Should find emojis matching 'cat'");

        let has_cat = results
            .iter()
            .any(|e| e.shortcode.as_deref() == Some("cat"));
        assert!(has_cat, "Should find ':cat' emoji in results");
    }

    #[test]
    fn test_finds_fire_emoji() {
        let results = find_matching_emojis("fire", SkinTone::Default);

        assert!(!results.is_empty(), "Should find emojis matching 'fire'");

        let has_fire = results
            .iter()
            .any(|e| e.shortcode.as_deref() == Some("fire"));
        assert!(has_fire, "Should find ':fire' emoji in results");
    }

    #[test]
    fn test_empty_query_returns_nothing() {
        let results = find_matching_emojis("", SkinTone::Default);
        assert!(results.is_empty(), "Empty query should return no results");
    }

    #[test]
    fn test_results_limited_to_100() {
        let results = find_matching_emojis("e", SkinTone::Default);
        assert!(
            results.len() <= 100,
            "Results should be limited to 100, got {}",
            results.len()
        );
    }

    #[test]
    fn test_scores_are_ordered() {
        let results = find_matching_emojis("smile", SkinTone::Default);

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
    fn test_matching_ignores_case() {
        let lower_results = find_matching_emojis("smile", SkinTone::Default);
        assert!(!lower_results.is_empty());
        assert_eq!(
            find_matching_emojis("SMILE", SkinTone::Default),
            lower_results
        );
        assert_eq!(
            find_matching_emojis("SmIlE", SkinTone::Default),
            lower_results
        );
    }

    #[test]
    fn test_whitespace_query_returns_nothing() {
        assert!(find_matching_emojis(" ", SkinTone::Default).is_empty());
    }

    #[test]
    fn test_matches_two_words() {
        let results = find_matching_emojis("thumbs up", SkinTone::Default);
        assert!(results.iter().any(|e| e.emoji_char == "👍"));
    }

    #[test]
    fn test_matches_second_shortcode() {
        let results = find_matching_emojis("thumbsup", SkinTone::Default);

        let thumbs_up = results
            .iter()
            .find(|e| e.emoji_char == "👍")
            .expect("Should find 👍");
        assert_eq!(thumbs_up.shortcode.as_deref(), Some("thumbsup"));
    }

    #[test]
    fn test_applies_skin_tone() {
        let results = find_matching_emojis("thumbsup", SkinTone::Dark);
        assert_eq!(results[0].emoji_char, "👍🏿");
        assert_eq!(results[0].shortcode.as_deref(), Some("thumbsup"));
    }

    #[test]
    fn test_skin_tone_leaves_other_emojis_alone() {
        let results = find_matching_emojis("sunglasses", SkinTone::Dark);
        assert!(results.iter().any(|e| e.emoji_char == "😎"));
    }

    #[test]
    fn test_skin_tone_applies_to_both_people() {
        let results = find_matching_emojis("people holding hands", SkinTone::Dark);
        assert!(results.iter().any(|e| e.emoji_char == "🧑🏿‍🤝‍🧑🏿"));
    }
}
