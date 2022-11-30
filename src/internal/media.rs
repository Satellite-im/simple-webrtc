use std::collections::HashMap;
use std::sync::Arc;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use webrtc::api::media_engine::{
    MIME_TYPE_AV1, MIME_TYPE_G722, MIME_TYPE_H264, MIME_TYPE_OPUS, MIME_TYPE_PCMA, MIME_TYPE_PCMU,
    MIME_TYPE_VP8, MIME_TYPE_VP9,
};
use webrtc::rtp;
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocalWriter;
use crate::PeerId;

/// Indicates the device from which the media stream originates
#[derive(Serialize, Deserialize)]
pub enum MediaSource {
    /// audio destination
    Microphone,
    /// video destination
    Camera,
    /// video destination
    Screen,
}

/// represents the MIME types from webrtc::api::media_engine
#[derive(Serialize, Deserialize)]
pub enum MimeType {
    // https://en.wikipedia.org/wiki/Advanced_Video_Coding
    // the most popular video compression standard
    // requires paying patent licencing royalties to MPEG LA
    // patent expires in july 2023
    H264,
    // https://en.wikipedia.org/wiki/VP8
    // royalty-free video compression format
    // can be multiplexed into Matroska and WebM containers, along with Vorbis and Opus audio
    VP8,
    // https://en.wikipedia.org/wiki/VP9
    // royalty-free  video coding format
    VP9,
    // https://en.wikipedia.org/wiki/AV1
    // royalty-free video coding format
    // successor to VP9
    AV1,
    // https://en.wikipedia.org/wiki/Opus_(audio_format)
    // lossy audio coding format
    // BSD-3 license
    OPUS,
    // https://en.wikipedia.org/wiki/G.722
    // royalty-free audio codec
    // 7kHz wideband audio at data rates from 48, 56, and 65 kbit/s
    // commonly used for VoIP
    G722,
    // https://en.wikipedia.org//wiki/G.711
    // royalty free audio codec
    // narrowband audio codec
    // also known as G.711 µ-law
    PCMU,
    // https://en.wikipedia.org//wiki/G.711
    // also known as G.711 A-law
    PCMA,
}

impl ToString for MimeType {
    fn to_string(&self) -> String {
        let s = match self {
            MimeType::H264 => MIME_TYPE_H264,
            MimeType::VP8 => MIME_TYPE_VP8,
            MimeType::VP9 => MIME_TYPE_VP9,
            MimeType::AV1 => MIME_TYPE_AV1,
            MimeType::OPUS => MIME_TYPE_OPUS,
            MimeType::G722 => MIME_TYPE_G722,
            MimeType::PCMU => MIME_TYPE_PCMU,
            MimeType::PCMA => MIME_TYPE_PCMA,
        };
        s.into()
    }
}

impl MimeType {
    pub fn from_string(s: &str) -> Result<Self> {
        let mime_type = match s {
            MIME_TYPE_H264 => MimeType::H264,
            MIME_TYPE_VP8 => MimeType::VP8,
            MIME_TYPE_VP9 => MimeType::VP9,
            MIME_TYPE_AV1 => MimeType::AV1,
            MIME_TYPE_OPUS => MimeType::OPUS,
            MIME_TYPE_G722 => MimeType::G722,
            MIME_TYPE_PCMU => MimeType::PCMU,
            MIME_TYPE_PCMA => MimeType::PCMA,
            _ => bail!(format! {"invalid mime type: {}", s}),
        };
        Ok(mime_type)
    }
}


pub enum MediaWorkerCommands {
    AddTrack {
        peer: PeerId,
        track: Arc<TrackLocalStaticRTP>,
    },
    RemoveTrack {
        peer: PeerId,
    },
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

/// Used by SimpleWebRTC to control its MediaWorkers
/// may want to allow the user to dynamically add/remove Media sources
pub struct MediaWorkerChannels {
    /// controls a worker thread which processes camera input
    pub camera: mpsc::UnboundedSender<MediaWorkerCommands>,
    /// controls a worker thread which processes microphone input
    pub microphone: mpsc::UnboundedSender<MediaWorkerCommands>,
    /// controls a worker thread which processes screen input
    pub screen: mpsc::UnboundedSender<MediaWorkerCommands>,
}

/// allows the user to send their captured media streams to SimpleWebRTC
pub struct MediaWorkerInputs {
    pub camera_tx: mpsc::UnboundedSender<rtp::packet::Packet>,
    pub microphone_tx: mpsc::UnboundedSender<rtp::packet::Packet>,
    pub screen_tx: mpsc::UnboundedSender<rtp::packet::Packet>,
}

/// adds/removes tracks in response to control signals
/// receives RTP packets and forwards them to the tracks
impl MediaWorker {
    pub async fn run(&mut self) {
        loop {
            tokio::select! {
                cmd = self.control_rx.recv() => match cmd {
                  Some(cmd) => match cmd {
                        MediaWorkerCommands::AddTrack{
                            peer,
                            track
                        } => {
                            if self.outgoing_media_tracks.insert(peer.clone(), track).is_some() {
                                log::info!("overwriting media track for peer {}", peer);
                            }
                        },
                        MediaWorkerCommands::RemoveTrack{ peer } => {
                            if self.outgoing_media_tracks.remove(&peer).is_none() {
                                log::info!("removed nonexistent media track for peer: {}", &peer);
                            }
                        },
                        MediaWorkerCommands::Terminate => return,
                    }
                    None => return
                },
                opt = self.media_rx.recv() => match opt {
                    Some(packet) => {
                        for (track_name, track) in &self.outgoing_media_tracks {
                            if let Err(e) = track.write_rtp(&packet).await {
                                // todo: should the track be removed if write_rtp fails?
                                log::warn!("failed to write RTP packet to track {}", track_name);
                            }
                        }
                    },
                    None => {
                        // channel was closed
                        log::info!("MediaWorker channel closed. exiting");
                        return;
                    }
                }
            }
        }
    }
}