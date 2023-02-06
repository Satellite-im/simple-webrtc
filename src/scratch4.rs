use crate::scratch2::CallId;

pub enum BlinkEvents<PeerId> {
    IncomingCall { call_id: CallId },
    CallAccepted { call_id: CallId },
    // not sure if this is needed
    CallEnded { call_id: CallId },
    // somehow only accept WebRTC connections to active participants
    ParticipantJoined { peer_id: PeerId },
    ParticipantLeft { peer_id: PeerId },
    ParticipantSpeaking { peer_id: PeerId },
    ParticipantNotSpeaking { peer_id: PeerId },
}
