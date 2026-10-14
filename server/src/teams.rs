/// Classic Skype and Teams codes, typed as `(code)`, with the Unicode emoji each one stands for.
/// Codes for Teams' own animated emojis are left out because they have no Unicode equivalent.
pub const TEAMS_CODES: &[(&str, &str)] = &[
    ("smile", "🙂"),
    ("happy", "😃"),
    ("laugh", "😄"),
    ("lol", "😂"),
    ("cwl", "😂"),
    ("rofl", "🤣"),
    ("giggle", "🤭"),
    ("wink", "😉"),
    ("blush", "😊"),
    ("cool", "😎"),
    ("inlove", "😍"),
    ("kiss", "😘"),
    ("tongueout", "😛"),
    ("mmm", "😋"),
    ("nerd", "🤓"),
    ("smirk", "😏"),
    ("angel", "😇"),
    ("party", "🥳"),
    ("hug", "🤗"),
    ("think", "🤔"),
    ("wonder", "🤔"),
    ("sad", "🙁"),
    ("cry", "😢"),
    ("sweat", "😓"),
    ("whew", "😅"),
    ("worry", "😟"),
    ("confused", "😕"),
    ("surprised", "😮"),
    ("speechless", "😐"),
    ("dull", "😑"),
    ("envy", "😒"),
    ("unamused", "😒"),
    ("sleepy", "😪"),
    ("yawn", "🥱"),
    ("puke", "🤮"),
    ("drunk", "🥴"),
    ("lipssealed", "🤐"),
    ("angry", "😠"),
    ("swear", "🤬"),
    ("devil", "😈"),
    ("evilgrin", "😈"),
    ("doh", "🤦"),
    ("facepalm", "🤦"),
    ("wasntme", "🤷"),
    ("shrug", "🤷"),
    ("bow", "🙇"),
    ("talk", "🗣️"),
    ("hi", "👋"),
    ("wave", "👋"),
    ("bye", "👋"),
    ("y", "👍"),
    ("yes", "👍"),
    ("like", "👍"),
    ("n", "👎"),
    ("no", "👎"),
    ("ok", "👌"),
    ("clap", "👏"),
    ("handshake", "🤝"),
    ("fistbump", "👊"),
    ("fingerscrossed", "🤞"),
    ("rock", "🤘"),
    ("muscle", "💪"),
    ("wait", "✋"),
    ("dance", "💃"),
    ("ninja", "🥷"),
    ("makeup", "💄"),
    ("heart", "❤️"),
    ("brokenheart", "💔"),
    ("star", "⭐"),
    ("fire", "🔥"),
    ("idea", "💡"),
    ("celebrate", "🎉"),
    ("gift", "🎁"),
    ("cake", "🎂"),
    ("champagne", "🍾"),
    ("beer", "🍺"),
    ("drink", "🍸"),
    ("coffee", "☕"),
    ("pizza", "🍕"),
    ("banana", "🍌"),
    ("cash", "💵"),
    ("mail", "✉️"),
    ("call", "📞"),
    ("phone", "📱"),
    ("time", "⏰"),
    ("music", "🎵"),
    ("movie", "🎬"),
    ("games", "🎮"),
    ("smoke", "🚬"),
    ("flower", "🌹"),
    ("sun", "☀️"),
    ("rain", "🌧️"),
    ("rainbow", "🌈"),
    ("umbrella", "☂️"),
    ("island", "🏝️"),
    ("plane", "✈️"),
    ("car", "🚗"),
    ("bike", "🚲"),
    ("xmastree", "🎄"),
    ("bug", "🐛"),
    ("dog", "🐶"),
    ("cat", "🐱"),
    ("monkey", "🐵"),
    ("pig", "🐷"),
    ("sheep", "🐑"),
    ("penguin", "🐧"),
    ("snail", "🐌"),
    ("turkey", "🦃"),
    ("heidy", "🐿️"),
    ("ghost", "👻"),
    ("skull", "💀"),
    ("poop", "💩"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_emoji_is_known() {
        for (code, emoji) in TEAMS_CODES {
            assert!(
                emojis::get(emoji).is_some(),
                "({code}) maps to unknown {emoji}"
            );
        }
    }

    #[test]
    fn codes_are_unique() {
        let mut codes: Vec<_> = TEAMS_CODES.iter().map(|(code, _)| code).collect();
        codes.sort();
        let count = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), count);
    }

    #[test]
    fn codes_are_single_words() {
        for (code, _) in TEAMS_CODES {
            assert!(code.chars().all(|c| c.is_ascii_alphanumeric()), "({code})");
        }
    }
}
