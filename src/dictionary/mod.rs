//! User dictionary: fixes what the speech model keeps getting wrong
//! (「クラウドコード」 → "Claude Code").
//!
//! This file is the pure part: the entries and the replacement itself. It
//! works on the transcribed text, so it is the same for every provider.
//! `store` adds the file and the sharing between threads.

pub mod store;

pub use store::{DictionaryError, DictionaryStore};

use serde::{Deserialize, Serialize};

/// How many terms are offered to a provider as recognition hints.
pub const MAX_HINTS: usize = 50;

/// One rule: whenever the model writes `from`, the user meant `to`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// What the model wrote (how the word sounds).
    pub from: String,
    /// What should be typed instead.
    pub to: String,
}

impl Entry {
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dictionary {
    #[serde(default, rename = "entry")]
    pub entries: Vec<Entry>,
}

impl Dictionary {
    pub fn new(entries: Vec<Entry>) -> Self {
        Self { entries }
    }

    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string(self).expect("a dictionary always serializes")
    }

    /// Replaces every occurrence of an entry's `from` with its `to`.
    ///
    /// Matching ignores what the model varies between runs: spaces and `・`
    /// inside the word, hiragana vs katakana, full-width vs half-width
    /// letters, and letter case. The longest entry wins where several
    /// match, and replaced text is never matched again. An entry that
    /// starts or ends with a Latin letter or digit only matches at a word
    /// boundary, so `AI` does not fire inside `main`.
    pub fn apply(&self, text: &str) -> String {
        let mut rules: Vec<(Vec<char>, &str)> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let pattern: Vec<char> = entry.from.chars().filter_map(fold).collect();
                (!pattern.is_empty()).then_some((pattern, entry.to.as_str()))
            })
            .collect();
        if rules.is_empty() {
            return text.to_owned();
        }
        // Stable sort: among equally long patterns the earlier entry wins.
        rules.sort_by_key(|rule| std::cmp::Reverse(rule.0.len()));

        // The text as the matcher sees it, each char with its byte span in
        // the original so the untouched parts are copied verbatim.
        let folded: Vec<(char, usize, usize)> = text
            .char_indices()
            .filter_map(|(start, c)| fold(c).map(|f| (f, start, start + c.len_utf8())))
            .collect();

        let mut out = String::with_capacity(text.len());
        let mut copied = 0;
        let mut i = 0;
        while i < folded.len() {
            let hit = rules
                .iter()
                .find(|(pattern, _)| matches_at(text, &folded, i, pattern));
            match hit {
                Some((pattern, to)) => {
                    let start = folded[i].1;
                    let end = folded[i + pattern.len() - 1].2;
                    out.push_str(&text[copied..start]);
                    out.push_str(to);
                    copied = end;
                    i += pattern.len();
                }
                None => i += 1,
            }
        }
        out.push_str(&text[copied..]);
        out
    }

    /// The correct spellings, for providers that accept vocabulary hints.
    pub fn hints(&self) -> Vec<String> {
        let mut hints: Vec<String> = Vec::new();
        for entry in &self.entries {
            let term = entry.to.trim();
            if !term.is_empty() && !hints.iter().any(|h| h == term) {
                hints.push(term.to_owned());
            }
            if hints.len() == MAX_HINTS {
                break;
            }
        }
        hints
    }
}

/// The form a character is compared in; `None` means it is skipped.
fn fold(c: char) -> Option<char> {
    if c.is_whitespace() || c == '・' {
        return None;
    }
    let c = match c {
        // Full-width ASCII -> ASCII.
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        // Hiragana -> katakana.
        '\u{3041}'..='\u{3096}' => char::from_u32(c as u32 + 0x60).unwrap_or(c),
        _ => c,
    };
    Some(c.to_lowercase().next().unwrap_or(c))
}

fn matches_at(text: &str, folded: &[(char, usize, usize)], at: usize, pattern: &[char]) -> bool {
    let Some(window) = folded.get(at..at + pattern.len()) else {
        return false;
    };
    if !window.iter().map(|(c, _, _)| c).eq(pattern.iter()) {
        return false;
    }
    let (first, start, _) = window[0];
    let (last, _, end) = window[window.len() - 1];
    let glued = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    if first.is_ascii_alphanumeric() && glued(text[..start].chars().next_back()) {
        return false;
    }
    if last.is_ascii_alphanumeric() && glued(text[end..].chars().next()) {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict(pairs: &[(&str, &str)]) -> Dictionary {
        Dictionary::new(pairs.iter().map(|(f, t)| Entry::new(*f, *t)).collect())
    }

    #[test]
    fn replaces_the_registered_word_and_keeps_the_rest() {
        let d = dict(&[("クラウドコード", "Claude Code")]);
        assert_eq!(
            d.apply("クラウドコードで実装してください"),
            "Claude Codeで実装してください"
        );
        assert_eq!(d.apply("何も変わらない文"), "何も変わらない文");
    }

    #[test]
    fn every_occurrence_is_replaced() {
        let d = dict(&[("クラウドコード", "Claude Code")]);
        assert_eq!(
            d.apply("クラウドコードとクラウドコード"),
            "Claude CodeとClaude Code"
        );
    }

    #[test]
    fn spaces_kana_type_width_and_case_do_not_matter() {
        let d = dict(&[
            ("クラウドコード", "Claude Code"),
            ("cloud code", "Claude Code"),
        ]);
        assert_eq!(d.apply("クラウド コードを開く"), "Claude Codeを開く");
        assert_eq!(d.apply("クラウド・コードを開く"), "Claude Codeを開く");
        assert_eq!(d.apply("くらうどこーどを開く"), "Claude Codeを開く");
        assert_eq!(d.apply("Cloud Codeを開く"), "Claude Codeを開く");
        assert_eq!(d.apply("ＣｌｏｕｄCodeを開く"), "Claude Codeを開く");
    }

    #[test]
    fn spaces_around_a_match_are_left_alone() {
        let d = dict(&[("クラウドコード", "Claude Code")]);
        assert_eq!(d.apply("では クラウドコード で"), "では Claude Code で");
    }

    #[test]
    fn the_longest_entry_wins() {
        let d = dict(&[("クラウド", "cloud"), ("クラウドコード", "Claude Code")]);
        assert_eq!(d.apply("クラウドコードとクラウド"), "Claude Codeとcloud");
    }

    #[test]
    fn replaced_text_is_not_matched_again() {
        let d = dict(&[("a", "b"), ("b", "a")]);
        assert_eq!(d.apply("a b"), "b a");
    }

    #[test]
    fn latin_entries_only_match_whole_words() {
        let d = dict(&[("ai", "AI")]);
        assert_eq!(d.apply("main ai fail"), "main AI fail");
        assert_eq!(d.apply("aiを使う"), "AIを使う");
    }

    #[test]
    fn blank_entries_are_ignored() {
        let d = dict(&[("", "x"), ("  ", "y")]);
        assert_eq!(d.apply("そのまま"), "そのまま");
    }

    #[test]
    fn hints_are_the_distinct_spellings_in_order() {
        let d = dict(&[
            ("クラウドコード", "Claude Code"),
            ("クロードコード", "Claude Code"),
            ("ぎっとはぶ", "GitHub"),
            ("x", " "),
        ]);
        assert_eq!(d.hints(), vec!["Claude Code", "GitHub"]);
    }

    #[test]
    fn toml_round_trips() {
        let d = dict(&[("クラウドコード", "Claude Code"), ("ぎっとはぶ", "GitHub")]);
        let text = d.to_toml();
        assert!(text.contains("[[entry]]"), "{text}");
        assert_eq!(Dictionary::from_toml(&text).unwrap(), d);
        assert_eq!(Dictionary::from_toml("").unwrap(), Dictionary::default());
        assert_eq!(
            Dictionary::from_toml(&Dictionary::default().to_toml()).unwrap(),
            Dictionary::default()
        );
    }
}
