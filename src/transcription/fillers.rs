//! Removes spoken Japanese fillers ("えーと", "あのー", "うーん" ...) from a
//! transcript, for every provider.
//!
//! Deliberately conservative: a filler is only removed when it stands on its
//! own (text start or punctuation/whitespace before it, punctuation,
//! whitespace or text end after it) and is spelled in a way no ordinary word
//! is — with a long-vowel mark, or as one of the "えっと" forms. Plain
//! "あの" / "その" / "まあ" are never touched ("あの人", "まあまあ"), and an
//! exclamation such as "えー！" is kept.

use super::provider::{TranscriptionError, TranscriptionProvider};
use crate::audio::audio_data::AudioData;

/// Wraps a provider and strips fillers from whatever it returns.
pub struct FillerStripping(pub std::sync::Arc<dyn TranscriptionProvider>);

#[async_trait::async_trait]
impl TranscriptionProvider for FillerStripping {
    async fn transcribe(&self, audio: AudioData) -> Result<String, TranscriptionError> {
        self.0.transcribe(audio).await.map(|t| strip_fillers(&t))
    }

    async fn transcribe_with_hints(
        &self,
        audio: AudioData,
        hints: &[String],
    ) -> Result<String, TranscriptionError> {
        self.0
            .transcribe_with_hints(audio, hints)
            .await
            .map(|t| strip_fillers(&t))
    }
}

/// Returns `text` without standalone fillers. Text containing none comes
/// back unchanged, byte for byte.
pub fn strip_fillers(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    // True at the text start, after a separator, and right after a filler.
    let mut at_boundary = true;
    // True while nothing but removed fillers precedes in this sentence.
    let mut sentence_start = true;

    while i < chars.len() {
        if at_boundary {
            if let Some(end) = filler_end(&chars[i..]).map(|len| i + len) {
                i = end;
                // The filler's own comma / spacing goes with it.
                while i < chars.len() && (is_comma(chars[i]) || chars[i].is_whitespace()) {
                    i += 1;
                }
                let sentence_ends = i == chars.len() || is_period(chars[i]);
                if sentence_ends {
                    // "明日は、えーと。" -> "明日は。", not "明日は、。"
                    let kept = out.trim_end_matches(|c: char| is_comma(c) || c.is_whitespace());
                    out.truncate(kept.len());
                    if sentence_start {
                        // The sentence was nothing but fillers: drop its period.
                        while i < chars.len() && (is_period(chars[i]) || chars[i].is_whitespace()) {
                            i += 1;
                        }
                    }
                }
                continue;
            }
        }
        let c = chars[i];
        out.push(c);
        at_boundary = is_separator(c);
        if is_period(c) || matches!(c, '!' | '?' | '！' | '？' | '\n') {
            sentence_start = true;
        } else if !is_separator(c) {
            sentence_start = false;
        }
        i += 1;
    }

    if out.trim().is_empty() {
        out.clear();
    }
    out
}

/// Length in chars of the filler `s` starts with, if it is followed by a
/// place where a filler may end.
fn filler_end(s: &[char]) -> Option<usize> {
    let (len, needs_right_boundary) = match_filler(s)?;
    let closed = match s.get(len) {
        None => true,
        Some(&c) => is_comma(c) || is_period(c) || c.is_whitespace(),
    };
    (closed || !needs_right_boundary).then_some(len)
}

/// `(length, needs_right_boundary)`. Only the "えーと" forms are distinctive
/// enough to be removed when text follows directly ("えーと明日は").
fn match_filler(s: &[char]) -> Option<(usize, bool)> {
    let at = |i: usize| s.get(i).copied();
    let longs = |from: usize| s.iter().skip(from).take_while(|&&c| is_long(c)).count();

    match at(0)? {
        'え' => {
            // えーと / えーっと / えっと (+ trailing ー)
            let n = longs(1);
            let mut j = 1 + n;
            let small_tsu = at(j) == Some('っ');
            if small_tsu {
                j += 1;
            }
            if (n > 0 || small_tsu) && at(j) == Some('と') {
                return Some((j + 1 + longs(j + 1), false));
            }
            // ええと: boundary required, "ええとこ" is a word.
            if at(1) == Some('え') && at(2) == Some('と') {
                return Some((3 + longs(3), true));
            }
            (n > 0).then_some((1 + n, true)) // えー
        }
        'あ' => {
            let n = longs(1);
            if n > 0 {
                return Some((1 + n, true)); // あー
            }
            if at(1) != Some('の') {
                return None;
            }
            let m = longs(2);
            if m > 0 {
                Some((2 + m, true)) // あのー
            } else if matches!(at(2), Some('う' | 'ぅ')) {
                Some((3 + longs(3), true)) // あのう
            } else {
                None
            }
        }
        'そ' => {
            let m = longs(2);
            (at(1) == Some('の') && m > 0).then_some((2 + m, true)) // そのー
        }
        'う' => {
            // うーん / うーんと
            let n = longs(1);
            if n == 0 || at(1 + n) != Some('ん') {
                return None;
            }
            let mut j = 2 + n + longs(2 + n);
            if at(j) == Some('と') {
                j += 1 + longs(j + 1);
            }
            Some((j, true))
        }
        'ん' => {
            let n = longs(1);
            (n > 0).then_some((1 + n, true)) // んー
        }
        _ => None,
    }
}

fn is_long(c: char) -> bool {
    matches!(c, 'ー' | '〜' | '～' | '~')
}

fn is_comma(c: char) -> bool {
    matches!(c, '、' | '，' | ',')
}

fn is_period(c: char) -> bool {
    matches!(c, '。' | '．' | '.' | '…')
}

/// Anything a standalone filler may directly follow.
fn is_separator(c: char) -> bool {
    c.is_whitespace()
        || is_comma(c)
        || is_period(c)
        || matches!(
            c,
            '!' | '?' | '！' | '？' | '「' | '」' | '（' | '）' | '(' | ')'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leading_fillers_and_their_commas_are_removed() {
        assert_eq!(
            strip_fillers("えーと、あのー、明日の会議なんですけど"),
            "明日の会議なんですけど"
        );
        assert_eq!(strip_fillers("えっと 明日です"), "明日です");
        assert_eq!(strip_fillers("うーん、どうしよう"), "どうしよう");
        assert_eq!(strip_fillers("え〜、本日は"), "本日は");
    }

    #[test]
    fn mid_sentence_fillers_leave_one_comma_behind() {
        assert_eq!(
            strip_fillers("明日は、えーと、会議です。"),
            "明日は、会議です。"
        );
        assert_eq!(strip_fillers("それで、あのー、んー、次に"), "それで、次に");
    }

    #[test]
    fn a_filler_before_the_period_takes_its_comma_with_it() {
        assert_eq!(strip_fillers("明日は、えーと。"), "明日は。");
        assert_eq!(strip_fillers("明日は会議です、えー"), "明日は会議です");
    }

    #[test]
    fn a_sentence_of_nothing_but_fillers_disappears() {
        assert_eq!(
            strip_fillers("えーと。明日は会議です。"),
            "明日は会議です。"
        );
        assert_eq!(
            strip_fillers("会議です。えー、あのー。次に進みます。"),
            "会議です。次に進みます。"
        );
        assert_eq!(strip_fillers("えーと、あのー。"), "");
        assert_eq!(strip_fillers("うーん"), "");
    }

    #[test]
    fn the_eeto_forms_are_removed_even_when_text_follows_directly() {
        assert_eq!(strip_fillers("えーと明日は会議です"), "明日は会議です");
        assert_eq!(strip_fillers("えーっとあのー、明日"), "明日");
        // ...but only at a boundary, and the others need one on both sides.
        assert_eq!(strip_fillers("あのー明日は"), "あのー明日は");
        assert_eq!(strip_fillers("それでえーと明日"), "それでえーと明日");
    }

    #[test]
    fn ordinary_words_and_exclamations_are_left_alone() {
        for text in [
            "あの人はその本をまあまあ気に入った。",
            "あの、すみません",
            "なんか変だ",
            "ええ、そうです",
            "ええとこやな",
            "えー！本当に？",
            "カラーコーディネーター",
            "あーだこーだ言う",
            "んーっと",
            "Hello, world. This is a test.",
            "  前後の空白はそのまま  ",
            "",
        ] {
            assert_eq!(strip_fillers(text), text);
        }
    }

    #[tokio::test]
    async fn the_wrapper_strips_what_the_inner_provider_returns() {
        struct Fixed;
        #[async_trait::async_trait]
        impl TranscriptionProvider for Fixed {
            async fn transcribe(&self, _: AudioData) -> Result<String, TranscriptionError> {
                Ok("えーと、こんにちは".into())
            }
        }

        let result = FillerStripping(std::sync::Arc::new(Fixed))
            .transcribe(AudioData::new(16_000, vec![0; 160]))
            .await;
        assert_eq!(result, Ok("こんにちは".into()));
    }
}
