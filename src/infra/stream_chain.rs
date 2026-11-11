//! Layered audio stream resolution: InnerTube VISIONOS primary, yt-dlp fallback.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use super::innertube_stream::{FallbackReason, InnertubeResolver};
use super::stream::{AudioQuality, RangeResponse, RangeTransport, ResolvedStream, StreamResolver};
use super::stream_prefetch_cache::{PREFETCH_BYTES, StreamPrefetchCache};
use super::stream_reqwest::ReqwestRangeTransport;
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

/// Fetches the leading bytes of a resolved stream URL for next-track prefetch.
pub trait PrefixFetcher: Send + Sync {
    fn fetch_prefix(&self, url: &str, max_bytes: usize) -> Option<Vec<u8>>;
}

/// Real HTTP prefix fetcher used by the production chain.
#[derive(Debug, Clone, Default)]
pub struct HttpPrefixFetcher;

fn open_prefix_transport(url: &str) -> Option<ReqwestRangeTransport> {
    match ReqwestRangeTransport::new(url) {
        Ok(transport) => Some(transport),
        Err(err) => {
            tracing::debug!(error = ?err, "failed to create transport for prefix fetch");
            None
        }
    }
}

fn prefix_from_response(
    response: std::io::Result<RangeResponse>,
    max_bytes: usize,
) -> Option<Vec<u8>> {
    match response {
        Ok(RangeResponse::Partial { mut data, .. }) => {
            data.truncate(max_bytes);
            Some(data)
        }
        Ok(RangeResponse::Full(_)) => {
            tracing::debug!("prefix fetch received full response instead of range");
            None
        }
        Err(err) => {
            tracing::debug!(error = ?err, "prefix fetch range request failed");
            None
        }
    }
}

impl PrefixFetcher for HttpPrefixFetcher {
    fn fetch_prefix(&self, url: &str, max_bytes: usize) -> Option<Vec<u8>> {
        let mut transport = open_prefix_transport(url)?;
        prefix_from_response(transport.fetch_range(0, max_bytes), max_bytes)
    }
}

#[derive(Debug, Default)]
struct NoopPrefixFetcher;

impl PrefixFetcher for NoopPrefixFetcher {
    fn fetch_prefix(&self, _url: &str, _max_bytes: usize) -> Option<Vec<u8>> {
        None
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
    prefetch_cache: Arc<StreamPrefetchCache>,
    prefix_fetcher: Arc<dyn PrefixFetcher>,
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
            prefetch_cache: Arc::new(StreamPrefetchCache::new()),
            prefix_fetcher: Arc::new(HttpPrefixFetcher),
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
            prefetch_cache: Arc::new(StreamPrefetchCache::new()),
            prefix_fetcher: Arc::new(NoopPrefixFetcher),
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
            prefetch_cache: Arc::new(StreamPrefetchCache::new()),
            prefix_fetcher: Arc::new(NoopPrefixFetcher),
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

    /// Injects a custom prefix fetcher for testing or custom transport wiring.
    pub fn with_prefix_fetcher(mut self, fetcher: Arc<dyn PrefixFetcher>) -> Self {
        self.prefix_fetcher = fetcher;
        self
    }

    /// Removes and returns prefetched audio bytes for `url` from the cache.
    pub fn take_prefetched(&self, url: &str) -> Option<Vec<u8>> {
        self.prefetch_cache.take(url)
    }

    fn find_or_resolve_stream(&self, id: &str) -> Option<ResolvedStream> {
        let now = Instant::now();
        if let Some(cached) = self.check_cached(id, now) {
            return Some(cached);
        }

        let Some(primary) = &self.primary else {
            return self.resolve_fallback(id);
        };

        self.resolve_primary(primary.as_ref(), id, now)
    }

    /// Resolves `video_id` to a stream with optional metadata, trying primary tier then fallback.
    pub fn resolve_stream(&self, video_id: &str) -> Option<ResolvedStream> {
        let id = video_id.trim();
        if id.is_empty() {
            return None;
        }
        self.find_or_resolve_stream(id)
    }

    /// Resolves `video_id` to a direct stream URL through the layered tiers.
    pub fn resolve(&self, video_id: &str) -> Option<String> {
        self.resolve_stream(video_id).map(|stream| stream.url)
    }

    /// Prefetches the leading audio blocks of `video_id` into the prefetch cache.
    ///
    /// Resolves the stream URL using the same tier order as `resolve_stream`,
    /// skips downloading if the prefix is already cached, and fetches `PREFETCH_BYTES`.
    /// Returns true if the prefix is or becomes cached.
    pub fn prefetch_prefix(&self, video_id: &str) -> bool {
        let id = video_id.trim();
        if id.is_empty() {
            return false;
        }
        let Some(stream) = self.find_or_resolve_stream(id) else {
            return false;
        };
        if self.prefetch_cache.contains(&stream.url) {
            return true;
        }
        self.fetch_and_cache_prefix(&stream.url)
    }

    fn fetch_and_cache_prefix(&self, url: &str) -> bool {
        let Some(bytes) = self.prefix_fetcher.fetch_prefix(url, PREFETCH_BYTES) else {
            return false;
        };
        self.prefetch_cache.insert(url, bytes);
        true
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

    fn prefetch_url(&self, id: &str) -> bool {
        let now = Instant::now();
        if self.is_cached(id, now) {
            return true;
        }

        let Some(primary) = &self.primary else {
            return self.fallback.prefetch(id);
        };

        self.prefetch_primary(primary.as_ref(), id, now)
    }

    /// Prefetches and caches stream resolution for `video_id`.
    ///
    /// Returns `true` if resolution succeeds or if the URL is already cached.
    pub fn prefetch(&self, video_id: &str) -> bool {
        let id = video_id.trim();
        if id.is_empty() {
            return false;
        }
        self.prefetch_url(id)
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

    struct FakePrefixFetcher {
        bytes: Mutex<Option<Vec<u8>>>,
        calls: Mutex<Vec<(String, usize)>>,
    }

    impl FakePrefixFetcher {
        fn new(bytes: Vec<u8>) -> Self {
            Self { bytes: Mutex::new(Some(bytes)), calls: Mutex::new(Vec::new()) }
        }
    }

    impl PrefixFetcher for FakePrefixFetcher {
        fn fetch_prefix(&self, url: &str, max_bytes: usize) -> Option<Vec<u8>> {
            self.calls.lock().unwrap().push((url.to_string(), max_bytes));
            self.bytes.lock().unwrap().clone()
        }
    }

    #[test]
    fn prefetch_prefix_inserts_bytes_with_fake_fetcher() {
        let stream = ResolvedStream::from_url("https://primary.test/v-prefix");
        let primary = Arc::new(FakePrimary::new(Ok(stream)));
        let fallback = YtDlpResolver::new("/definitely/missing/binary/yt-dlp");
        let fetcher = Arc::new(FakePrefixFetcher::new(vec![0xAA; PREFETCH_BYTES]));
        let chain = StreamChain::with_primary(primary, fallback)
            .with_prefix_fetcher(Arc::clone(&fetcher) as Arc<dyn PrefixFetcher>);

        assert!(chain.prefetch_prefix("v-prefix"));
        let calls = fetcher.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], ("https://primary.test/v-prefix".to_string(), PREFETCH_BYTES));

        let prefetched = chain.take_prefetched("https://primary.test/v-prefix");
        assert_eq!(prefetched, Some(vec![0xAA; PREFETCH_BYTES]));
    }

    #[test]
    fn prefetch_prefix_skips_blank_and_unresolved() {
        let fallback = YtDlpResolver::new("/definitely/missing/binary/yt-dlp");
        let fetcher = Arc::new(FakePrefixFetcher::new(vec![0xAA; 100]));
        let chain = StreamChain::from_ytdlp(fallback)
            .with_prefix_fetcher(Arc::clone(&fetcher) as Arc<dyn PrefixFetcher>);

        assert!(!chain.prefetch_prefix(""));
        assert!(!chain.prefetch_prefix("   "));
        assert!(!chain.prefetch_prefix("unresolved-id"));
        assert!(fetcher.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn prefetch_prefix_returns_false_when_fetcher_has_no_bytes() {
        let stream = ResolvedStream::from_url("https://primary.test/v-none");
        let primary = Arc::new(FakePrimary::new(Ok(stream)));
        let fallback = YtDlpResolver::new("/definitely/missing/binary/yt-dlp");
        let chain = StreamChain::with_primary(primary, fallback);

        assert!(!chain.prefetch_prefix("v-none"));
        assert_eq!(chain.take_prefetched("https://primary.test/v-none"), None);
    }

    #[test]
    fn take_prefetched_is_single_use() {
        let stream = ResolvedStream::from_url("https://primary.test/v-single");
        let primary = Arc::new(FakePrimary::new(Ok(stream)));
        let fallback = YtDlpResolver::new("/definitely/missing/binary/yt-dlp");
        let fetcher = Arc::new(FakePrefixFetcher::new(vec![1, 2, 3]));
        let chain = StreamChain::with_primary(primary, fallback)
            .with_prefix_fetcher(Arc::clone(&fetcher) as Arc<dyn PrefixFetcher>);

        assert!(chain.prefetch_prefix("v-single"));
        assert_eq!(chain.take_prefetched("https://primary.test/v-single"), Some(vec![1, 2, 3]));
        assert_eq!(chain.take_prefetched("https://primary.test/v-single"), None);
    }
}
