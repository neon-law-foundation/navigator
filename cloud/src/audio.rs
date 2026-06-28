//! Pure-Rust audio decoding for the Live Inquiry demo.
//!
//! The `live-transcription` CLI lets staff point `--audio` at whatever
//! recording they have — an iPhone Voice Memo (`.m4a`/AAC), an `.mp3`, a
//! `.wav`, a `.flac`, an `.ogg`. Google Speech-to-Text v2's auto-decoding
//! does not accept every container (notably AAC/ALAC `.m4a`), so rather than
//! make the caller pre-convert, we decode the file ourselves with
//! [Symphonia] into 16-bit mono PCM and hand Google raw `LINEAR16` samples.
//!
//! Symphonia probes the format from the file contents (the extension is only
//! a hint), so callers never have to name the codec.
//!
//! [Symphonia]: https://docs.rs/symphonia

use std::fs::File;
use std::path::Path;

use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::audio::{AudioDecoderOptions, CODEC_ID_NULL_AUDIO};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use thiserror::Error;

/// A decoded recording: interleaved-free 16-bit mono PCM and its sample rate.
#[derive(Debug, Clone)]
pub struct DecodedAudio {
    /// Signed 16-bit samples, one channel (mono).
    pub samples: Vec<i16>,
    /// Sample rate in hertz, as reported by the decoder.
    pub sample_rate: u32,
}

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("io error on {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("could not decode audio (unsupported or corrupt format): {0}")]
    Decode(#[from] SymphoniaError),
    #[error("audio file has no decodable audio track")]
    NoTrack,
    #[error("decoder did not report a sample rate")]
    NoSampleRate,
    #[error("audio file decoded to zero samples")]
    Empty,
}

/// Decode any Symphonia-supported audio file into 16-bit mono PCM.
///
/// Multi-channel audio is down-mixed to mono by averaging the channels of
/// each frame, which is what speech recognition wants. The sample rate is
/// preserved (Google STT accepts the native rate via `explicitDecodingConfig`).
///
/// # Errors
///
/// Returns [`AudioError`] if the file cannot be opened, the format is not
/// recognized, no audio track is present, or it decodes to nothing.
pub fn decode_to_mono_pcm16(path: &Path) -> Result<DecodedAudio, AudioError> {
    let file = File::open(path).map_err(|source| AudioError::Io {
        path: path.display().to_string(),
        source,
    })?;
    let stream = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let mut format = symphonia::default::get_probe().probe(
        &hint,
        stream,
        FormatOptions::default(),
        MetadataOptions::default(),
    )?;

    let (track_id, codec_params, mut sample_rate) = format
        .tracks()
        .iter()
        .find_map(|track| match &track.codec_params {
            Some(CodecParameters::Audio(params)) if params.codec != CODEC_ID_NULL_AUDIO => {
                Some((track.id, params.clone(), params.sample_rate.unwrap_or(0)))
            }
            _ => None,
        })
        .ok_or(AudioError::NoTrack)?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&codec_params, &AudioDecoderOptions::default())?;

    let mut samples: Vec<i16> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            // Clean end of stream: Symphonia signals EOF as an io error.
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(e) => return Err(e.into()),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                if sample_rate == 0 {
                    sample_rate = audio_buf.spec().rate();
                }
                append_mono_i16(&audio_buf, &mut samples);
            }
            // A recoverable decode hiccup: skip the packet, keep going.
            Err(SymphoniaError::DecodeError(_)) => {}
            Err(e) => return Err(e.into()),
        }
    }

    if sample_rate == 0 {
        return Err(AudioError::NoSampleRate);
    }
    if samples.is_empty() {
        return Err(AudioError::Empty);
    }
    Ok(DecodedAudio {
        samples,
        sample_rate,
    })
}

/// Convert one decoded buffer to mono `i16` and append it to `out`.
fn append_mono_i16(decoded: &GenericAudioBufferRef<'_>, out: &mut Vec<i16>) {
    let channels = decoded.spec().channels().count().max(1);

    let mut interleaved = Vec::with_capacity(decoded.samples_interleaved());
    decoded.copy_to_vec_interleaved::<i16>(&mut interleaved);

    if channels == 1 {
        out.extend_from_slice(&interleaved);
        return;
    }
    let divisor = i32::try_from(channels).unwrap_or(1).max(1);
    for frame in interleaved.chunks(channels) {
        let sum: i32 = frame.iter().map(|&s| i32::from(s)).sum();
        let avg = sum / divisor;
        out.push(i16::try_from(avg).unwrap_or(if avg < 0 { i16::MIN } else { i16::MAX }));
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_to_mono_pcm16, AudioError};
    use std::io::Write;

    /// A minimal 16-bit mono WAV (440 Hz-ish square wave) written by hand,
    /// decoded back through Symphonia. Proves the round-trip without a
    /// committed binary fixture or any external tool.
    #[test]
    fn decodes_a_handwritten_wav() {
        let sample_rate: u32 = 8000;
        let frames: u32 = 1600; // 0.2s
        let data_len = frames * 2; // 16-bit mono

        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
        wav.extend_from_slice(&2u16.to_le_bytes()); // block align
        wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..frames {
            let s: i16 = if i % 2 == 0 { 8000 } else { -8000 };
            wav.extend_from_slice(&s.to_le_bytes());
        }

        let mut tmp = tempfile::Builder::new()
            .suffix(".wav")
            .tempfile()
            .expect("tempfile");
        tmp.write_all(&wav).expect("write wav");
        tmp.flush().expect("flush");

        let decoded = decode_to_mono_pcm16(tmp.path()).expect("decode wav");
        assert_eq!(decoded.sample_rate, sample_rate);
        assert_eq!(decoded.samples.len(), frames as usize);
    }

    #[test]
    fn rejects_a_non_audio_file() {
        let mut tmp = tempfile::Builder::new()
            .suffix(".m4a")
            .tempfile()
            .expect("tempfile");
        tmp.write_all(b"this is not audio").expect("write");
        tmp.flush().expect("flush");

        let err = decode_to_mono_pcm16(tmp.path()).expect_err("garbage must not decode");
        assert!(
            matches!(err, AudioError::Decode(_) | AudioError::NoTrack),
            "expected a decode/no-track error, got {err:?}"
        );
    }
}
