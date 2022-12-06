use std::sync::Arc;

use anyhow::Result;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    PlayStreamError,
};
use rand::prelude::*;
use simple_webrtc::PeerId;
use tokio::sync::mpsc::{self, error::TryRecvError};
use webrtc::rtp::{self};
use webrtc::track::track_local::TrackLocalWriter;

pub struct OpusPacketizer {
    // encodes groups of samples (frames)
    encoder: opus::Encoder,
    // queues samples, to build a frame
    raw_samples: Vec<i16>,
    // used for the encoder
    opus_out: Vec<u8>,
    // number of samples in a frame
    frame_size: u32,
    packetizer: Box<dyn rtp::packetizer::Packetizer>,
}

impl OpusPacketizer {
    pub fn init(frame_size: u32, sample_rate: u32, channels: opus::Channels) -> Result<Self> {
        let mut rng = rand::thread_rng();
        let ssrc: u32 = rng.gen();

        let mut buf = Vec::new();
        buf.reserve(frame_size as usize);
        let mut opus_out = Vec::new();
        opus_out.resize(frame_size as usize, 0);
        let encoder = opus::Encoder::new(sample_rate, channels, opus::Application::Voip)?;
        let opus = Box::new(rtp::codecs::opus::OpusPayloader {});
        let seq = Box::new(rtp::sequence::new_random_sequencer());

        let packetizer = rtp::packetizer::new_packetizer(
            // i16 is 2 bytes
            (frame_size * 2) as usize,
            // payload type means nothing
            // https://en.wikipedia.org/wiki/RTP_payload_formats
            98,
            // randomly generated and uniquely identifies the source
            ssrc,
            opus,
            seq,
            sample_rate,
        );
        Ok(Self {
            encoder,
            raw_samples: buf,
            opus_out,
            frame_size,
            packetizer: Box::new(packetizer),
        })
    }

    pub async fn encode(&mut self, sample: i16) -> Vec<rtp::packet::Packet> {
        self.raw_samples.push(sample);
        if self.raw_samples.len() == self.frame_size as usize {
            match self.encoder.encode(
                self.raw_samples.as_mut_slice(),
                self.opus_out.as_mut_slice(),
            ) {
                Ok(size) => {
                    self.raw_samples.clear();
                    let bytes = bytes::Bytes::copy_from_slice(self.opus_out.as_slice());
                    match self.packetizer.packetize(&bytes, size as u32).await {
                        Ok(packets) => packets,
                        Err(e) => {
                            log::error!("OpusPacketizer failed to packetize: {}", e);
                            vec![]
                        }
                    }
                }
                Err(e) => {
                    log::error!("OpusPacketizer failed to encode: {}", e);
                    vec![]
                }
            }
        } else {
            vec![]
        }
    }
}

pub struct SourceTrack {
    device: cpal::Device,
    stream: cpal::Stream,
    track: Arc<dyn TrackLocalWriter>,
}

impl SourceTrack {
    pub fn init(
        track: Arc<dyn TrackLocalWriter>,
        sample_rate: u32,
        channels: opus::Channels,
    ) -> Result<Self> {
        let (producer, mut consumer) = mpsc::unbounded_channel::<i16>();
        let mut packetizer = OpusPacketizer::init(120, sample_rate, channels)?;

        tokio::spawn(async move { while let Some(sample) = consumer.recv().await {} });
        let input_data_fn = move |data: &[i16], _: &cpal::InputCallbackInfo| {
            for sample in data {
                if let Err(e) = producer.send(*sample) {
                    log::error!("SourceTrack failed to send sample: {}", e);
                }
            }
        };

        let host = cpal::default_host();
        // todo: allow switching the input device during the call.
        let input_device: cpal::Device = host
            .default_input_device()
            .expect("couldn't find default input device");
        let config = input_device.default_input_config().unwrap();
        let input_stream =
            input_device.build_input_stream(&config.into(), input_data_fn, err_fn)?;

        Ok(Self {
            track,
            device: input_device,
            stream: input_stream,
        })
    }
}

pub struct SinkTrack {
    peer_id: PeerId,
    device: cpal::Device,
    stream: cpal::Stream,
}

// todo: sample rate?
impl SinkTrack {
    // should receive raw samples from `consumer`
    pub fn init(peer_id: PeerId, mut consumer: mpsc::UnboundedReceiver<i16>) -> Result<Self> {
        let output_data_fn = move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
            let mut input_fell_behind = false;
            for sample in data {
                *sample = match consumer.try_recv() {
                    Ok(s) => s,
                    Err(TryRecvError::Empty) => {
                        input_fell_behind = true;
                        0
                    }
                    Err(e) => {
                        log::error!("channel closed: {}", e);
                        0
                    }
                }
            }
            if input_fell_behind {
                log::error!("input stream fell behind: try increasing latency");
            }
        };

        let host = cpal::default_host();
        // todo: allow switching the output device during the call.
        let output_device: cpal::Device = host
            .default_output_device()
            .expect("couldn't find default output device");
        let config = output_device.default_output_config().unwrap();
        let output_stream =
            output_device.build_output_stream(&config.into(), output_data_fn, err_fn)?;

        Ok(Self {
            peer_id,
            device: output_device,
            stream: output_stream,
        })
    }

    pub fn play(&self) -> Result<()> {
        if let Err(e) = self.stream.play() {
            return Err(e.into());
        }
        Ok(())
    }

    pub fn get_device(&self) -> &cpal::Device {
        &self.device
    }

    pub fn get_peer_id(&self) -> PeerId {
        self.peer_id.clone()
    }
}

fn err_fn(err: cpal::StreamError) {
    log::error!("an error occurred on stream: {}", err);
}
