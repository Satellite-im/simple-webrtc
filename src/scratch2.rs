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
    // ------ Create/Join a call ------

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

    // ------ Select input/output devices ------

    fn get_available_microphones(&self) -> Vec<String>;
    fn select_microphone(&mut self, device_name: &str);
    fn get_available_speakers(&self) -> Vec<String>;
    fn select_speaker(&mut self, device_name: &str);
    fn get_available_cameras(&self) -> Vec<String>;
    fn select_camera(&mut self, device_name: &str);

    // ------ Media controls ------

    fn mute_self(&mut self);
    fn unmute_self(&mut self);
    fn enable_camera(&mut self);
    fn disable_camera(&mut self);
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
