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
use webrtc::{
    media::io::sample_builder::SampleBuilder,
    rtp::{self, packetizer::Depacketizer},
    track::track_remote::TrackRemote,
};
use webrtc::{
    track::track_local::track_local_static_rtp::TrackLocalStaticRTP,
    util::{Conn, Marshal, Unmarshal},
};

use example::*;

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
    /*let sample_rate = 48000;
    let channels = opus::Channels::Mono;
    // get a track to send audio
    let track = {
        let mut s = swrtc.lock().await;
        // a media source must be added before attempting to connect or SDP will fail
        s.add_media_source(
            "audio".into(),
            RTCRtpCodecCapability {
                mime_type: MimeType::OPUS.to_string(),
                clock_rate: sample_rate.clone(),
                channels: channels.clone() as u16,
                ..Default::default()
            },
        )
        .await?
    };

    // create an audio source
    let source_track = SourceTrack::init(track, sample_rate, channels)?;
    source_track.play()?;*/

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

    let mut sink_tracks = vec![];

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
                        let (producer, consumer) = mpsc::unbounded_channel::<i16>();
                        let sink_track = SinkTrack::init(peer.clone(), consumer)?;
                        sink_track.play()?;
                        sink_tracks.push(sink_track);

                        let depacketizer = webrtc::rtp::codecs::opus::OpusPacket::default();
                        // todo: get the clock rate (and possibly max_late) from the codec capability
                        let sample_rate = 48000;
                        let sample_builder =
                            SampleBuilder::new(480, depacketizer, sample_rate as u32);

                        tokio::spawn(async move {
                            if let Err(e) = decode_media_stream(
                                track.clone(),
                                sample_builder,
                                producer,
                                sample_rate,
                            )
                            .await
                            {
                                log::error!("error decoding media stream: {}", e);
                            }
                            log::debug!("stopping decode_media_stream thread");
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
