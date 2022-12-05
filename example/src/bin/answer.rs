use anyhow::Result;
use clap::Parser;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Sample, SupportedStreamConfig,
};
use simple_webrtc::testing::*;
use simple_webrtc::{Controller, EmittedEvents, MimeType, RTCRtpCodecCapability};
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{
    mpsc::{self, error::TryRecvError},
    Mutex,
};
use tokio::time::sleep;
use webrtc::util::{Conn, Marshal, Unmarshal};
use webrtc::{
    media::io::sample_builder::SampleBuilder,
    rtp::{self, packetizer::Depacketizer},
    track::track_remote::TrackRemote,
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// the server address for this process
    local: String,
    /// the network address of the remote peer
    remote: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::new()
        .format(|buf, record| {
            writeln!(
                buf,
                "{}:{} [{}] {} - {}",
                record.file().unwrap_or("unknown"),
                record.line().unwrap_or(0),
                record.level(),
                chrono::Local::now().format("%H:%M:%S.%3f"),
                record.args()
            )
        })
        .filter(None, log::LevelFilter::Debug)
        .init();

    let cli = Cli::parse();

    // used to receive signals from the web server
    let (server_signal_tx, server_signal_rx) = mpsc::unbounded_channel::<PeerSignal>();

    // used to receive events from SimpleWebRTC
    let (client_event_tx, client_event_rx) = mpsc::unbounded_channel::<EmittedEvents>();

    // SimpleWebRTC instance
    let swrtc = simple_webrtc::Controller::init(simple_webrtc::InitArgs {
        id: cli.local.clone(),
        emitted_event_chan: client_event_tx,
    })?;
    let swrtc: Arc<Mutex<Controller>> = Arc::new(Mutex::new(swrtc));

    // hook up signaling
    set_signal_tx_chan(server_signal_tx).await;

    // create signaling server
    let signaling_server = signaling_server(&cli.local);

    tokio::select! {
        _ = signaling_server => {
             println!("signaling terminated");
        }
        _ = run(swrtc.clone(), cli.local.clone(), cli.remote.clone(), client_event_rx, server_signal_rx) => {
           println!( "swrtc terminated");
        }
         _ = tokio::signal::ctrl_c() => {
            println!("");
        }
    }

    {
        let mut s = swrtc.lock().await;
        s.deinit().await?;
    }

    Ok(())
}

async fn run(
    swrtc: Arc<Mutex<Controller>>,
    client_address: String,
    peer_address: String,
    client_event_rx: mpsc::UnboundedReceiver<EmittedEvents>,
    server_signal_rx: mpsc::UnboundedReceiver<PeerSignal>,
) {
    log::debug!("running answer");
    tokio::select! {
        r = handle_swrtc(client_address.clone(), peer_address.clone(), swrtc.clone()) => {
            println!("handle_swrtc terminated: {:?}", r);
        }
        r = handle_signals(client_address.clone(), peer_address.clone(), swrtc.clone(), server_signal_rx) => {
            println!("handle_signals terminated: {:?}", r);
        }
        r = handle_events(client_address.clone(), peer_address.clone(), swrtc.clone(), client_event_rx) => {
            println!("handle_events terminated: {:?}", r);
        }
    }
}

async fn handle_swrtc(
    _client_address: String,
    _peer_address: String,
    swrtc: Arc<Mutex<Controller>>,
) -> Result<()> {
    {
        let mut s = swrtc.lock().await;
        // a media source must be added before attempting to connect or SDP will fail
        s.add_media_source(
            "audio".into(),
            RTCRtpCodecCapability {
                mime_type: MimeType::OPUS.to_string(),
                ..Default::default()
            },
        )
        .await?;
    }

    loop {
        sleep(Duration::from_millis(1000)).await;
    }
}

async fn handle_signals(
    _client_address: String,
    _peer_address: String,
    swrtc: Arc<Mutex<Controller>>,
    mut server_signal_rx: mpsc::UnboundedReceiver<PeerSignal>,
) -> Result<()> {
    while let Some(sig) = server_signal_rx.recv().await {
        match sig {
            PeerSignal::Ice(sig) => {
                log::debug!("signal: ICE");
                let s = swrtc.lock().await;
                if let Err(e) = s.recv_ice(&sig.src, sig.ice).await {
                    log::error!("{}", e);
                }
            }
            PeerSignal::Sdp(sig) => {
                log::debug!("signal: SDP");
                let s = swrtc.lock().await;
                if let Err(e) = s.recv_sdp(&sig.src, sig.sdp).await {
                    log::error!("failed to recv_sdp: {}", e);
                }
            }
            PeerSignal::CallInitiated(sig) => {
                log::debug!("signal: CallInitiated");
                let mut s = swrtc.lock().await;

                if let Err(e) = s.accept_call(&sig.src, sig.sdp).await {
                    log::error!("failed to accept call: {}", e);
                    s.hang_up(&sig.src).await;
                    //send_disconnect(&sig.src, &client_address).await;
                }
            }
            PeerSignal::CallTerminated(src) => {
                log::debug!("signal: CallTerminated");
                let mut s = swrtc.lock().await;
                s.hang_up(&src).await;
            }
            PeerSignal::CallRejected(src) => {
                log::debug!("signal: CallRejected");
                let mut s = swrtc.lock().await;
                s.hang_up(&src).await;
            }
        }
    }
    Ok(())
}

async fn handle_events(
    client_address: String,
    _peer_address: String,
    swrtc: Arc<Mutex<Controller>>,
    mut client_event_rx: mpsc::UnboundedReceiver<EmittedEvents>,
) -> Result<()> {
    // want to send RTP packets to CPAL

    while let Some(evt) = client_event_rx.recv().await {
        match evt {
            EmittedEvents::CallInitiated { dest, sdp } => {
                log::debug!("event: CallInitiated");
                send_connect(
                    &dest,
                    SigSdp {
                        src: client_address.clone(),
                        sdp: *sdp,
                    },
                )
                .await?;
            }
            EmittedEvents::Sdp { dest, sdp } => {
                log::debug!("event: SDP");
                send_sdp(
                    &dest,
                    SigSdp {
                        src: client_address.clone(),
                        sdp: *sdp,
                    },
                )
                .await?;
            }
            EmittedEvents::Ice { dest, candidate } => {
                log::debug!("event: ICE");
                send_ice_candidate(
                    &dest,
                    SigIce {
                        src: client_address.clone(),
                        ice: *candidate,
                    },
                )
                .await?;
            }
            EmittedEvents::Disconnected { peer } => {
                log::debug!("event: Disconnected");
                let mut s = swrtc.lock().await;
                s.hang_up(&peer).await;
            }
            EmittedEvents::TrackAdded { peer, track } => {
                log::debug!("event: TrackAdded");

                // create a depacketizer based on the mime_type and pass it to a thread
                let mime_type = track.codec().await.capability.mime_type;
                match MimeType::from_string(&mime_type)? {
                    MimeType::OPUS => {
                        let depacketizer = webrtc::rtp::codecs::opus::OpusPacket::default();
                        // todo: get the clock rate (and possibly max_late) from the codec capability
                        let sample_builder = SampleBuilder::new(10, depacketizer, 7000);

                        tokio::spawn(async move {
                            if let Err(e) = decode_media_stream(track.clone(), sample_builder).await
                            {
                                log::error!("error decoding media stream: {}", e);
                            }
                        });
                    }
                    _ => {
                        log::error!("unhandled mime type: {}", &mime_type);
                        continue;
                    }
                };
            }
            _ => {}
        }
    }
    Ok(())
}

async fn decode_media_stream<T>(
    track: Arc<TrackRemote>,
    mut sample_builder: SampleBuilder<T>,
) -> Result<()>
where
    T: Depacketizer,
{
    let host = cpal::default_host();
    // todo: allow switching the output device during the call.
    let output_device: cpal::Device = host
        .default_output_device()
        .expect("couldn't find default output device");
    let config = output_device.default_output_config().unwrap();

    let (rtp_tx, mut rtp_rx) = mpsc::unbounded_channel::<webrtc::rtp::packet::Packet>();

    // read the raw data emitted by process_rtp
    let output_data_fn = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        let mut input_fell_behind = false;
        for sample in data {
            /*sample = match consumer.pop() {
                Some(s) => s,
                None => {
                    input_fell_behind = true;
                    0.0
                }
            };*/
        }
        if input_fell_behind {
            eprintln!("input stream fell behind: try increasing latency");
        }
    };

    let output_stream =
        output_device.build_output_stream(&config.into(), output_data_fn, err_fn)?;
    output_stream.play()?;

    let process_rtp = async move {
        while let Some(rtp_packet) = rtp_rx.recv().await {
            // turn RTP packets into samples via SampleBuilder.push
            sample_builder.push(rtp_packet);
            // check if a sample can be created
            while let Some(sample) = sample_builder.pop() {
                // todo: send the sample via a channel. possibly send it as a f32
                log::debug!("got sample");
            }
        }
    };

    let read_rtp = async move {
        // read RTP packets, convert to samples, and send samples via channel
        let mut b = [0u8; 1600];
        loop {
            match track.read(&mut b).await {
                Ok((siz, _attr)) => {
                    // get RTP packet
                    let mut buf = &b[..siz];
                    // todo: possibly continue on error.
                    let rtp_packet = match webrtc::rtp::packet::Packet::unmarshal(&mut buf) {
                        Ok(r) => r,
                        Err(_) => break,
                    };
                    // todo: set the payload_type
                    //rtp_packet.header.payload_type = ?;

                    // todo: send the RTP packet somewhere else if needed (such as something which is writing the media to an MP4 file)

                    if rtp_tx.send(rtp_packet).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    log::warn!("closing track: {}", e);
                    break;
                }
            }
        }
    };
    // todo: signal that an error occured
    tokio::select! {
        _ = process_rtp => {
            log::debug!("process_rtp stopped");
        }
        _ = read_rtp => {
            log::debug!("read_rtp stopped");
        }
    }

    Ok(())
}

fn err_fn(err: cpal::StreamError) {
    log::error!("an error occurred on stream: {}", err);
}
