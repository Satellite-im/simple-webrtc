use anyhow::Result;
use clap::Parser;
use cpal::traits::HostTrait;
use simple_webrtc::testing::*;
use simple_webrtc::{Controller, EmittedEvents};
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{
    mpsc::{self},
    Mutex,
};
use tokio::time::sleep;

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
    _swrtc: Arc<Mutex<Controller>>,
) -> Result<()> {
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
            EmittedEvents::TrackAdded { peer: _, track } => {
                log::debug!("event: TrackAdded");
                let host = cpal::default_host();
                // todo: allow switching the output device during the call.
                let output_device: cpal::Device = host
                    .default_output_device()
                    .expect("couldn't find default output device");
                // create a depacketizer based on the mime_type and pass it to a thread
                let codec = track.codec().await.capability;
                let sink_track =
                    simple_webrtc::media::create_sink_track(output_device, track, codec)?;
                //simple_webrtc::media::OpusSink::init(output_device, track, codec)?;
                sink_track.play()?;
                sink_tracks.push(sink_track);
            }
        }
    }
    Ok(())
}
