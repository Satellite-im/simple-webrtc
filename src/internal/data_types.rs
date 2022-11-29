use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_remote::TrackRemote;

/// uniquely identifies peers
pub type PeerId = String;

pub enum PeerState {
    Disconnected,
    WaitingForSdp,
    WaitingForIce,
    Connected,
}

/// stores a PeerConnection for updating SDP and ICE candidates, adding and removing tracks
/// also stores associated media streams
pub struct Peer {
    pub state: PeerState,
    pub id: PeerId,
    pub connection: Arc<RTCPeerConnection>,
    /// incoming media
    pub tracks: HashMap<String, Arc<TrackRemote>>,
}

pub enum MediaWorkerCommands {
    AddTrack,
    RemoveTrack,
    Terminate,
}

/// each MediaWorker only handles one type of media.
/// it should be sufficient to identify outgoing_media_tracks by the peer id
pub struct MediaWorker {
    /// receives control signals from Controller
    pub control_rx: mpsc::UnboundedReceiver<MediaWorkerCommands>,
    /// receives RTP packets from media devices. for sending to outgoing_media_tracks
    pub media_rx: mpsc::UnboundedReceiver<rtp::packet::Packet>,
    /// locally created tracks - for sending data to peers
    pub outgoing_media_tracks: HashMap<PeerId, Arc<TrackLocalStaticRTP>>,
}

pub struct MediaWorkerChannels {
    /// controls a worker thread which processes camera input
    pub camera: mpsc::UnboundedSender<MediaWorkerCommands>,
    /// controls a worker thread which processes microphone input
    pub microphone: mpsc::UnboundedSender<MediaWorkerCommands>,
    /// controls a worker thread which processes screen input
    pub screen: mpsc::UnboundedSender<MediaWorkerCommands>,
}

pub struct MediaWorkerInputs {
    pub camera_tx: mpsc::UnboundedSender<rtp::packet::Packet>,
    pub microphone_tx: mpsc::UnboundedSender<rtp::packet::Packet>,
    pub screen_tx: mpsc::UnboundedSender<rtp::packet::Packet>,
}

/// adds/removes tracks in response to control signals
/// receives RTP packets and forwards them to the tracks
/// maybe this shouldn't be hidden in data_types.rs
impl MediaWorker {
    pub async fn run(&mut self) {
        loop {
            tokio::select! {
                cmd = self.control_rx.recv() => match cmd {
                  Some(cmd) => match cmd {
                        MediaWorkerCommands::AddTrack => todo!(),
                        MediaWorkerCommands::RemoveTrack => todo!(),
                        MediaWorkerCommands::Terminate => return,
                    }
                    None => return
                },
                packet = self.media_rx.recv() => {
                    todo!()
                }
            }
        }
    }
}
