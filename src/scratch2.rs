use crate::scratch3::MediaCodec;

// this is really raygun::Conversation
type Conversation = ();

pub type StreamId = uuid::Uuid;
// the call id is the same as raygun::Conversation::id()
pub type CallId = uuid::Uuid;

// todo: add function to renegotiate codecs, either for the entire call or
// for one peer. the latter would provide a "low bandwidth" resolution
// even better: store the maximum resolution each peer provides and let the
// peers reach a consensus about what settings to use before a call starts
//
// todo: add functions for screen sharing
pub trait Blink {
    type PeerId;

    // Create/Join a call

    // only one call can be offered at a time.
    // cannot offer a call if another call is in progress
    fn offer_call(
        &mut self,
        conversation: Conversation,
        // default codecs for each type of stream
        config: CallConfig,
    );
    // accept an offered call and automatically publish and subscribe to audio
    fn answer_call(&mut self, call_id: CallId);
    // notify a sender/group that you will not join a call
    fn reject_call(&mut self, call_id: CallId);
    // end/leave the current call
    fn leave_call(&mut self);

    // Communicate during a call

    // create a source track
    fn publish_stream(&mut self, media_type: MediaType);
    // tell the remote side to forward their stream to you
    // a webrtc connection will start in response to this
    fn subscribe_stream(&mut self, peer_id: Self::PeerId, stream_id: StreamId);
    // stop offering the stream and close existing connections to it
    fn unpublish_stream(&mut self, stream_id: StreamId);
    // called by the remote side
    fn close_stream(&mut self, stream_id: StreamId);
    // when joining a call late, used to interrogate each peer about their published streams
    fn query_published_streams(&mut self, peer_id: Self::PeerId);

    // select input/output devices

    fn get_available_microphones(&self) -> Vec<String>;
    fn select_microphone(&mut self, device_name: &str);
    fn get_available_speakers(&self) -> Vec<String>;
    fn select_speaker(&mut self, device_name: &str);
    fn get_available_cameras(&self) -> Vec<String>;
    fn select_camera(&mut self, device_name: &str);
}

pub struct CallConfig {
    audio_codec: MediaCodec,
    camera_codec: MediaCodec,
    screen_share_codec: MediaCodec,
}

pub enum MediaType {
    Audio,
    Camera,
    ScreenShare,
}
