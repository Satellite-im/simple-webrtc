use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::interceptor::registry::Registry;

use crate::MimeType;
use webrtc::ice_transport::ice_candidate::{RTCIceCandidate, RTCIceCandidateInit};
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp_transceiver::rtp_receiver::RTCRtpReceiver;
use webrtc::{
    peer_connection::RTCPeerConnection,
    rtp,
    track::{track_local::track_local_static_rtp::TrackLocalStaticRTP, track_remote::TrackRemote},
};

/// simple-webrtc-internal

/// Indicates the device from which the media stream originates
#[derive(Serialize, Deserialize)]
pub enum MediaSource {
    /// audio destination
    Microphone,
    /// video destination
    Camera,
    /// video destination
    Screen,
}

/// Used to initiate sending audio/video to a peer
#[derive(Serialize, Deserialize)]
pub struct TrackDescription {
    track_id: String,
    media_source: MediaSource,
    mime_type: MimeType,
    peer_id: PeerId,
}

/// Used to configure and control the WebRTC connection
/// These signals are exchanged with the peer
pub enum PeerSignal {
    /// Initiates a connection
    /// the client will create a RTCPeerConnection and associate it with the peer ID
    /// the client will create a SDP object and set the local SDP for the associated connection
    /// the client will send this SDP object with the connect request
    Connect {
        peer_id: PeerId,
        sdp: RTCSessionDescription,
    },
    /// rejects a connection
    /// if a client's "connect" signal is rejected, the corresponding RTCPeerConnection is
    /// deleted
    Reject { peer_id: PeerId },
    /// terminates a connection
    Disconnect { peer_id: PeerId },
    /// Shares an ICE candidate
    /// Requried by webrtc-rs
    IceCandidate {
        peer_id: PeerId,
        candidate: RTCIceCandidate,
    },
    /// Sends the Session Description Protocol object
    Sdp {
        peer_id: PeerId,
        sdp: RTCSessionDescription,
    },
    /// Add a media track (audio or video)
    AddTrack { description: TrackDescription },
    /// remove a media track
    RemoveTrack { peer_id: PeerId, track_id: String },
}

/// Used by the client to communicate with the controlling process
pub enum EventSignal {
    /// triggered when the client receives an ICE candidate during ICE discovery.
    /// This candidate needs to be sent to the peer
    IceCandidate {
        candidate: RTCIceCandidate,
        dest_peer: PeerId,
    },
    /// reply with Client's SDP. used when accepting a connection
    Sdp {
        sdp: RTCSessionDescription,
        dest_peer: PeerId,
    },
    /// triggered when ICE discovery fails or if the connection is dropped or terminated.
    /// if a Disconnect signal wasn't received, the client should attempt to reconnect, starting
    /// with ICE discovery
    ConnectionFailed { dest_peer: PeerId },
}

// the keys from outgoing_media_tracks correspond to the keys from local_media_tracks
pub struct SimpleWebRtc {
    api: webrtc::api::API,
    client_id: PeerId,
    /// used by the client to communicate with the controlling process
    event_signal_chan: mpsc::UnboundedSender<EventSignal>,
    /// receives incoming signals
    incoming_signal_chan: mpsc::UnboundedReceiver<PeerSignal>,
    /// when the remote side creates a track (for sending), pass it here
    incoming_media_chan: mpsc::UnboundedSender<TrackOpened>,
    /// peer ID, connection
    connections: HashMap<PeerId, Arc<RTCPeerConnection>>,
    /// used to transmit audio/video to every peer
    outgoing_media_tracks: HashMap<TrackKey, Arc<TrackLocalStaticRTP>>,
    /// Allows the client to feed RTP packets to all peers
    local_media_tracks: HashMap<TrackKey, broadcast::Receiver<rtp::packet::Packet>>,
}

// todo: possibly make this a trait
pub type PeerId = String;

// don't want track names to collide - combine with the peer_id
pub struct TrackKey {
    pub peer_id: PeerId,
    pub track_name: String,
}

pub struct TrackOpened {
    pub track: Option<Arc<TrackRemote>>,
    pub info: Option<Arc<RTCRtpReceiver>>,
}

pub struct SimpleWebRtcInit {
    pub client_id: PeerId,
    pub incoming_media_chan: mpsc::UnboundedSender<TrackOpened>,
    pub event_signal_chan: mpsc::UnboundedSender<EventSignal>,
    pub incoming_signal_chan: mpsc::UnboundedReceiver<PeerSignal>,
}

impl SimpleWebRtc {
    pub fn init(args: SimpleWebRtcInit) -> Result<Self> {
        Ok(Self {
            api: Self::create_api()?,
            client_id: args.client_id,
            event_signal_chan: args.event_signal_chan,
            incoming_signal_chan: args.incoming_signal_chan,
            incoming_media_chan: args.incoming_media_chan,
            connections: HashMap::new(),
            outgoing_media_tracks: HashMap::new(),
            local_media_tracks: HashMap::new(),
        })
    }

    /// Initiates a connection
    pub async fn connect(&mut self, peer: &PeerId) -> Result<()> {
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
            .connections
            .insert(peer.clone(), peer_connection)
            .is_some()
        {
            log::warn!("overwriting peer connection");
        }
        Ok(())
    }
    /// terminates a connection
    pub async fn disconnect(&mut self, peer: &PeerId) -> Result<()> {
        // todo: verify that the peer id exists in self.connections
        if self.connections.remove(peer).is_none() {
            log::warn!("attempted to remove nonexistent peer")
        }
        Ok(())
    }

    /// Tells the client of a possible address which may be used to connect to the remote side
    pub async fn recv_ice_candidate(
        &self,
        peer: &PeerId,
        candidate: RTCIceCandidate,
    ) -> Result<()> {
        if let Some(pc) = self.connections.get(peer) {
            let candidate = candidate.to_json()?.candidate;
            pc.add_ice_candidate(RTCIceCandidateInit {
                candidate,
                ..Default::default()
            })
            .await?;
        } else {
            bail!("peer not found");
        }

        Ok(())
    }

    /// pass the sdp of the remote side to the client
    pub async fn recv_sdp(&self, peer: &PeerId, sdp: RTCSessionDescription) -> Result<()> {
        if let Some(pc) = self.connections.get(peer) {
            pc.set_remote_description(sdp).await?;
        } else {
            bail!("peer not found");
        }

        Ok(())
    }

    /// Add a media track (audio or video)
    /// Registers a media source, stored in `local_media_tracks`
    /// when a peer connects, a corresponding connection is automatically created in
    /// `outgoing_media_tracks`.
    pub async fn add_track(
        &mut self,
        _id: &str,
        _source: broadcast::Receiver<rtp::packet::Packet>,
    ) {
        todo!()
    }
    /// remove a media track
    pub async fn remove_track(&mut self, _id: &str) {
        todo!()
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
}
