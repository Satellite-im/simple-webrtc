use anyhow::{bail, Result};
use std::sync::Arc;
use webrtc::{rtp_transceiver::rtp_codec::RTCRtpCodecCapability, track::track_remote::TrackRemote};

use crate::MimeType;
mod opus_sink;
pub use opus_sink::OpusSink;

pub trait SourceTrack {}

pub trait SinkTrack {
    fn init(
        output_device: cpal::Device,
        track: Arc<TrackRemote>,
        codec: RTCRtpCodecCapability,
    ) -> Result<Self>
    where
        Self: Sized;
    fn play(&self);
    fn change_output_device(&mut self, output_device: cpal::Device);
}

pub fn create_sink_track(
    output_device: cpal::Device,
    track: Arc<TrackRemote>,
    codec: RTCRtpCodecCapability,
) -> Result<Box<dyn SinkTrack>> {
    match MimeType::from_string(&codec.mime_type)? {
        MimeType::OPUS => Ok(Box::new(OpusSink::init(output_device, track, codec)?)),
        _ => {
            bail!("unhandled mime type: {}", &codec.mime_type);
        }
    }
}
