use anyhow::{bail, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::ice_transport::ice_candidate::{RTCIceCandidate, RTCIceCandidateInit};
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;

use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

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
/// signals and a channel for receiving signals.

pub struct Controller {
    api: webrtc::api::API,
    id: PeerId,
    /// store a PeerConnection for updating SDP and ICE candidates, adding and removing tracks
    peers: HashMap<PeerId, Peer>,
    // todo: store outgoing media tracks so that if local tracks are removed and re-added, the
    // remote tracks don't get lost
    // outgoing_media_tracks: HashMap<TrackKey, Arc<TrackLocalStaticRTP>>,
    /// used to control media_workers
    media_workers: HashMap<MediaSource, mpsc::UnboundedSender<()>>,
    /// used to emit events
    emitted_event_chan: mpsc::UnboundedSender<EmittedEvents>,
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
/// call_initiated
/// call_terminated
/// call_rejected
impl Controller {
    pub fn init(args: InitArgs) -> Result<Self> {
        Ok(Self {
            api: create_api()?,
            id: args.id,
            peers: HashMap::new(),
            media_workers: HashMap::new(),
            emitted_event_chan: args.emitted_event_chan,
        })
    }
    /// creates a RTCPeerConnection, sets the local SDP object, emits a CallInitiatedEvent,
    /// which contains the SDP object
    /// continues with the following signals: Sdp, CallTerminated, CallRejected
    pub fn dial(&self) {}
    /// adds the remote sdp, sets own sdp, and sends own sdp to remote
    pub fn accept_call(&self) {}
    /// Removes the RTCPeerConnection
    pub fn hang_up(&self) {}
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
    pub fn call_initiated(&self) {}
    pub fn call_terminated(&self) {}
    pub fn call_rejected(&self) {}

    /// adds a connection. called by dial and accept_call
    /// inserts the connection into self.peers
    async fn connect(&mut self, peer: PeerId) -> Result<()> {
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
                    id: peer,
                    connection: peer_connection,
                },
            )
            .is_some()
        {
            log::warn!("overwriting peer connection");
        }
        Ok(())
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
