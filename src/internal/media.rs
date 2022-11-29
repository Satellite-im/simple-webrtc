use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use webrtc::api::media_engine::{
    MIME_TYPE_AV1, MIME_TYPE_G722, MIME_TYPE_H264, MIME_TYPE_OPUS, MIME_TYPE_PCMA, MIME_TYPE_PCMU,
    MIME_TYPE_VP8, MIME_TYPE_VP9,
};

/// Indicates the device from which the media stream originates
#[derive(Serialize, Deserialize)]
pub enum MediaSource {
    /// audio destination
    Microphone,
    /// video destination
    Camera,
    /// video destination
    Screen,
}


/// represents the MIME types from webrtc::api::media_engine
#[derive(Serialize, Deserialize)]
pub enum MimeType {
    // https://en.wikipedia.org/wiki/Advanced_Video_Coding
    // the most popular video compression standard
    // requires paying patent licencing royalties to MPEG LA
    // patent expires in july 2023
    H264,
    // https://en.wikipedia.org/wiki/VP8
    // royalty-free video compression format
    // can be multiplexed into Matroska and WebM containers, along with Vorbis and Opus audio
    VP8,
    // https://en.wikipedia.org/wiki/VP9
    // royalty-free  video coding format
    VP9,
    // https://en.wikipedia.org/wiki/AV1
    // royalty-free video coding format
    // successor to VP9
    AV1,
    // https://en.wikipedia.org/wiki/Opus_(audio_format)
    // lossy audio coding format
    // BSD-3 license
    OPUS,
    // https://en.wikipedia.org/wiki/G.722
    // royalty-free audio codec
    // 7kHz wideband audio at data rates from 48, 56, and 65 kbit/s
    // commonly used for VoIP
    G722,
    // https://en.wikipedia.org//wiki/G.711
    // royalty free audio codec
    // narrowband audio codec
    // also known as G.711 µ-law
    PCMU,
    // https://en.wikipedia.org//wiki/G.711
    // also known as G.711 A-law
    PCMA,
}

impl ToString for MimeType {
    fn to_string(&self) -> String {
        let s = match self {
            MimeType::H264 => MIME_TYPE_H264,
            MimeType::VP8 => MIME_TYPE_VP8,
            MimeType::VP9 => MIME_TYPE_VP9,
            MimeType::AV1 => MIME_TYPE_AV1,
            MimeType::OPUS => MIME_TYPE_OPUS,
            MimeType::G722 => MIME_TYPE_G722,
            MimeType::PCMU => MIME_TYPE_PCMU,
            MimeType::PCMA => MIME_TYPE_PCMA,
        };
        s.into()
    }
}

impl MimeType {
    pub fn from_str(s: &str) -> Result<Self> {
        let mime_type = match s {
            MIME_TYPE_H264 => MimeType::H264,
            MIME_TYPE_VP8 => MimeType::VP8,
            MIME_TYPE_VP9 => MimeType::VP9,
            MIME_TYPE_AV1 => MimeType::AV1,
            MIME_TYPE_OPUS => MimeType::OPUS,
            MIME_TYPE_G722 => MimeType::G722,
            MIME_TYPE_PCMU => MimeType::PCMU,
            MIME_TYPE_PCMA => MimeType::PCMA,
            _ => bail!(format! {"invalid mime type: {}", s}),
        };
        Ok(mime_type)
    }
}
