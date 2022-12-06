use anyhow::Result;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    PlayStreamError,
};
use simple_webrtc::PeerId;
use tokio::sync::mpsc::{self, error::TryRecvError};

pub struct OutputTrack {
    peer_id: PeerId,
    device: cpal::Device,
    stream: cpal::Stream,
}

impl OutputTrack {
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
