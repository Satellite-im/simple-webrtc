// this is really raygun::Conversation
type Conversation = ();
pub type StreamId = uuid::Uuid;
// the call id is the same as raygun::Conversation::id()
pub type CallId = uuid::Uuid;
pub trait Blink {
    type PeerId;
    type AudioConfig;
    type VideoConfig;
    // only one call can be offered at a time.
    // cannot offer a call if another call is in progress
    fn offer_call(
        &mut self,
        conversation: Conversation,
        audio_config: Self::AudioConfig,
        video_config: Self::VideoConfig,
    );
    // accept an offered call and automatically publish and subscribe to audio
    fn answer_call(&mut self, call_id: CallId);
    // notify a sender/group that you will not join a call
    fn reject_call(&mut self, call_id: CallId);
    // end/leave the current call
    fn leave_call(&mut self);
    // create a source track
    fn publish_stream(&mut self, stream_config: MediaStreamConfig);
    // tell the remote side to forward their stream to you
    // a webrtc connection will start in response to this
    fn subscribe_stream(&mut self, peer_id: Self::PeerId, stream_id: StreamId);
    // stop offering the stream and close existing connections to it
    fn unpublish_stream(&mut self, stream_id: StreamId);
    // called by the remote side
    fn close_stream(&mut self, stream_id: StreamId);
    // when joining a call late, used to interrogate each peer about their published streams
    fn query_published_streams(&mut self, peer_id: Self::PeerId);
}

// used internally by the Blink implementation
pub struct CallConfig<AudioConfig, VideoConfig> {
    call_id: CallId,
    audio_config: AudioConfig,
    video_config: VideoConfig,
}

pub struct MediaStreamConfig {
    stream_id: StreamId,
    subtype: MediaStreamType,
}

pub enum MediaStreamType {
    Audio,
    Video,
}
