//! Layered audio stream resolution: InnerTube VISIONOS primary, yt-dlp fallback.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use super::innertube_stream::{FallbackReason, InnertubeResolver};
use super::stream::{AudioQuality, ResolvedStream, StreamResolver};
use super::stream_url_cache::{ResolverSource, StreamUrlCache};
use super::ytdlp::YtDlpResolver;

/// Primary stream resolution tier contract for [`StreamChain`].
pub trait PrimaryStreamResolver: Send + Sync {
    /// Resolves an audio stream for the given video ID and quality preference.
    fn fetch_primary(
        &self,
        video_id: &str,
        quality: AudioQuality,
    ) -> Result<ResolvedStream, FallbackReason>;
}

impl PrimaryStreamResolver for InnertubeResolver {
    fn fetch_primary(
        &self,
        video_id: &str,
        quality: AudioQuality,
    ) -> Result<ResolvedStream, FallbackReason> {
        self.fetch_stream_with(video_id, quality)
    }
}

/// Deduplicates fallback reason logging so each cause logs at most once per session.
fn record_once(logged: &mut HashSet<FallbackReason>, reason: FallbackReason) -> bool {
    logged.insert(reason)
}

/// Layered resolver combining an InnerTube primary tier with a yt-dlp fallback.
#[derive(Clone)]
pub struct StreamChain {
    primary: Option<Arc<dyn PrimaryStreamResolver>>,
    fallback: YtDlpResolver,
    primary_cache: StreamUrlCache,
    quality: AudioQuality,
    logged: Arc<Mutex<HashSet<FallbackReason>>>,
}

impl std::fmt::Debug for StreamChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamChain")
            .field("has_primary", &self.primary.is_some())
            .field("quality", &self.quality)
            .finish()
    }
}

impl StreamChain {
    /// Creates a stream chain with InnerTube primary and yt-dlp fallback.
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            primary: Some(Arc::new(InnertubeResolver::new())),
            fallback: YtDlpResolver::new(binary),
            primary_cache: StreamUrlCache::new(),
            quality: AudioQuality::default(),
            logged: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Constructs a stream chain with an explicit primary resolver and fallback adapter.
    pub fn with_primary(primary: Arc<dyn PrimaryStreamResolver>, fallback: YtDlpResolver) -> Self {
        let quality = fallback.quality();
        Self {
            primary: Some(primary),
            fallback,
            primary_cache: StreamUrlCache::new(),
            quality,
            logged: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Constructs a stream chain using only the yt-dlp fallback tier with primary disabled.
    pub fn from_ytdlp(fallback: YtDlpResolver) -> Self {
        let quality = fallback.quality();
        Self {
            primary: None,
            fallback,
            primary_cache: StreamUrlCache::new(),
            quality,
            logged: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Returns the fallback tier cache.
    ///
    /// This is the legacy URL cache shared with yt-dlp and used by prefetch wiring.
    pub fn cache(&self) -> &StreamUrlCache {
        self.fallback.cache()
    }

    /// Returns the primary tier cache for InnerTube-resolved streams.
    pub fn primary_cache(&self) -> &StreamUrlCache {
        &self.primary_cache
    }

    /// Returns the current audio stream quality preference.
    pub fn quality(&self) -> AudioQuality {
        self.quality
    }

    /// Updates the audio stream quality preference across all tiers.
    pub fn set_quality(&mut self, quality: AudioQuality) {
        self.quality = quality;
        self.fallback.set_quality(quality);
    }

    /// Invalidates cached stream entries for `video_id` across both tiers.
    pub fn invalidate(&self, video_id: &str) {
        self.primary_cache.invalidate(video_id);
        self.fallback.invalidate(video_id);
    }

    fn check_cached(&self, id: &str, now: Instant) -> Option<ResolvedStream> {
        if let Some(stream) =
            self.primary_cache.get_stream(ResolverSource::InnerTube, id, self.quality, now)
        {
            return Some(stream);
        }
        self.fallback.cache().get(id, self.quality, now).map(ResolvedStream::from_url)
    }

    fn resolve_fallback(&self, id: &str) -> Option<ResolvedStream> {
        self.fallback.resolve(id).map(ResolvedStream::from_url)
    }

    fn log_fallback(&self, reason: FallbackReason) {
        let mut guard = self.logged.lock().unwrap_or_else(PoisonError::into_inner);
        if record_once(&mut guard, reason) {
            tracing::info!(
                reason = reason.as_str(),
                "innertube stream resolution failed; using yt-dlp fallback"
            );
        }
    }

    fn resolve_primary(
        &self,
        primary: &dyn PrimaryStreamResolver,
        id: &str,
        now: Instant,
    ) -> Option<ResolvedStream> {
        match primary.fetch_primary(id, self.quality) {
            Ok(stream) => {
                self.primary_cache.insert_stream(
                    ResolverSource::InnerTube,
                    id,
                    self.quality,
                    &stream,
                    now,
                );
                Some(stream)
            }
            Err(reason) => {
                self.log_fallback(reason);
                self.resolve_fallback(id)
            }
        }
    }

    /// Resolves `video_id` to a stream with optional metadata, trying primary tier then fallback.
    pub fn resolve_stream(&self, video_id: &str) -> Option<ResolvedStream> {
        let id = video_id.trim();
        if id.is_empty() {
            return None;
        }

        let now = Instant::now();
        if let Some(cached) = self.check_cached(id, now) {
            return Some(cached);
        }

        let Some(primary) = &self.primary else {
            return self.resolve_fallback(id);
        };

        self.resolve_primary(primary.as_ref(), id, now)
    }

    /// Resolves `video_id` to a direct stream URL through the layered tiers.
    pub fn resolve(&self, video_id: &str) -> Option<String> {
        self.resolve_stream(video_id).map(|stream| stream.url)
    }

    fn is_cached(&self, id: &str, now: Instant) -> bool {
        self.primary_cache.get_stream(ResolverSource::InnerTube, id, self.quality, now).is_some()
            || self.fallback.cache().get(id, self.quality, now).is_some()
    }

    fn prefetch_primary(
        &self,
        primary: &dyn PrimaryStreamResolver,
        id: &str,
        now: Instant,
    ) -> bool {
        match primary.fetch_primary(id, self.quality) {
            Ok(stream) => {
                self.primary_cache.insert_stream(
                    ResolverSource::InnerTube,
                    id,
                    self.quality,
                    &stream,
                    now,
                );
                true
            }
            Err(reason) => {
                self.log_fallback(reason);
                self.fallback.prefetch(id)
            }
        }
    }

    /// Prefetches and caches stream resolution for `video_id`.
    ///
    /// Returns `true` if resolution succeeds or if the entry is already cached.
    pub fn prefetch(&self, video_id: &str) -> bool {
        let id = video_id.trim();
        if id.is_empty() {
            return false;
        }

        let now = Instant::now();
        if self.is_cached(id, now) {
            return true;
        }

        let Some(primary) = &self.primary else {
            return self.fallback.prefetch(id);
        };

        self.prefetch_primary(primary.as_ref(), id, now)
    }
}

impl StreamResolver for StreamChain {
    fn resolve(&self, video_id: &str) -> Option<String> {
        self.resolve_stream(video_id).map(|s| s.url)
    }

    fn resolve_stream(&self, video_id: &str) -> Option<ResolvedStream> {
        self.resolve_stream(video_id)
    }
}

impl From<YtDlpResolver> for StreamChain {
    fn from(resolver: YtDlpResolver) -> Self {
        Self::from_ytdlp(resolver)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakePrimary {
        calls: Mutex<usize>,
        response: Mutex<Option<Result<ResolvedStream, FallbackReason>>>,
    }

    impl FakePrimary {
        fn new(response: Result<ResolvedStream, FallbackReason>) -> Self {
            Self { calls: Mutex::new(0), response: Mutex::new(Some(response)) }
        }

        fn call_count(&self) -> usize {
            *self.calls.lock().unwrap()
        }
    }

    impl PrimaryStreamResolver for FakePrimary {
        fn fetch_primary(
            &self,
            _video_id: &str,
            _quality: AudioQuality,
        ) -> Result<ResolvedStream, FallbackReason> {
            *self.calls.lock().unwrap() += 1;
            self.response.lock().unwrap().clone().unwrap_or(Err(FallbackReason::Http))
        }
    }

    #[test]
    fn primary_success_is_cached_and_metadata_survives() {
        let stream = ResolvedStream {
            url: "https://primary.test/stream".to_string(),
            mime_type: Some("audio/mp4".to_string()),
            loudness_db: Some(1.5),
        };
        let primary = Arc::new(FakePrimary::new(Ok(stream.clone())));
        let fallback = YtDlpResolver::new("/definitely/missing/binary/yt-dlp");
        let chain = StreamChain::with_primary(
            Arc::clone(&primary) as Arc<dyn PrimaryStreamResolver>,
            fallback,
        );

        let first = chain.resolve_stream("v-1").expect("first resolve succeeds");
        assert_eq!(first, stream);
        assert_eq!(primary.call_count(), 1);

        let second = chain.resolve_stream("v-1").expect("second resolve succeeds from cache");
        assert_eq!(second, stream);
        assert_eq!(primary.call_count(), 1);
    }

    #[test]
    fn cached_ytdlp_url_short_circuits_primary() {
        let primary = Arc::new(FakePrimary::new(Err(FallbackReason::Http)));
        let fallback = YtDlpResolver::new("/definitely/missing/binary/yt-dlp");
        fallback.cache().insert(
            "v-fallback",
            AudioQuality::Standard,
            "https://fallback.test/v".to_string(),
            Instant::now(),
        );
        let chain = StreamChain::with_primary(
            Arc::clone(&primary) as Arc<dyn PrimaryStreamResolver>,
            fallback,
        );

        let resolved = chain.resolve_stream("v-fallback").expect("fallback resolves from cache");
        assert_eq!(resolved.url, "https://fallback.test/v");
        assert_eq!(resolved.loudness_db, None);
        assert_eq!(primary.call_count(), 0);
    }

    #[test]
    fn blank_video_id_resolves_to_none() {
        let fallback = YtDlpResolver::new("/definitely/missing/binary/yt-dlp");
        let chain = StreamChain::from_ytdlp(fallback);
        assert_eq!(chain.resolve_stream(""), None);
        assert_eq!(chain.resolve_stream("   "), None);
    }

    #[test]
    fn from_ytdlp_reads_legacy_cache_without_primary() {
        let fallback = YtDlpResolver::new("/definitely/missing/binary/yt-dlp");
        fallback.cache().insert(
            "v-legacy",
            AudioQuality::Standard,
            "https://legacy.test/v".to_string(),
            Instant::now(),
        );
        let chain = StreamChain::from_ytdlp(fallback);
        let stream = chain.resolve_stream("v-legacy").expect("resolves from legacy cache");
        assert_eq!(stream.url, "https://legacy.test/v");
    }

    #[test]
    fn invalidate_clears_primary_and_fallback_caches() {
        let stream = ResolvedStream::from_url("https://primary.test/v-inv");
        let primary = Arc::new(FakePrimary::new(Ok(stream)));
        let fallback = YtDlpResolver::new("/definitely/missing/binary/yt-dlp");
        let chain = StreamChain::with_primary(primary, fallback);

        let _ = chain.resolve_stream("v-inv");
        assert!(
            chain
                .primary_cache()
                .get_stream(
                    ResolverSource::InnerTube,
                    "v-inv",
                    AudioQuality::Standard,
                    Instant::now()
                )
                .is_some()
        );

        chain.cache().insert(
            "v-inv",
            AudioQuality::Standard,
            "https://fallback.test/v-inv".into(),
            Instant::now(),
        );
        assert!(chain.cache().get("v-inv", AudioQuality::Standard, Instant::now()).is_some());

        chain.invalidate("v-inv");
        assert_eq!(
            chain.primary_cache().get_stream(
                ResolverSource::InnerTube,
                "v-inv",
                AudioQuality::Standard,
                Instant::now()
            ),
            None
        );
        assert_eq!(chain.cache().get("v-inv", AudioQuality::Standard, Instant::now()), None);
    }

    #[test]
    fn quality_is_shared_with_fallback_tier() {
        let fallback = YtDlpResolver::new("/definitely/missing/binary/yt-dlp");
        let mut chain = StreamChain::from_ytdlp(fallback);
        chain.set_quality(AudioQuality::High);
        assert_eq!(chain.quality(), AudioQuality::High);

        chain.cache().insert(
            "v-high",
            AudioQuality::High,
            "https://high.test/v".into(),
            Instant::now(),
        );
        let stream = chain.resolve_stream("v-high").expect("resolves high quality stream");
        assert_eq!(stream.url, "https://high.test/v");
    }

    #[test]
    fn record_once_deduplicates_reasons() {
        let mut logged = HashSet::new();
        assert!(record_once(&mut logged, FallbackReason::Http));
        assert!(!record_once(&mut logged, FallbackReason::Http));
        assert!(record_once(&mut logged, FallbackReason::Timeout));
    }

    #[test]
    fn prefetch_disabled_primary_uses_fallback_cache() {
        let fallback = YtDlpResolver::new("/definitely/missing/binary/yt-dlp");
        fallback.cache().insert(
            "v-pre",
            AudioQuality::Standard,
            "https://pre.test/v".into(),
            Instant::now(),
        );
        let chain = StreamChain::from_ytdlp(fallback);
        assert!(chain.prefetch("v-pre"));
    }
}
