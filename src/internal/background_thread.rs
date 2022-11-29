use crate::{
    internal::simple_webrtc::{SimpleWebRtc, SimpleWebRtcInit},
    PeerId,
};
use tokio::sync::{broadcast, mpsc, oneshot};
use webrtc::rtp;

pub type GenericResponse = Result<(), ()>;
pub enum InternalCmd {
    Connect {
        peer: PeerId,
        response: oneshot::Sender<GenericResponse>,
    },
    Disconnect {
        peer: PeerId,
        response: oneshot::Sender<GenericResponse>,
    },
    AddTrack {
        id: String,
        source: broadcast::Receiver<rtp::packet::Packet>,
        response: oneshot::Sender<GenericResponse>,
    },
    RemoveTrack {
        id: String,
        response: oneshot::Sender<GenericResponse>,
    },
}

pub async fn run(args: SimpleWebRtcInit, mut rx: mpsc::UnboundedReceiver<InternalCmd>) {
    let _simple_webrtc = SimpleWebRtc::init(args);
    while let Some(_cmd) = rx.recv().await {
        todo!()
        // todo: handle command
    }
}
