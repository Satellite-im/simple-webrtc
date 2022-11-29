use anyhow::Result;
use crate::{
    internal::simple_webrtc::{SimpleWebRtc, SimpleWebRtcInit},
    PeerId,
};
use tokio::sync::{broadcast, mpsc, oneshot};
use webrtc::ice_transport::ice_candidate::RTCIceCandidate;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
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
    IceCandidate {
        peer: PeerId,
        candidate: RTCIceCandidate,
        response: oneshot::Sender<GenericResponse>,
    },
    Sdp {
        peer: PeerId,
        sdp: RTCSessionDescription,
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

pub async fn run(args: SimpleWebRtcInit, mut rx: mpsc::UnboundedReceiver<InternalCmd>) -> Result<()> {
    let mut simple_webrtc = SimpleWebRtc::init(args)? ;
    while let Some(cmd) = rx.recv().await {
       match cmd {
           InternalCmd::Connect {peer, response} => {
               let result = match simple_webrtc.connect(&peer).await {
                   Ok(_) => {
                       Ok(())
                   },
                   Err(e) => {
                       log::error!("connect failed: {}", e);
                       Err(())
                   }
               };

               if response.send(result).is_err() {
                   log::error!("failed to send connect response");
               }
           }
           InternalCmd::Disconnect {peer, response} => {
               let result = match simple_webrtc.disconnect(&peer).await {
                   Ok(_) => {
                       Ok(())
                   },
                   Err(e) => {
                       log::error!("disconnect failed: {}", e);
                       Err(())
                   }
               };

               if response.send(result).is_err() {
                   log::error!("failed to send disconnect response");
               }
           }
           InternalCmd::IceCandidate {peer, candidate, response} => {
               let result = match simple_webrtc.recv_ice_candidate(&peer, candidate).await {
                   Ok(_) => {
                      Ok(())
                   },
                   Err(e) => {
                       log::error!("recv_ice_candidate failed: {}", e);
                       Err(())
                   }
               };

               if response.send(result).is_err() {
                   log::error!("failed to send recv_ice_candidate response");
               }
           }
           InternalCmd::Sdp{peer, sdp, response} => {
               let result = match simple_webrtc.recv_sdp(&peer, sdp).await {
                   Ok(_) => {
                       Ok(())
                   },
                   Err(e) => {
                       log::error!("recv_sdp failed: {}", e);
                       Err(())
                   }
               };

               if response.send(result).is_err() {
                   log::error!("failed to send recv_sdp response");
               }
           }
           InternalCmd::AddTrack {id, source, response} => {
               todo!()
           }
           InternalCmd::RemoveTrack {id, response} => {

               todo!()
           }
       }
    }

    Ok(())
}
