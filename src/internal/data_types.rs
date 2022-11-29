use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;

/// uniquely identifies peers
pub type PeerId = String;

// don't want track names to collide - combine with the peer_id
pub struct TrackKey {
    pub peer_id: PeerId,
    pub track_name: String,
}

pub enum PeerState {
    Disconnected,
    WaitingForSdp,
    WaitingForIce,
    Connected,
}

pub struct Peer {
    pub state: PeerState,
    pub id: PeerId,
    pub connection: Arc<RTCPeerConnection>,
}

pub struct MediaWorker {
    /// receives RTP packets from media devices. for sending to outgoing_media_tracks
    media_rx: broadcast::Receiver<rtp::packet::Packet>,
    outgoing_media_tracks: HashMap<TrackKey, Arc<TrackLocalStaticRTP>>,
}