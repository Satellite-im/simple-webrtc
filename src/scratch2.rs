// this is really raygun::Conversation
type Conversation = ();
pub trait Blink {
    type PeerId;
    type CallId;
    type StreamId;
    type AudioConfig;
    type VideoConfig;
    // only one call can be offered at a time.
    // cannot offer a call if another call is in progress
    fn offer_call(
        &mut self,
        conversation: Conversation,
        call_config: CallConfig<Self::CallId, Self::AudioConfig, Self::VideoConfig>,
    );
    // accept an offered call and automatically publish and subscribe to audio
    fn answer_call(&mut self, call_id: Self::CallId);
    // notify a sender/group that you will not join a call
    fn reject_call(&mut self, call_id: Self::CallId);
    // end/leave the current call
    fn leave_call(&mut self);
    fn publish_stream(&mut self, stream_config: MediaStreamConfig<Self::StreamId>);
    fn subscribe_stream(&mut self, peer_id: Self::PeerId, stream_id: Self::StreamId);
    fn revoke_stream(&mut self, stream_id: Self::StreamId);
    fn close_stream(&mut self, stream_id: Self::StreamId);
    fn query_offered_streams(&mut self, peer_id: Self::PeerId);
}

pub struct CallConfig<CallId, AudioConfig, VideoConfig> {
    call_id: CallId,
    audio_config: AudioConfig,
    video_config: VideoConfig,
}

pub struct MediaStreamConfig<StreamId> {
    stream_id: StreamId,
    subtype: MediaStreamType,
}

pub enum MediaStreamType {
    Audio,
    Video,
}
