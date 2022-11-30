use crate::internal::data_types::PeerId;
use std::sync::Arc;
use webrtc::ice_transport::ice_candidate::RTCIceCandidate;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

use webrtc::track::track_remote::TrackRemote;

/// Signaling required for SimpleWebRTC
/// the user intercepts EmittedEvents and, when signaling is required, transforms the event into
/// the appropriate signal.
pub enum PeerSignal {
    Ice {
        peer_id: String,
        candidate: Box<RTCIceCandidate>,
    },
    Sdp {
        peer_id: String,
        sdp: Box<RTCSessionDescription>,
    },
    CallInitiated {
        peer_id: String,
        sdp: Box<RTCSessionDescription>,
    },
    CallTerminated {
        peer_id: String,
    },
    CallRejected {
        peer_id: String,
    },
}

#[derive(Debug)]
pub enum EmittedEvents {
    Ice {
        dest: PeerId,
        candidate: Box<RTCIceCandidate>,
    },
    Sdp {
        dest: PeerId,
        sdp: Box<RTCSessionDescription>,
    },
    /// created after calling `Dial`
    CallInitiated {
        dest: PeerId,
        sdp: Box<RTCSessionDescription>,
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
    // it appears that WebRTC doesn't emit an event for this. perhaps the track is automatically
    // closed on the remote side when the local side calls `remove_track`
    // TrackRemoved,
}
