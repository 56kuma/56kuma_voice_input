//! Applies the user dictionary to whatever a provider returns. It wraps any
//! [`TranscriptionProvider`], so the dictionary works the same with every
//! backend and the controller never knows it exists.

use std::sync::Arc;

use super::provider::{TranscriptionError, TranscriptionProvider};
use crate::audio::audio_data::AudioData;
use crate::dictionary::DictionaryStore;

pub struct DictionaryProvider {
    inner: Arc<dyn TranscriptionProvider>,
    store: Arc<DictionaryStore>,
    /// Also offer the dictionary's spellings to the provider as hints.
    send_hints: bool,
}

impl DictionaryProvider {
    pub fn new(
        inner: Arc<dyn TranscriptionProvider>,
        store: Arc<DictionaryStore>,
        send_hints: bool,
    ) -> Self {
        Self {
            inner,
            store,
            send_hints,
        }
    }
}

#[async_trait::async_trait]
impl TranscriptionProvider for DictionaryProvider {
    async fn transcribe(&self, audio: AudioData) -> Result<String, TranscriptionError> {
        let hints = if self.send_hints {
            self.store.hints()
        } else {
            Vec::new()
        };
        let raw = self.inner.transcribe_with_hints(audio, &hints).await?;
        let text = self.store.apply(&raw);
        self.store.record_text(&text);
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary::{Dictionary, Entry};
    use crate::doubles::FakeTranscriptionProvider;

    fn store() -> Arc<DictionaryStore> {
        Arc::new(DictionaryStore::in_memory(Dictionary::new(vec![
            Entry::new("クラウドコード", "Claude Code"),
        ])))
    }

    fn audio() -> AudioData {
        AudioData::new(16_000, vec![0; 16])
    }

    #[tokio::test]
    async fn the_providers_text_is_corrected_and_remembered() {
        let inner = FakeTranscriptionProvider::returning("クラウドコードを開いて");
        let store = store();
        let provider = DictionaryProvider::new(Arc::new(inner), Arc::clone(&store), true);

        let text = provider.transcribe(audio()).await.unwrap();

        assert_eq!(text, "Claude Codeを開いて");
        assert_eq!(store.last_text().as_deref(), Some("Claude Codeを開いて"));
    }

    #[tokio::test]
    async fn an_edit_made_while_running_applies_to_the_next_transcription() {
        let inner = FakeTranscriptionProvider::returning("ぎっとはぶ");
        let store = store();
        let provider = DictionaryProvider::new(Arc::new(inner), Arc::clone(&store), true);
        assert_eq!(provider.transcribe(audio()).await.unwrap(), "ぎっとはぶ");

        store
            .set_entries(vec![Entry::new("ぎっとはぶ", "GitHub")])
            .unwrap();

        assert_eq!(provider.transcribe(audio()).await.unwrap(), "GitHub");
    }

    #[tokio::test]
    async fn the_spellings_are_offered_as_hints_unless_turned_off() {
        let inner = FakeTranscriptionProvider::returning("x");
        let with = DictionaryProvider::new(Arc::new(inner.clone()), store(), true);
        with.transcribe(audio()).await.unwrap();
        assert_eq!(inner.received_hints(), vec![vec!["Claude Code".to_owned()]]);

        let inner = FakeTranscriptionProvider::returning("x");
        let without = DictionaryProvider::new(Arc::new(inner.clone()), store(), false);
        without.transcribe(audio()).await.unwrap();
        assert_eq!(inner.received_hints(), vec![Vec::<String>::new()]);
    }

    #[tokio::test]
    async fn hints_reach_the_provider_through_filler_stripping() {
        use crate::transcription::fillers::FillerStripping;
        let inner = FakeTranscriptionProvider::returning("えーと、クラウドコード");
        let wrapped = Arc::new(FillerStripping(Arc::new(inner.clone())));
        let provider = DictionaryProvider::new(wrapped, store(), true);

        assert_eq!(provider.transcribe(audio()).await.unwrap(), "Claude Code");
        assert_eq!(inner.received_hints(), vec![vec!["Claude Code".to_owned()]]);
    }

    #[tokio::test]
    async fn a_provider_failure_passes_through_untouched() {
        let inner = FakeTranscriptionProvider::failing(TranscriptionError::Timeout);
        let store = store();
        let provider = DictionaryProvider::new(Arc::new(inner), Arc::clone(&store), true);

        assert_eq!(
            provider.transcribe(audio()).await,
            Err(TranscriptionError::Timeout)
        );
        assert_eq!(store.last_text(), None);
    }
}
