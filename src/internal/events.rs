
/// peer-to-peer signals - can be sent or received
pub enum PeerSignals {
    Ice,
    Sdp,
    CallInitiated,
    CallTerminated,
    CallRejected
}

pub enum EmittedEvents {
    Ice,
    Sdp,
    /// unless a CallTerminated event was received, results in a reconnect
    Disconnected,
    /// a peer added a track. this track is stored and sent to the corresponding MediaWorker
    TrackAdded,
    TrackRemoved,
}