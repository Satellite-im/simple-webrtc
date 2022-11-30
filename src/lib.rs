use anyhow::{bail, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc};
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::ice_transport::ice_candidate::{RTCIceCandidate, RTCIceCandidateInit};
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::RTCPeerConnection;

use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp;
use webrtc::rtp_transceiver::rtp_receiver::RTCRtpReceiver;

use webrtc::track::track_remote::TrackRemote;

mod internal;

use crate::internal::data_types::*;
use crate::internal::events::*;

// public exports
pub use internal::media::*;

#[cfg(feature = "test-server")]
mod testing;
#[cfg(feature = "test-server")]
pub use testing::signaling_server;

#[cfg(feature = "test-server")]
#[macro_use]
extern crate lazy_static;

/// simple-webrtc
/// This library augments the [webrtc-rs](https://github.com/webrtc-rs/webrtc) library, hopefully
/// simplifying the process of exchanging media with multiple peers simultaneously.
///
/// this library allows for the exchange of RTP packets. Transforming audio/video into RTP packets
/// is the user's responsibility.
///
/// WebRTC requires out of band signalling. The `SimpleWebRtc` accepts a callback for transmitting
/// signals which must be forwarded to the specified peer
///
/// This library is not responsible for Media capture. Initializing a `Controller` results in
/// the Controller struct and a set of channels which shall be used to transmit RTP packets
/// over WebRTC.

pub struct Controller {
    api: webrtc::api::API,
    /// client's id
    id: PeerId,
    /// list of peers
    peers: HashMap<PeerId, Peer>,
    /// used to emit events
    emitted_event_chan: mpsc::UnboundedSender<EmittedEvents>,
    /// used to control the threads which receive RTP packets from the user
    media_worker_channels: MediaWorkerChannels,
}

// a lazy version of the builder pattern
pub struct InitArgs {
    pub id: PeerId,
    pub emitted_event_chan: mpsc::UnboundedSender<EmittedEvents>,
}

/// The following functions are driven by the UI:
/// dial
/// accept_call
/// hang_up
/// add_track
/// remove_track
///
/// The following functions are driven by signaling
/// recv_ice
/// recv_sdp
impl Controller {
    // hey look it's a 70+ line constructor
    pub fn init(args: InitArgs) -> Result<(Self, MediaWorkerInputs)> {
        // create channels used to exchange RTP packets
        let (camera_tx, camera_rx) = mpsc::unbounded_channel::<rtp::packet::Packet>();
        let (microphone_tx, microphone_rx) = mpsc::unbounded_channel::<rtp::packet::Packet>();
        let (screen_tx, screen_rx) = mpsc::unbounded_channel::<rtp::packet::Packet>();

        // this will be returned to the user
        let media_worker_inputs = MediaWorkerInputs {
            camera_tx,
            microphone_tx,
            screen_tx,
        };

        // create channels to control the media workers
        let (camera_worker_tx, camera_worker_rx) = mpsc::unbounded_channel::<MediaWorkerCommands>();
        let (microphone_worker_tx, microphone_worker_rx) =
            mpsc::unbounded_channel::<MediaWorkerCommands>();
        let (screen_worker_tx, screen_worker_rx) = mpsc::unbounded_channel::<MediaWorkerCommands>();

        let media_worker_channels = MediaWorkerChannels {
            camera: camera_worker_tx,
            microphone: microphone_worker_tx,
            screen: screen_worker_tx,
        };

        // spawn media workers
        tokio::spawn(async move {
            let mut worker = MediaWorker {
                control_rx: camera_worker_rx,
                media_rx: camera_rx,
                outgoing_media_tracks: HashMap::new(),
            };
            worker.run().await;
        });

        tokio::spawn(async move {
            let mut worker = MediaWorker {
                control_rx: microphone_worker_rx,
                media_rx: microphone_rx,
                outgoing_media_tracks: HashMap::new(),
            };
            worker.run().await;
        });

        tokio::spawn(async move {
            let mut worker = MediaWorker {
                control_rx: screen_worker_rx,
                media_rx: screen_rx,
                outgoing_media_tracks: HashMap::new(),
            };
            worker.run().await;
        });

        Ok((
            Self {
                api: create_api()?,
                id: args.id,
                peers: HashMap::new(),
                media_worker_channels,
                emitted_event_chan: args.emitted_event_chan,
            },
            media_worker_inputs,
        ))
    }
    /// creates a RTCPeerConnection, sets the local SDP object, emits a CallInitiatedEvent,
    /// which contains the SDP object
    /// continues with the following signals: Sdp, CallTerminated, CallRejected
    pub async fn dial(&mut self, peer: PeerId) -> Result<()> {
        let pc = self.connect(peer.clone()).await?;
        let local_sdp = pc.create_offer(None).await?;
        // Sets the LocalDescription, and starts our UDP listeners
        // Note: this will start the gathering of ICE candidates
        pc.set_local_description(local_sdp.clone()).await?;

        self.emitted_event_chan.send(EmittedEvents::Sdp {
            dest: peer,
            sdp: Box::new(local_sdp),
        })?;

        Ok(())
    }
    /// adds the remote sdp, sets own sdp, and sends own sdp to remote
    pub async fn accept_call(
        &mut self,
        peer: PeerId,
        remote_sdp: RTCSessionDescription,
    ) -> Result<()> {
        let pc = self.connect(peer.clone()).await?;
        if let Err(e) = pc.set_remote_description(remote_sdp).await {
            log::error!("failed to set remote description: {:?}", e);
            return Err(e.into());
        }

        let answer = pc.create_answer(None).await?;
        pc.set_local_description(answer.clone()).await?;

        if let Some(p) = self.peers.get_mut(&peer) {
            p.state = PeerState::WaitingForIce;
        } else {
            bail!("peer not found");
        }

        self.emitted_event_chan.send(EmittedEvents::Sdp {
            dest: peer,
            sdp: Box::new(answer),
        })?;

        Ok(())
    }
    /// Removes the RTCPeerConnection
    /// the controlling application sould send a HangUp signal to the remote side
    pub fn hang_up(&mut self, peer: PeerId) {
        // todo: tell MediaWorker to drop channels associated with Peer

        if self.peers.remove(&peer).is_none() {
            log::info!("called hang_up for non-connected peer");
        }
    }
    /// spawns a MediaWorker to capture media and write to registered TrackLocalStaticRTP
    /// adds tracks to the RTCPeerConnection and sends them to the MediaWorker
    pub fn add_track(&self) {}
    /// removes tracks from the RTCPeerConnection and closes the MediaWorker
    pub fn remove_track(&self) {}
    /// receive an ICE candidate from the remote side
    pub async fn recv_ice(&self, peer: PeerId, candidate: RTCIceCandidate) -> Result<()> {
        if let Some(peer) = self.peers.get(&peer) {
            let candidate = candidate.to_json()?.candidate;
            peer.connection
                .add_ice_candidate(RTCIceCandidateInit {
                    candidate,
                    ..Default::default()
                })
                .await?;
        } else {
            bail!("peer not found");
        }

        Ok(())
    }
    /// receive an SDP object from the remote side
    pub async fn recv_sdp(&self, peer: PeerId, sdp: RTCSessionDescription) -> Result<()> {
        if let Some(peer) = self.peers.get(&peer) {
            peer.connection.set_remote_description(sdp).await?;
        } else {
            bail!("peer not found");
        }

        Ok(())
    }

    /// adds a connection. called by dial and accept_call
    /// inserts the connection into self.peers
    /// initializes state to WaitingForSdp
    async fn connect(&mut self, peer: PeerId) -> Result<Arc<RTCPeerConnection>> {
        // todo: ensure id is not in self.connections

        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec![
                    "stun:stun.services.mozilla.com:3478".into(),
                    "stun:stun.l.google.com:19302".into(),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };

        // Create a new RTCPeerConnection
        let peer_connection = Arc::new(self.api.new_peer_connection(config).await?);
        if self
            .peers
            .insert(
                peer.clone(),
                Peer {
                    state: PeerState::WaitingForSdp,
                    id: peer.clone(),
                    connection: peer_connection.clone(),
                    tracks: HashMap::new(),
                },
            )
            .is_some()
        {
            log::warn!("overwriting peer connection");
        }

        // configure callbacks

        // send discovered ice candidates (for self) to remote peer
        // the next 2 lines is some nonsense to satisfy the (otherwise excellent) rust compiler
        let tx = self.emitted_event_chan.clone();
        let dest = peer.clone();
        peer_connection.on_ice_candidate(Box::new(move |c: Option<RTCIceCandidate>| {
            let tx = tx.clone();
            let dest = dest.clone();
            Box::pin(async move {
                if let Some(candidate) = c {
                    if let Err(e) = tx.send(EmittedEvents::Ice {
                        dest: dest.clone(),
                        candidate: Box::new(candidate),
                    }) {
                        log::error!("failed to send ice candidate to peer {}: {}", &dest, e);
                    }
                }
            })
        }));

        // Set the handler for ICE connection state
        // This will notify you when the peer has connected/disconnected
        // the next 2 lines is some nonsense to satisfy the (otherwise excellent) rust compiler
        let tx = self.emitted_event_chan.clone();
        let dest = peer.clone();
        peer_connection.on_ice_connection_state_change(Box::new(
            move |connection_state: RTCIceConnectionState| {
                let tx = tx.clone();
                let dest = dest.clone();
                log::info!(
                    "Connection State for peer {} has changed {}",
                    &dest,
                    connection_state
                );
                if connection_state == RTCIceConnectionState::Failed {
                    if let Err(e) = tx.send(EmittedEvents::Disconnected { peer: dest.clone() }) {
                        log::error!("failed to send disconnect event for peer {}: {}", &dest, e);
                    }
                }
                Box::pin(async {})
            },
        ));

        // store media tracks when created
        // the next 2 lines is some nonsense to satisfy the (otherwise excellent) rust compiler
        let tx = self.emitted_event_chan.clone();
        let dest = peer.clone();
        peer_connection.on_track(Box::new(
            move |track: Option<Arc<TrackRemote>>, _receiver: Option<Arc<RTCRtpReceiver>>| {
                let tx = tx.clone();
                let dest = dest.clone();
                if let Some(track) = track {
                    if let Err(e) = tx.send(EmittedEvents::TrackAdded {
                        peer: dest.clone(),
                        track,
                    }) {
                        log::error!("failed to send track added event for peer {}: {}", &dest, e);
                    }
                }
                Box::pin(async {})
            },
        ));

        Ok(peer_connection)
    }

    /// terminates a connection
    async fn disconnect(&mut self, peer: PeerId) -> Result<()> {
        // todo: verify that the peer id exists in self.connections
        if self.peers.remove(&peer).is_none() {
            log::warn!("attempted to remove nonexistent peer")
        }
        Ok(())
    }
}

// todo: add support for more codecs. perhaps make it configurable
fn create_api() -> Result<webrtc::api::API> {
    let mut media = MediaEngine::default();
    media.register_default_codecs()?;

    // Create a InterceptorRegistry. This is the user configurable RTP/RTCP Pipeline.
    // This provides NACKs, RTCP Reports and other features. If you use `webrtc.NewPeerConnection`
    // this is enabled by default. If you are manually managing You MUST create a InterceptorRegistry
    // for each PeerConnection.
    let mut registry = Registry::new();

    // Use the default set of Interceptors
    registry = register_default_interceptors(registry, &mut media)?;

    // Create the API object with the MediaEngine
    Ok(APIBuilder::new()
        .with_media_engine(media)
        .with_interceptor_registry(registry)
        .build())
}
