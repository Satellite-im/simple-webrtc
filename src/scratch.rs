pub mod media {
    use anyhow::Result;
    use async_trait::async_trait;
    use std::sync::Arc;

    /// Captures media on the host computer
    pub trait SourceTrack {
        type Device;
        type LocalTrack;
        type Codec;
        fn init(
            input_device: Self::Device,
            track: Arc<Self::LocalTrack>,
            codec: Self::Codec,
        ) -> Result<Self>
        where
            Self: Sized;

        fn play(&self) -> Result<()>;
        fn pause(&self) -> Result<()>;
        fn change_input_device(&mut self, input_device: Self::Device);
    }

    /// Receives incoming media tracks
    #[async_trait]
    pub trait SinkTrack {
        type Device;
        type Codec;
        type RemoteTrack;
        // for an audio track, this could be None
        // probably only needed for a video track
        type OutputStream;

        fn init(
            output_device: Self::Device,
            track: Arc<Self::RemoteTrack>,
            codec: Self::Codec,
        ) -> Result<Self>
        where
            Self: Sized;

        fn play(&mut self) -> Result<()>;
        fn pause(&self) -> Result<()>;
        fn change_output_device(&mut self, output_device: Self::Device);
        async fn get_output_stream(&mut self) -> Result<Self::OutputStream>;
    }
}

pub mod web_rtc {
    use anyhow::Result;
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    pub type MediaSourceId = uuid::Uuid;

    /// Handles WebRTC functions. Relies on an external
    /// module for transmitting the signals (dial, accept_call, etc)
    #[async_trait]
    pub trait Controller {
        type PeerId;
        type LocalTrack;
        type RemoteTrack;
        type Codec;
        type Sdp;
        type IceCandidate;
        fn init(
            id: Self::PeerId,
            emitted_event_chan: mpsc::UnboundedSender<
                EmittedEvents<Self::PeerId, Self::IceCandidate, Self::Sdp, Self::RemoteTrack>,
            >,
        ) -> Result<Self>
        where
            Self: Sized;
        fn deinit(&mut self) -> Result<()>;
        async fn dial(&mut self, peer_id: Self::PeerId) -> Result<()>;
        async fn accept_call(&mut self, peer_id: Self::PeerId, remote_sdp: Self::Sdp)
            -> Result<()>;
        async fn hang_up(&mut self, peer_id: Self::PeerId);
        async fn add_media_source(
            &mut self,
            source_id: Self::PeerId,
            codec: Self::Codec,
        ) -> Result<Arc<Self::LocalTrack>>;
        async fn remove_media_source(&mut self, source_id: MediaSourceId) -> Result<()>;
        async fn recv_ice(
            &mut self,
            peer_id: Self::PeerId,
            candidate: Self::IceCandidate,
        ) -> Result<()>;
        async fn recv_sdp(&mut self, peer_id: Self::PeerId, sdp: Self::Sdp) -> Result<()>;
    }

    pub enum EmittedEvents<PeerId, Ice, Sdp, TrackRemote> {
        Ice {
            dest: PeerId,
            candidate: Box<Ice>,
        },
        Sdp {
            dest: PeerId,
            sdp: Box<Sdp>,
        },
        /// created after calling `Dial`
        CallInitiated {
            dest: PeerId,
            sdp: Box<Sdp>,
        },
        /// unless a CallTerminated event was received, results in a reconnect
        /// needs to be handled by the developer
        Disconnected {
            peer: PeerId,
        },
        /// a peer added a track. The calling application is responsible for reading from the track
        /// and processing the output
        TrackAdded {
            peer: PeerId,
            track: Arc<TrackRemote>,
        },
    }
}
