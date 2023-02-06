use uuid::Uuid;

use crate::scratch2::CallId;

pub struct Peer {
    peer_id: Uuid,
    display_name: String,
}

pub enum BlinkEvents {
    IncomingCall { call_id: CallId },
    CallAccepted { call_id: CallId },
    // not sure if this is needed
    CallEnded { call_id: CallId },
    // somehow only accept WebRTC connections to active participants
    ParticipantJoined { peer_id: Peer },
    ParticipantLeft { peer_id: Peer },
    ParticipantSpeaking { peer_id: Peer },
    ParticipantNotSpeaking { peer_id: Peer },
}
