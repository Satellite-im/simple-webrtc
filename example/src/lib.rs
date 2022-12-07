use std::sync::Arc;

use anyhow::Result;
use bytes::Bytes;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rand::prelude::*;
use simple_webrtc::PeerId;
use tokio::sync::mpsc::{self, error::TryRecvError};
use webrtc::{
    media::io::sample_builder::SampleBuilder,
    rtp::{
        self,
        packetizer::{Depacketizer, Packetizer},
    },
    track::{
        track_local::{track_local_static_rtp::TrackLocalStaticRTP, TrackLocalWriter},
        track_remote::TrackRemote,
    },
    util::Unmarshal,
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

pub struct SourceTrack {
    device: cpal::Device,
    stream: cpal::Stream,
    track: Arc<TrackLocalStaticRTP>,
}

impl SourceTrack {
    pub fn init(
        track: Arc<TrackLocalStaticRTP>,
        sample_rate: u32,
        channels: opus::Channels,
    ) -> Result<Self> {
        let (producer, mut consumer) = mpsc::unbounded_channel::<Bytes>();
        let frame_size = 120;
        let mut rng = rand::thread_rng();
        let ssrc: u32 = rng.gen();
        let mut framer = OpusFramer::init(frame_size, sample_rate, channels)?;
        let opus = Box::new(rtp::codecs::opus::OpusPayloader {});
        let seq = Box::new(rtp::sequence::new_random_sequencer());

        let mut packetizer = rtp::packetizer::new_packetizer(
            // i16 is 2 bytes
            // frame size is number of i16 samles
            // 12 is for the header, though there may be an additional 4*csrc bytes in the header.
            (frame_size * 2 + 12) as usize,
            // payload type means nothing
            // https://en.wikipedia.org/wiki/RTP_payload_formats
            98,
            // randomly generated and uniquely identifies the source
            ssrc,
            opus,
            seq,
            sample_rate,
        );

        let track2 = track.clone();
        tokio::spawn(async move {
            while let Some(bytes) = consumer.recv().await {
                // todo: figure out how many samples were actually created
                match packetizer.packetize(&bytes, frame_size as u32).await {
                    Ok(packets) => {
                        for packet in &packets {
                            if let Err(e) = track2.write_rtp(packet).await {
                                log::error!("failed to send RTP packet: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("failed to packetize for opus: {}", e);
                    }
                }
            }
            log::debug!("SourceTrack packetizer thread quitting");
        });
        let input_data_fn = move |data: &[i16], _: &cpal::InputCallbackInfo| {
            for sample in data {
                if let Some(bytes) = framer.frame(*sample) {
                    if let Err(e) = producer.send(bytes) {
                        log::error!("SourceTrack failed to send sample: {}", e);
                    }
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

    pub fn play(&self) -> Result<()> {
        if let Err(e) = self.stream.play() {
            return Err(e.into());
        }
        Ok(())
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
    pub fn init(track: Arc<TrackRemote>, peer_id: PeerId, sample_rate: u32) -> Result<Self> {
        // number of late samples allowed
        let max_late = 480;
        let (producer, mut consumer) = mpsc::unbounded_channel::<i16>();
        let depacketizer = webrtc::rtp::codecs::opus::OpusPacket::default();
        let sample_builder = SampleBuilder::new(max_late, depacketizer, sample_rate as u32);

        tokio::spawn(async move {
            if let Err(e) =
                decode_media_stream(track.clone(), sample_builder, producer, sample_rate).await
            {
                log::error!("error decoding media stream: {}", e);
            }
            log::debug!("stopping decode_media_stream thread");
        });

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

// todo: put this in a different file
async fn decode_media_stream<T>(
    track: Arc<TrackRemote>,
    mut sample_builder: SampleBuilder<T>,
    producer: mpsc::UnboundedSender<i16>,
    sample_rate: u32,
) -> Result<()>
where
    T: Depacketizer,
{
    let mut decoder = opus::Decoder::new(sample_rate, opus::Channels::Mono)?;
    let mut decoder_output_buf = [0; 4096];
    // read RTP packets, convert to samples, and send samples via channel
    let mut b = [0u8; 4096];
    loop {
        match track.read(&mut b).await {
            Ok((siz, _attr)) => {
                // get RTP packet
                let mut buf = &b[..siz];
                // todo: possibly continue on error.
                let rtp_packet = match webrtc::rtp::packet::Packet::unmarshal(&mut buf) {
                    Ok(r) => r,
                    Err(e) => {
                        log::error!("unmarshall rtp packet failed: {}", e);
                        break;
                    }
                };
                // todo: set the payload_type
                //rtp_packet.header.payload_type = ?;

                // todo: send the RTP packet somewhere else if needed (such as something which is writing the media to an MP4 file)

                // turn RTP packets into samples via SampleBuilder.push
                sample_builder.push(rtp_packet);
                // check if a sample can be created
                while let Some(media_sample) = sample_builder.pop() {
                    match decoder.decode(media_sample.data.as_ref(), &mut decoder_output_buf, false)
                    {
                        Ok(siz) => {
                            let to_send = decoder_output_buf.iter().take(siz);
                            for audio_sample in to_send {
                                if let Err(e) = producer.send(*audio_sample) {
                                    log::error!("failed to send sample: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("decode error: {}", e);
                            continue;
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("closing track: {}", e);
                break;
            }
        }
    }

    Ok(())
}
