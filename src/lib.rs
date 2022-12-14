use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
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

use webrtc::rtp_transceiver::rtp_receiver::RTCRtpReceiver;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;

use webrtc::track::track_remote::TrackRemote;

mod internal;

use crate::internal::data_types::*;

// public exports
pub mod media;
pub use internal::data_types::{MediaSourceId, MimeType, PeerId};
pub use internal::events::EmittedEvents;
pub use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::rtp_transceiver::rtp_sender::RTCRtpSender;

#[cfg(feature = "test-server")]
pub mod testing;

#[cfg(feature = "test-server")]
#[macro_use]
extern crate lazy_static;

/// simple-webrtc
/// This library augments the [webrtc-rs](https://github.com/webrtc-rs/webrtc) library, hopefully
/// simplifying the process of exchanging media with multiple peers simultaneously.
///
/// this library allows for the exchange of RTP packets. Transforming audio/video into RTP packets
/// is the user's responsibility. `webrtc-rs` provides a `rtp::packetizer` to turn raw samples into
/// RTP packets. `webrtc-rs` also provides a `media::io::sample_builder` to turn received RTP packets
/// into samples. `simple-webrtc` may expose these interfaces later.
///
/// The `add_media_source` function returns a `TrackLocalWriter`, with which the user will send
/// their RTP packets. Internally, a track is created and added to all existing and future peer
/// connections.. Writing a packet to the `TrackLocalWriter` will cause the packet to be forwarded
/// to all connected peers.
///
/// WebRTC requires out of band signalling. The `SimpleWebRtc` accepts a callback for transmitting
/// signals which must be forwarded to the specified peer
///

pub struct Controller {
    api: webrtc::api::API,
    /// client's id
    id: PeerId,
    /// list of peers
    peers: HashMap<PeerId, Peer>,
    /// used to emit events
    emitted_event_chan: mpsc::UnboundedSender<EmittedEvents>,
    /// attach these to every PeerConnection
    media_sources: HashMap<MediaSourceId, Arc<TrackLocalStaticRTP>>,
}

// a lazy version of the builder pattern
pub struct InitArgs {
    pub id: PeerId,
    pub emitted_event_chan: mpsc::UnboundedSender<EmittedEvents>,
}

/// stores a PeerConnection for updating SDP and ICE candidates, adding and removing tracks
/// also stores associated media streams
pub struct Peer {
    pub state: PeerState,
    pub id: PeerId,
    pub connection: Arc<RTCPeerConnection>,
    /// webrtc has a remove_track function which requires passing a RTCRtpSender
    /// to a RTCPeerConnection. this is created by add_track, though the user
    /// only receives a TrackWriter
    /// in the future, the RTCRtpSender can be used to have finer control over the stream.
    /// it can do things like pause the stream, without disconnecting it.
    pub rtp_senders: HashMap<MediaSourceId, Arc<RTCRtpSender>>,
}

/// The following functions are driven by the UI:
/// dial
/// accept_call
/// hang_up
/// add_media_source
/// remove_media_source
///
/// The following functions are driven by signaling
/// recv_ice
/// recv_sdp
impl Controller {
    pub fn init(args: InitArgs) -> Result<Self> {
        Ok(Self {
            api: create_api()?,
            id: args.id,
            peers: HashMap::new(),
            emitted_event_chan: args.emitted_event_chan,
            media_sources: HashMap::new(),
        })
    }
    /// Rust doesn't have async drop, so this function should be called when the user is
    /// done with Controller. it will clean up all threads
    pub async fn deinit(&mut self) -> Result<()> {
        let peer_ids: Vec<PeerId> = self.peers.keys().cloned().collect();
        for peer_id in peer_ids {
            self.hang_up(&peer_id).await;
        }

        Ok(())
    }
    /// creates a RTCPeerConnection, sets the local SDP object, emits a CallInitiatedEvent,
    /// which contains the SDP object
    /// continues with the following signals: Sdp, CallTerminated, CallRejected
    pub async fn dial(&mut self, peer_id: &PeerId) -> Result<()> {
        let pc = self.connect(peer_id).await?;
        let local_sdp = pc.create_offer(None).await?;
        // Sets the LocalDescription, and starts our UDP listeners
        // Note: this will start the gathering of ICE candidates
        pc.set_local_description(local_sdp.clone()).await?;

        self.emitted_event_chan.send(EmittedEvents::CallInitiated {
            dest: peer_id.clone(),
            sdp: Box::new(local_sdp),
        })?;

        Ok(())
    }
    /// adds the remote sdp, sets own sdp, and sends own sdp to remote
    pub async fn accept_call(
        &mut self,
        peer_id: &PeerId,
        remote_sdp: RTCSessionDescription,
    ) -> Result<()> {
        let pc = self
            .connect(peer_id)
            .await
            .context(format!("{}:{}", file!(), line!()))?;
        pc.set_remote_description(remote_sdp)
            .await
            .context(format!("{}:{}", file!(), line!()))?;

        let answer = pc
            .create_answer(None)
            .await
            .context(format!("{}:{}", file!(), line!()))?;
        pc.set_local_description(answer.clone())
            .await
            .context(format!("{}:{}", file!(), line!()))?;

        if let Some(p) = self.peers.get_mut(peer_id) {
            p.state = PeerState::WaitingForIce;
        } else {
            bail!("peer not found");
        }

        self.emitted_event_chan.send(EmittedEvents::Sdp {
            dest: peer_id.clone(),
            sdp: Box::new(answer),
        })?;

        Ok(())
    }
    /// Terminates a connection
    /// the controlling application should send a HangUp signal to the remote side
    pub async fn hang_up(&mut self, peer_id: &PeerId) {
        // not sure if it's necessary to remove all tracks
        if let Some(peer) = self.peers.get_mut(peer_id) {
            for (source_id, rtp_sender) in &peer.rtp_senders {
                // remove_track internally calls rtp_sender.stop(), which will stop the associated
                // thread
                if let Err(e) = peer.connection.remove_track(rtp_sender).await {
                    log::error!(
                        "failed to remove rtp_sender for source {} from peer {} on disconnect: {:?}",
                        &source_id,
                        &peer_id,
                        e
                    );
                }
            }
        }
        match self.peers.remove(peer_id) {
            Some(peer) => drop(peer),
            None => log::warn!("attempted to remove nonexistent peer"),
        }
    }

    /// Spawns a MediaWorker which will receive RTP packets and forward them to all peers
    /// todo: the peers may want to agree on the MimeType
    pub async fn add_media_source(
        &mut self,
        source_id: MediaSourceId,
        codec: RTCRtpCodecCapability,
    ) -> Result<Arc<TrackLocalStaticRTP>> {
        // todo: don't allow adding duplicate source_ids
        let track = Arc::new(TrackLocalStaticRTP::new(
            codec,
            source_id.clone(),
            self.id.clone(),
        ));
        // save this for later, for when connections are established to new peers
        self.media_sources.insert(source_id.clone(), track.clone());

        for (peer_id, peer) in &mut self.peers {
            match peer.connection.add_track(track.clone()).await {
                Ok(rtp_sender) => {
                    // returns None if the value was newly inserted.
                    if peer
                        .rtp_senders
                        .insert(source_id.clone(), rtp_sender.clone())
                        .is_some()
                    {
                        log::error!("duplicate rtp_sender");
                    } else {
                        // Read incoming RTCP packets
                        // Before these packets are returned they are processed by interceptors. For things
                        // like NACK this needs to be called.
                        tokio::spawn(async move {
                            let mut rtcp_buf = vec![0u8; 1500];
                            while let Ok((_, _)) = rtp_sender.read(&mut rtcp_buf).await {}
                            Result::<()>::Ok(())
                        });
                    }
                }
                Err(e) => {
                    log::error!(
                        "failed to add track for {} to peer {}: {:?}",
                        &source_id,
                        peer_id,
                        e
                    );
                }
            }
        }

        Ok(track)
    }
    /// Removes the media track
    /// ex: stop sharing screen
    /// the user should discard the TrackLocalWriter which they received from add_media_source
    pub async fn remove_media_source(&mut self, source_id: MediaSourceId) -> Result<()> {
        for (peer_id, peer) in &mut self.peers {
            // if source_id isn't found, it will be logged by the next statement
            if let Some(rtp_sender) = peer.rtp_senders.get(&source_id) {
                if let Err(e) = peer.connection.remove_track(rtp_sender).await {
                    log::error!(
                        "failed to remove track {} for peer {}: {:?}",
                        &source_id,
                        peer_id,
                        e
                    );
                }
            }

            if peer.rtp_senders.remove(&source_id).is_none() {
                log::warn!("media source {} not found for peer {}", &source_id, peer_id);
            }
        }

        if self.media_sources.remove(&source_id).is_none() {
            log::warn!(
                "media source {} not found in self.media_sources",
                &source_id
            );
        }
        Ok(())
    }

    /// receive an ICE candidate from the remote side
    pub async fn recv_ice(&self, peer_id: &PeerId, candidate: RTCIceCandidate) -> Result<()> {
        if let Some(peer) = self.peers.get(peer_id) {
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
    pub async fn recv_sdp(&self, peer_id: &PeerId, sdp: RTCSessionDescription) -> Result<()> {
        if let Some(peer) = self.peers.get(peer_id) {
            peer.connection.set_remote_description(sdp).await?;
        } else {
            bail!("peer not found");
        }

        Ok(())
    }

    /// adds a connection. called by dial and accept_call
    /// inserts the connection into self.peers
    /// initializes state to WaitingForSdp
    async fn connect(&mut self, peer_id: &PeerId) -> Result<Arc<RTCPeerConnection>> {
        // todo: ensure id is not in self.connections

        // create ICE gatherer
        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".into()],
                ..Default::default()
            }],
            ..Default::default()
        };

        // Create and store a new RTCPeerConnection
        let peer_connection = Arc::new(self.api.new_peer_connection(config).await?);
        if self
            .peers
            .insert(
                peer_id.clone(),
                Peer {
                    state: PeerState::WaitingForSdp,
                    id: peer_id.clone(),
                    connection: peer_connection.clone(),
                    rtp_senders: HashMap::new(),
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
        let dest = peer_id.clone();
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
        let dest = peer_id.clone();
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
        let dest = peer_id.clone();
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

        // attach all media sources to the peer
        let mut rtp_senders = HashMap::new();
        for (source_id, track) in &self.media_sources {
            match peer_connection.add_track(track.clone()).await {
                Ok(rtp_sender) => {
                    rtp_senders.insert(source_id.clone(), rtp_sender.clone());
                    // Read incoming RTCP packets
                    // Before these packets are returned they are processed by interceptors. For things
                    // like NACK this needs to be called.
                    tokio::spawn(async move {
                        let mut rtcp_buf = vec![0u8; 1500];
                        while let Ok((_, _)) = rtp_sender.read(&mut rtcp_buf).await {}
                        Result::<()>::Ok(())
                    });
                }
                Err(e) => {
                    log::error!(
                        "failed to add track for {} to peer {}: {:?}",
                        &source_id,
                        &peer_id,
                        e
                    );
                }
            }
        }
        match self.peers.get_mut(peer_id) {
            Some(p) => p.rtp_senders = rtp_senders,
            None => {
                log::error!(
                    "failed to set rtp senders when connecting to peer {}",
                    &peer_id
                );
            }
        }
        Ok(peer_connection)
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
