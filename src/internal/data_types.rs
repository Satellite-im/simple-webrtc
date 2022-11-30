/// uniquely identifies peers
pub type PeerId = String;

pub enum PeerState {
    Disconnected,
    WaitingForSdp,
    WaitingForIce,
    Connected,
}