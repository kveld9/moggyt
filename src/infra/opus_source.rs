//! Opus WebM audio source and packet decoding.
//!
//! Provides a rodio [`Source`] adapter decoding Opus audio packets demuxed
//! from WebM containers via Symphonia and decoded using libopus.

use std::collections::VecDeque;
use std::io::{Read, Seek};
use std::time::Duration;

use anyhow::{Context, anyhow};
use opus::{Channels, Decoder};
use rodio::Source;
use symphonia::core::formats::FormatReader;
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::probe::Hint;

/// Default sample rate for Opus audio streams (48 kHz).
pub const OPUS_SAMPLE_RATE: u32 = 48_000;

/// Default channel count for stereo Opus playback.
pub const OPUS_DEFAULT_CHANNELS: u16 = 2;

/// Maximum number of samples per channel in a single Opus frame (120ms at 48 kHz).
pub const OPUS_MAX_FRAME_SAMPLES_PER_CHANNEL: usize = 5760;

/// Returns true if the stream metadata or URL indicates an Opus WebM audio stream.
pub fn is_opus_webm(mime_type: Option<&str>, url: &str) -> bool {
    if let Some(mime) = mime_type {
        let lower = mime.to_ascii_lowercase();
        if lower.contains("webm") || lower.contains("opus") {
            return true;
        }
    }

    let url_lower = url.to_ascii_lowercase();
    url_lower.contains(".webm")
        || url_lower.contains("mime=audio%2fwebm")
        || url_lower.contains("mime=audio/webm")
        || url_lower.contains("webm")
}

/// EBML/WebM magic header bytes.
pub const WEBM_EBML_MAGIC: [u8; 4] = [0x1A, 0x45, 0xDF, 0xA3];

/// Reads the first four bytes of a seekable stream and reports whether they are the
/// EBML/WebM magic header, always rewinding the reader to position 0.
pub fn sniff_webm_header<R: Read + Seek>(reader: &mut R) -> std::io::Result<bool> {
    let mut magic = [0u8; 4];
    let is_webm = match reader.read_exact(&mut magic) {
        Ok(()) => magic == WEBM_EBML_MAGIC,
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => false,
        Err(err) => return Err(err),
    };
    reader.seek(std::io::SeekFrom::Start(0))?;
    Ok(is_webm)
}

/// Opus packet decoder wrapping libopus [`Decoder`].
pub struct OpusPacketDecoder {
    decoder: Decoder,
    channels: u16,
    scratch: Vec<f32>,
}

impl OpusPacketDecoder {
    /// Creates a new `OpusPacketDecoder` for the specified sample rate and channel count.
    pub fn new(sample_rate: u32, channels: u16) -> anyhow::Result<Self> {
        let opus_channels = match channels {
            1 => Channels::Mono,
            _ => Channels::Stereo,
        };
        let decoder = Decoder::new(sample_rate, opus_channels)
            .context("failed to initialize libopus decoder")?;
        let max_samples = OPUS_MAX_FRAME_SAMPLES_PER_CHANNEL * (channels as usize).max(1);
        Ok(Self { decoder, channels, scratch: vec![0.0; max_samples] })
    }

    /// Decodes an Opus packet into interleaved 32-bit floating point PCM samples.
    pub fn decode_packet(&mut self, packet: &[u8]) -> anyhow::Result<&[f32]> {
        let samples_per_channel = self
            .decoder
            .decode_float(packet, &mut self.scratch, false)
            .context("failed to decode opus frame")?;
        let total_samples = samples_per_channel * (self.channels as usize);
        Ok(&self.scratch[..total_samples])
    }

    /// Resets the decoder state so playback can resume from a seeked packet boundary.
    pub fn reset(&mut self) -> anyhow::Result<()> {
        self.decoder.reset_state().context("failed to reset libopus decoder")
    }

    /// Returns the configured channel count.
    pub fn channels(&self) -> u16 {
        self.channels
    }
}

/// Abstract packet reader feeding raw encoded packets to [`OpusWebmSource`].
pub trait PacketReader: Send {
    /// Pulls the next encoded audio packet, or returns `Ok(None)` on end of stream.
    fn next_packet(&mut self) -> anyhow::Result<Option<Vec<u8>>>;

    /// Seeks to `position`, returning the position actually reached (never after `position`).
    fn seek(&mut self, _position: std::time::Duration) -> anyhow::Result<std::time::Duration> {
        Err(anyhow!("seek is not supported by this packet reader"))
    }
}

struct SymphoniaDemuxer {
    format: Box<dyn FormatReader>,
    track_id: u32,
    time_base: symphonia::core::units::TimeBase,
}

impl SymphoniaDemuxer {
    fn new(
        format: Box<dyn FormatReader>,
        track_id: u32,
        time_base: symphonia::core::units::TimeBase,
    ) -> Self {
        Self { format, track_id, time_base }
    }
}

impl PacketReader for SymphoniaDemuxer {
    fn next_packet(&mut self) -> anyhow::Result<Option<Vec<u8>>> {
        loop {
            match self.format.next_packet() {
                Ok(packet) => {
                    if packet.track_id() == self.track_id {
                        return Ok(Some(packet.data.to_vec()));
                    }
                }
                Err(symphonia::core::errors::Error::IoError(err))
                    if err.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(None);
                }
                Err(symphonia::core::errors::Error::ResetRequired) => {
                    return Ok(None);
                }
                Err(err) => {
                    return Err(anyhow!("symphonia demuxer error: {err}"));
                }
            }
        }
    }

    fn seek(&mut self, position: std::time::Duration) -> anyhow::Result<std::time::Duration> {
        use symphonia::core::formats::{SeekMode, SeekTo};
        let to = SeekTo::Time { time: position.into(), track_id: Some(self.track_id) };
        let seeked = self
            .format
            .seek(SeekMode::Accurate, to)
            .with_context(|| format!("failed to seek webm stream to {position:?}"))?;
        let time = self.time_base.calc_time(seeked.actual_ts);
        Ok(std::time::Duration::from_secs(time.seconds)
            + std::time::Duration::from_nanos((time.frac * 1_000_000_000.0) as u64))
    }
}

/// In-memory packet queue used for testing and decoupled packet playback.
pub struct QueuePacketReader {
    packets: VecDeque<Vec<u8>>,
}

impl QueuePacketReader {
    pub fn new(packets: impl IntoIterator<Item = Vec<u8>>) -> Self {
        Self { packets: packets.into_iter().collect() }
    }
}

impl PacketReader for QueuePacketReader {
    fn next_packet(&mut self) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(self.packets.pop_front())
    }

    /// Discards whole 20 ms Opus packets up to `position`, returning the actual duration skipped.
    ///
    /// This test helper models 20 ms Opus frames (960 samples per channel at 48 kHz).
    fn seek(&mut self, position: std::time::Duration) -> anyhow::Result<std::time::Duration> {
        let frames_to_skip = (position.as_millis() / 20) as usize;
        let mut skipped = 0usize;
        while skipped < frames_to_skip && self.packets.pop_front().is_some() {
            skipped += 1;
        }
        Ok(std::time::Duration::from_millis(20 * skipped as u64))
    }
}

/// Probes the total byte length exposed by a seekable reader, restoring its position.
///
/// Returns `None` when the reader cannot seek relative to its end, which is how the
/// range reader behaves before it learns the remote total length.
fn probe_stream_len<R: Seek>(reader: &mut R) -> Option<u64> {
    let start = reader.stream_position().ok()?;
    let end = reader.seek(std::io::SeekFrom::End(0)).ok()?;
    reader.seek(std::io::SeekFrom::Start(start)).ok()?;
    Some(end)
}

struct MediaSourceAdapter<R> {
    inner: R,
    len: Option<u64>,
}

impl<R: Read> Read for MediaSourceAdapter<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl<R: Seek> Seek for MediaSourceAdapter<R> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

impl<R: Read + Seek + Send + Sync> MediaSource for MediaSourceAdapter<R> {
    /// Reports seekability only when the byte length is known: Symphonia's Matroska
    /// demuxer rejects a source that claims to be seekable without a length.
    fn is_seekable(&self) -> bool {
        self.len.is_some()
    }

    fn byte_len(&self) -> Option<u64> {
        self.len
    }
}

/// Rodio audio source decoding Opus streams from WebM containers.
pub struct OpusWebmSource {
    packet_reader: Box<dyn PacketReader>,
    decoder: OpusPacketDecoder,
    sample_buf: VecDeque<f32>,
    channels: u16,
    sample_rate: u32,
    total_duration: Option<Duration>,
}

impl OpusWebmSource {
    /// Creates a new `OpusWebmSource` reading and demuxing from a stream.
    pub fn new<R>(mut reader: R) -> anyhow::Result<Self>
    where
        R: Read + Seek + Send + Sync + 'static,
    {
        let len = probe_stream_len(&mut reader);
        let adapter = MediaSourceAdapter { inner: reader, len };
        let mss = MediaSourceStream::new(Box::new(adapter), Default::default());

        let mut hint = Hint::new();
        hint.with_extension("webm");
        hint.mime_type("audio/webm");

        let probe = symphonia::default::get_probe();
        let probed = probe
            .format(
                &hint,
                mss,
                &symphonia::core::formats::FormatOptions::default(),
                &symphonia::core::meta::MetadataOptions::default(),
            )
            .context("failed to probe webm format")?;

        let format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or_else(|| anyhow!("no playable audio track found in webm stream"))?;

        let track_id = track.id;
        let sample_rate = track.codec_params.sample_rate.unwrap_or(OPUS_SAMPLE_RATE);
        let channels =
            track.codec_params.channels.map(|c| c.count() as u16).unwrap_or(OPUS_DEFAULT_CHANNELS);

        let total_duration = track
            .codec_params
            .time_base
            .zip(track.codec_params.n_frames)
            .map(|(tb, frames)| tb.calc_time(frames))
            .map(|ts| {
                Duration::from_secs(ts.seconds)
                    .saturating_add(Duration::from_nanos((ts.frac * 1_000_000_000.0) as u64))
            });

        let time_base =
            track.codec_params.time_base.unwrap_or(symphonia::core::units::TimeBase::new(1, 1000));
        let packet_reader = Box::new(SymphoniaDemuxer::new(format, track_id, time_base));
        let decoder = OpusPacketDecoder::new(sample_rate, channels)?;

        Ok(Self {
            packet_reader,
            decoder,
            sample_buf: VecDeque::new(),
            channels,
            sample_rate,
            total_duration,
        })
    }

    /// Constructs an `OpusWebmSource` from pre-demuxed encoded Opus packets for testing.
    pub fn source_from_packets(packets: Vec<Vec<u8>>) -> anyhow::Result<Self> {
        let sample_rate = OPUS_SAMPLE_RATE;
        let channels = OPUS_DEFAULT_CHANNELS;
        let packet_reader = Box::new(QueuePacketReader::new(packets));
        let decoder = OpusPacketDecoder::new(sample_rate, channels)?;

        Ok(Self {
            packet_reader,
            decoder,
            sample_buf: VecDeque::new(),
            channels,
            sample_rate,
            total_duration: None,
        })
    }
}

impl Iterator for OpusWebmSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        while self.sample_buf.is_empty() {
            let packet = match self.packet_reader.next_packet() {
                Ok(Some(pkt)) => pkt,
                Ok(None) => return None,
                Err(err) => {
                    tracing::warn!(error = ?err, "failed reading next packet");
                    return None;
                }
            };

            match self.decoder.decode_packet(&packet) {
                Ok(samples) => {
                    self.sample_buf.extend(samples.iter().copied());
                }
                Err(err) => {
                    tracing::warn!(error = ?err, "failed decoding opus packet");
                    return None;
                }
            }
        }

        self.sample_buf.pop_front()
    }
}

impl Source for OpusWebmSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }

    fn try_seek(&mut self, pos: std::time::Duration) -> Result<(), rodio::source::SeekError> {
        let result = self.packet_reader.seek(pos).and_then(|_target| {
            self.sample_buf.clear();
            self.decoder.reset()
        });
        result.map_err(|err| {
            tracing::warn!(error = ?err, "failed to seek opus stream");
            rodio::source::SeekError::NotSupported { underlying_source: "OpusWebmSource" }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opus::{Application, Encoder};

    struct EndSeekUnsupported {
        inner: std::io::Cursor<Vec<u8>>,
    }

    impl std::io::Read for EndSeekUnsupported {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.inner.read(buf)
        }
    }

    impl std::io::Seek for EndSeekUnsupported {
        fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
            if matches!(pos, std::io::SeekFrom::End(_)) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "cannot seek from end",
                ));
            }
            self.inner.seek(pos)
        }
    }

    /// Minimal WebM container with a single Opus track (EBML header, Segment, Info, Tracks).
    const MINIMAL_OPUS_WEBM: [u8; 93] = [
        0x1a, 0x45, 0xdf, 0xa3, 0x9f, 0x42, 0x86, 0x81, 0x01, 0x42, 0xf7, 0x81, 0x01, 0x42, 0xf2,
        0x81, 0x04, 0x42, 0xf3, 0x81, 0x08, 0x42, 0x82, 0x84, 0x77, 0x65, 0x62, 0x6d, 0x42, 0x87,
        0x81, 0x02, 0x42, 0x85, 0x81, 0x02, 0x18, 0x53, 0x80, 0x67, 0xb4, 0x15, 0x49, 0xa9, 0x66,
        0x87, 0x2a, 0xd7, 0xb1, 0x83, 0x0f, 0x42, 0x40, 0x16, 0x54, 0xae, 0x6b, 0xa3, 0xae, 0xa1,
        0xd7, 0x81, 0x01, 0x73, 0xc5, 0x81, 0x01, 0x83, 0x81, 0x02, 0x86, 0x86, 0x41, 0x5f, 0x4f,
        0x50, 0x55, 0x53, 0xe1, 0x8d, 0xb5, 0x88, 0x40, 0xe7, 0x70, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x9f, 0x81, 0x01,
    ];

    #[test]
    fn probe_stream_len_reports_length_and_restores_position() {
        let mut cursor = std::io::Cursor::new(vec![0u8; 12]);
        cursor.set_position(3);

        assert_eq!(probe_stream_len(&mut cursor), Some(12));
        assert_eq!(cursor.position(), 3);
    }

    #[test]
    fn probe_stream_len_is_none_when_end_seek_is_unsupported() {
        let mut reader = EndSeekUnsupported { inner: std::io::Cursor::new(vec![0u8; 12]) };
        reader.inner.set_position(5);

        assert_eq!(probe_stream_len(&mut reader), None);
        assert_eq!(reader.inner.position(), 5);
    }

    #[test]
    fn media_source_adapter_claims_seekable_only_with_known_length() {
        let seekable =
            MediaSourceAdapter { inner: std::io::Cursor::new(vec![0u8; 8]), len: Some(8) };
        assert!(seekable.is_seekable());
        assert_eq!(seekable.byte_len(), Some(8));

        let streaming =
            MediaSourceAdapter { inner: std::io::Cursor::new(Vec::<u8>::new()), len: None };
        assert!(!streaming.is_seekable());
        assert_eq!(streaming.byte_len(), None);
    }

    #[test]
    fn opus_webm_source_probes_stream_without_known_length() {
        let reader = EndSeekUnsupported { inner: std::io::Cursor::new(MINIMAL_OPUS_WEBM.to_vec()) };

        let _source = OpusWebmSource::new(reader)
            .expect("webm stream must probe even when its length is unknown");
    }

    #[test]
    fn opus_webm_source_probes_seekable_stream_with_known_length() {
        let reader = std::io::Cursor::new(MINIMAL_OPUS_WEBM.to_vec());

        let _source = OpusWebmSource::new(reader)
            .expect("seekable webm stream with a known length must probe");
    }

    #[test]
    fn is_opus_webm_detects_mime_and_url_patterns() {
        assert!(is_opus_webm(Some("audio/webm"), "https://stream.local/audio"));
        assert!(is_opus_webm(Some("audio/webm; codecs=\"opus\""), "https://stream.local/audio"));
        assert!(is_opus_webm(Some("audio/opus"), "https://stream.local/audio"));
        assert!(is_opus_webm(None, "https://stream.local/track.webm"));
        assert!(is_opus_webm(None, "https://stream.local/playback?mime=audio%2Fwebm&itag=251"));
        assert!(is_opus_webm(None, "https://stream.local/playback?mime=audio/webm"));

        assert!(!is_opus_webm(Some("audio/mp4"), "https://stream.local/track.m4a"));
        assert!(!is_opus_webm(Some("audio/aac"), "https://stream.local/track.aac"));
        assert!(!is_opus_webm(None, "https://stream.local/track.m4a"));
        assert!(!is_opus_webm(None, "https://stream.local/track.mp3"));
    }

    #[test]
    fn sniff_webm_header_detects_ebml_magic_and_rewinds() {
        let mut valid = std::io::Cursor::new([0x1A, 0x45, 0xDF, 0xA3, 0x00, 0x01]);
        assert!(sniff_webm_header(&mut valid).unwrap());
        assert_eq!(valid.position(), 0);

        let mut non_webm = std::io::Cursor::new(b"....ftypM4A ");
        assert!(!sniff_webm_header(&mut non_webm).unwrap());
        assert_eq!(non_webm.position(), 0);

        let mut short = std::io::Cursor::new([0x1A, 0x45]);
        assert!(!sniff_webm_header(&mut short).unwrap());
        assert_eq!(short.position(), 0);
    }

    #[test]
    fn opus_packet_decoder_roundtrip_with_encoder() {
        let sample_rate = 48_000;
        let channels = 2;
        let frame_size = 960;
        let mut encoder =
            Encoder::new(sample_rate, Channels::Stereo, Application::Audio).expect("encoder init");
        let mut decoder = OpusPacketDecoder::new(sample_rate, channels).expect("decoder init");

        let mut pcm_in = Vec::with_capacity(frame_size * 2);
        for i in 0..frame_size {
            let t = i as f32 / sample_rate as f32;
            let sample = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.5;
            pcm_in.push(sample);
            pcm_in.push(sample);
        }

        let mut packet = vec![0u8; 1024];
        let packet_len = encoder.encode_float(&pcm_in, &mut packet).expect("encoding succeeds");
        packet.truncate(packet_len);
        assert!(packet_len > 0);

        let decoded = decoder.decode_packet(&packet).expect("decoding packet succeeds");
        assert_eq!(decoded.len(), frame_size * 2);

        let max_amp = decoded.iter().copied().fold(0.0_f32, |acc, s| acc.max(s.abs()));
        assert!(
            max_amp > 0.2,
            "decoded signal should maintain reasonable amplitude, got {max_amp}"
        );
    }

    #[test]
    fn source_from_packets_iterates_and_reports_source_properties() {
        let sample_rate = 48_000;
        let frame_size = 960;
        let mut encoder =
            Encoder::new(sample_rate, Channels::Stereo, Application::Audio).expect("encoder init");

        let mut packets = Vec::new();
        for _ in 0..3 {
            let pcm_in = vec![0.25f32; frame_size * 2];
            let mut packet = vec![0u8; 1024];
            let len = encoder.encode_float(&pcm_in, &mut packet).expect("encode succeeds");
            packet.truncate(len);
            packets.push(packet);
        }

        let mut source = OpusWebmSource::source_from_packets(packets).expect("source init");
        assert_eq!(source.sample_rate(), 48_000);
        assert_eq!(source.channels(), 2);
        assert_eq!(source.total_duration(), None);
        assert_eq!(source.current_span_len(), None);

        let samples: Vec<f32> = source.by_ref().collect();
        assert_eq!(samples.len(), 3 * frame_size * 2);
    }

    #[test]
    fn queue_packet_reader_seek_skips_whole_packets() {
        let mut reader = QueuePacketReader::new(vec![vec![1], vec![2], vec![3], vec![4], vec![5]]);
        assert_eq!(reader.seek(Duration::from_millis(60)).unwrap(), Duration::from_millis(60));
        assert!(reader.next_packet().unwrap().is_some());
        assert!(reader.next_packet().unwrap().is_some());
        assert!(reader.next_packet().unwrap().is_none());

        let mut clamped = QueuePacketReader::new(vec![vec![1], vec![2]]);
        assert_eq!(clamped.seek(Duration::from_secs(10)).unwrap(), Duration::from_millis(40));
        assert!(clamped.next_packet().unwrap().is_none());
    }

    #[test]
    fn opus_source_try_seek_clears_buffered_audio_and_advances_to_target() {
        let sample_rate = 48_000;
        let frame_size = 960;
        let mut encoder =
            Encoder::new(sample_rate, Channels::Stereo, Application::Audio).expect("encoder init");

        let mut packets = Vec::new();
        for _ in 0..10 {
            let pcm_in = vec![0.25f32; frame_size * 2];
            let mut packet = vec![0u8; 1024];
            let len = encoder.encode_float(&pcm_in, &mut packet).expect("encode succeeds");
            packet.truncate(len);
            packets.push(packet);
        }

        let mut source = OpusWebmSource::source_from_packets(packets).expect("source init");
        for _ in 0..(frame_size * 2) {
            assert!(source.next().is_some());
        }

        assert!(source.try_seek(Duration::from_millis(60)).is_ok());

        let rest: Vec<f32> = source.collect();
        assert_eq!(rest.len(), 6 * frame_size * 2);
    }
}
