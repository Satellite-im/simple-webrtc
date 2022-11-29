use tokio::sync::{broadcast, mpsc, oneshot};
use webrtc::rtp;

mod internal;
use internal::background_thread::GenericResponse;
use internal::background_thread::InternalCmd;

// public exports
pub use internal::mime_type::*;
pub use internal::simple_webrtc::{MediaSource, PeerId, PeerSignal, SimpleWebRtcInit, TrackOpened};

#[cfg(feature = "test-server")]
mod testing;
#[cfg(feature = "test-server")]
pub use testing::signaling_server;

#[cfg(feature = "test-server")]
#[macro_use]
extern crate lazy_static;

/// simple-webrtc
/// This library augments the [webrtc-rs](https://github.com/webrtc-rs/webrtc) library, hopefully
/// simplifying the process of exchanging media with multiple peers simultaneously.
///
/// this library allows for the exchange of RTP packets. Transforming audio/video into RTP packets
/// is the user's responsibility.
///
/// WebRTC requires out of band signalling. The `SimpleWebRtc` accepts a callback for transmitting
/// signals and a channel for receiving signals.

/// Managing the WebRTC connections requires simultaneously monitoring multiple channels. Rust
/// doesn't allow multiple mutable references so it seems too much trouble to use one struct
/// to both poll channels and provide an API which provides mutator methods. Instead, let an
/// external API provide the nice convenient methods a user would like and then behind the
/// scenes communicate with a background thread via channels.
pub struct SimpleWebRtc {
    // when SimpleWebRtc is dropped, this channel will close and then the
    // background thread will detect a closed channel and terminate
    control_channel: mpsc::UnboundedSender<InternalCmd>,
}

impl SimpleWebRtc {
    pub async fn init(args: SimpleWebRtcInit) -> Self {
        let (tx, rx) = mpsc::unbounded_channel::<InternalCmd>();

        tokio::spawn(async move {
            internal::background_thread::run(args, rx).await;
        });

        Self {
            control_channel: tx,
        }
    }

    /// Initiates a connection
    pub async fn connect(&self, peer: PeerId) {
        let (tx, rx) = oneshot::channel::<GenericResponse>();
        if let Err(_e) = self
            .control_channel
            .send(InternalCmd::Connect { peer, response: tx })
        {
            todo!();
        }

        match rx.await {
            Ok(_r) => todo!(),
            Err(_e) => todo!(),
        }
    }
    /// terminates a connection
    pub async fn disconnect(&self, peer: PeerId) {
        let (tx, rx) = oneshot::channel::<GenericResponse>();
        if let Err(_e) = self
            .control_channel
            .send(InternalCmd::Disconnect { peer, response: tx })
        {
            todo!();
        }

        match rx.await {
            Ok(_r) => todo!(),
            Err(_e) => todo!(),
        }
    }
    /// Add a media track (audio or video)
    /// Registers a media source, stored in `local_media_tracks`
    /// when a peer connects, a corresponding connection is automatically created in
    /// `outgoing_media_tracks`.
    pub async fn add_track(&self, id: &str, source: broadcast::Receiver<rtp::packet::Packet>) {
        let (tx, rx) = oneshot::channel::<GenericResponse>();
        if let Err(_e) = self.control_channel.send(InternalCmd::AddTrack {
            id: id.into(),
            source,
            response: tx,
        }) {
            todo!();
        }

        match rx.await {
            Ok(_r) => todo!(),
            Err(_e) => todo!(),
        }
    }
    /// remove a media track
    pub async fn remove_track(&self, id: &str) {
        let (tx, rx) = oneshot::channel::<GenericResponse>();
        if let Err(_e) = self.control_channel.send(InternalCmd::RemoveTrack {
            id: id.into(),
            response: tx,
        }) {
            todo!();
        }

        match rx.await {
            Ok(_r) => todo!(),
            Err(_e) => todo!(),
        }
    }
}
