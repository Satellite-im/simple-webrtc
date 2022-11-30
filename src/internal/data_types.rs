use tokio::sync::mpsc;
use webrtc::rtp;

/// uniquely identifies peers
pub type PeerId = String;

pub enum PeerState {
    Disconnected,
    WaitingForSdp,
    WaitingForIce,
    Connected,
}

pub type MediaSourceId = String;
pub type MediaSourceTx = mpsc::UnboundedSender<rtp::packet::Packet>;
