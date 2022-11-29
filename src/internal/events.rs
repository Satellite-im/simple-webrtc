use crate::internal::data_types::PeerId;
use std::sync::Arc;
use webrtc::ice_transport::ice_candidate::RTCIceCandidate;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_remote::TrackRemote;

/// peer-to-peer signals - can be sent or received
pub enum PeerSignals {
    Ice,
    Sdp,
    CallInitiated,
    CallTerminated,
    CallRejected,
}

#[derive(Debug)]
pub enum EmittedEvents {
    Ice {
        dest: PeerId,
        candidate: RTCIceCandidate,
    },
    Sdp {
        dest: PeerId,
        sdp: RTCSessionDescription,
    },
    /// unless a CallTerminated event was received, results in a reconnect
    /// needs to be handled by the developer
    Disconnected { peer: PeerId },
    /// a peer added a track. The calling application is responsible for reading from the track
    /// and processing the output
    TrackAdded {
        peer: PeerId,
        track: Arc<TrackRemote>,
    },
    // it apperas that WebRTC doesn't emit an event for this. perhaps the track is automatically
    // closed on the remote side when the local side calls `remove_track`
    // TrackRemoved,
}
