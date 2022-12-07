use anyhow::{bail, Result};
use bytes::Bytes;
use opus::Channels;
use std::sync::Arc;
use tokio::sync::mpsc;
use webrtc::{
    media::io::sample_builder::SampleBuilder, rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_remote::TrackRemote,
};

pub struct OpusFramer {
    // encodes groups of samples (frames)
    encoder: opus::Encoder,
    // queues samples, to build a frame
    raw_samples: Vec<i16>,
    // used for the encoder
    opus_out: Vec<u8>,
    // number of samples in a frame
    frame_size: usize,
}

impl OpusFramer {
    pub fn init(frame_size: usize, sample_rate: u32, channels: opus::Channels) -> Result<Self> {
        let mut buf = Vec::new();
        buf.reserve(frame_size as usize);
        let mut opus_out = Vec::new();
        opus_out.resize(frame_size, 0);
        let encoder = opus::Encoder::new(sample_rate, channels, opus::Application::Voip)?;

        Ok(Self {
            encoder,
            raw_samples: buf,
            opus_out,
            frame_size,
        })
    }

    pub fn frame(&mut self, sample: i16) -> Option<Bytes> {
        self.raw_samples.push(sample);
        if self.raw_samples.len() == self.frame_size {
            match self.encoder.encode(
                self.raw_samples.as_mut_slice(),
                self.opus_out.as_mut_slice(),
            ) {
                Ok(size) => {
                    self.raw_samples.clear();
                    let slice = self.opus_out.as_slice();
                    let bytes = bytes::Bytes::copy_from_slice(&slice[0..size]);
                    Some(bytes)
                }
                Err(e) => {
                    log::error!("OpusPacketizer failed to encode: {}", e);
                    None
                }
            }
        } else {
            None
        }
    }
}
