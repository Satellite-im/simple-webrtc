use crate::internal::simple_webrtc::{PeerSignal, TrackDescription};
use crate::MediaSource;
use anyhow::Result;
use hyper::{
    service::{make_service_fn, service_fn},
    Body, Client, Method, Request, Response, Server, StatusCode,
};
use hyper_tls::HttpsConnector;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use webrtc::ice_transport::ice_candidate::RTCIceCandidate;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

/// testing
/// simple_webrtc requires signaling to initiate the WebRTC connection and to add/remove tracks
/// a signaling server is provided for development purposes. This will allow the developers to
/// test audio/video transmission without integrating this library into another application
///
/// Hyper (the web server) doesn't have a good way to share data when the service function
/// isn't a closure so the unboudned channel, used to exchange signaling data, is stored statically.

lazy_static! {
    static ref SIGNAL_CHAN: Mutex<Option<mpsc::UnboundedSender<PeerSignal>>> = Mutex::new(None);
}

/// when a signal is received by the web server, it is transmitted via this channel
pub async fn set_signal_tx_chan(chan: mpsc::UnboundedSender<PeerSignal>) {
    let chan = Some(chan);
    let mut lock = SIGNAL_CHAN.lock().await;
    *lock = chan;
}

pub async fn send_connect(remote_host: &str, id: &str) -> Result<()> {
    send_signal(remote_host, "/connect", id.into()).await
}

pub async fn send_disconnect(remote_host: &str, id: &str) -> Result<()> {
    send_signal(remote_host, "/disconnect", id.into()).await
}

pub async fn send_ice_candidate(remote_host: &str, candidate: &RTCIceCandidate) -> Result<()> {
    let payload = candidate.to_json()?.candidate;
    send_signal(remote_host, "/ice-candidate", payload).await
}

pub async fn send_sdp(remote_host: &str, sdp: &RTCSessionDescription) -> Result<()> {
    let payload = serde_json::to_string(&sdp)?;
    send_signal(remote_host, "/sdp", payload).await
}

pub async fn send_add_track(remote_host: &str, desc: TrackDescription) -> Result<()> {
    let payload = serde_json::to_string(&desc)?;
    send_signal(remote_host, "/add-track", payload).await
}

pub async fn send_remove_track(remote_host: &str, desc: TrackDescription) -> Result<()> {
    let payload = serde_json::to_string(&desc)?;
    send_signal(remote_host, "/remove-track", payload).await
}

async fn send_signal(remote_host: &str, route: &str, payload: String) -> Result<()> {
    let https = HttpsConnector::new();
    let client = Client::builder().build::<_, hyper::Body>(https);

    let req = match Request::builder()
        .method(Method::POST)
        .uri(format!("{}/{}", remote_host, route))
        .header("content-type", "application/json; charset=utf-8")
        .body(Body::from(payload))
    {
        Ok(req) => req,
        Err(err) => {
            log::error!("failed to create request : {}", err);
            return Err(err.into());
        }
    };
    if let Err(e) = client.request(req).await {
        log::error!("failed to send signaling parameters: {}", e);
        return Err(e.into());
    }

    Ok(())
}

pub async fn signaling_server(addr: &str) -> Result<()> {
    let addr = SocketAddr::from_str(addr)?;
    let service = make_service_fn(|_| async { Ok::<_, hyper::Error>(service_fn(remote_handler)) });
    let server = hyper::Server::bind(&addr).serve(service);
    // Run this server for... forever!
    if let Err(e) = server.await {
        log::error!("server error: {}", e);
    }
    Ok(())
}

// would abstract the parsing code if this was actually going to be used
async fn remote_handler(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    // let sdp_tx = CHANNELS.sdp_tx.clone();
    //let ice_tx = CHANNELS.ice_tx.clone();
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::OK;
    match (req.method(), req.uri().path()) {
        (&Method::POST, "/connect") => {
            let peer_id_str =
                match std::str::from_utf8(&hyper::body::to_bytes(req.into_body()).await?) {
                    Ok(s) => s.to_owned(),
                    Err(err) => {
                        log::error!(" error parsing payload: {}", err);
                        *response.status_mut() = StatusCode::BAD_REQUEST;
                        return Ok(response);
                    }
                };
            todo!()
        }
        (&Method::POST, "/disconnect") => {
            let peer_id = match std::str::from_utf8(&hyper::body::to_bytes(req.into_body()).await?)
            {
                Ok(s) => s.to_owned(),
                Err(err) => {
                    log::error!(" error parsing payload: {}", err);
                    *response.status_mut() = StatusCode::BAD_REQUEST;
                    return Ok(response);
                }
            };
            todo!()
        }
        (&Method::POST, "/sdp") => {
            // todo: parse sdp and write to tx channel
            let sdp_str = match std::str::from_utf8(&hyper::body::to_bytes(req.into_body()).await?)
            {
                Ok(s) => s.to_owned(),
                Err(err) => {
                    log::error!(" error parsing payload: {}", err);
                    *response.status_mut() = StatusCode::BAD_REQUEST;
                    return Ok(response);
                }
            };
            let _sdp = match serde_json::from_str::<RTCSessionDescription>(&sdp_str) {
                Ok(s) => s,
                Err(err) => {
                    log::error!("SDP deserialize error: {}", err);
                    *response.status_mut() = StatusCode::BAD_REQUEST;
                    return Ok(response);
                }
            };

            /*if let Err(e) = sdp_tx.send(sdp) {
                log::error!("failed to send SDP: {}", e);
                *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
            }*/

            Ok(response)
        }
        // this route was being used in the webrtc offer-answer example
        // without it, no ICE candiates were gathered. perhaps because of intermittent service from Google's STUN server
        (&Method::POST, "/ice-candidate") => {
            let _candidate =
                match std::str::from_utf8(&hyper::body::to_bytes(req.into_body()).await?) {
                    Ok(s) => s.to_owned(),
                    Err(err) => {
                        log::error!(" error parsing payload: {}", err);
                        *response.status_mut() = StatusCode::BAD_REQUEST;
                        return Ok(response);
                    }
                };

            // todo: don't keep making ICE connections once a connection has been established
            /*if let Err(e) = ice_tx.send(candidate) {
                log::error!("failed to send ICE candidate: {}", e);
                *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
            }*/
            Ok(response)
        }
        (&Method::POST, "/add-track") => {
            let track_str =
                match std::str::from_utf8(&hyper::body::to_bytes(req.into_body()).await?) {
                    Ok(s) => s.to_owned(),
                    Err(err) => {
                        log::error!(" error parsing payload: {}", err);
                        *response.status_mut() = StatusCode::BAD_REQUEST;
                        return Ok(response);
                    }
                };

            let _track_desc = match serde_json::from_str::<TrackDescription>(&track_str) {
                Ok(s) => s,
                Err(err) => {
                    log::error!("StreamInit deserialize error: {}", err);
                    *response.status_mut() = StatusCode::BAD_REQUEST;
                    return Ok(response);
                }
            };

            Ok(response)
        }
        (&Method::POST, "/remove-track") => {
            let track_str =
                match std::str::from_utf8(&hyper::body::to_bytes(req.into_body()).await?) {
                    Ok(s) => s.to_owned(),
                    Err(err) => {
                        log::error!(" error parsing payload: {}", err);
                        *response.status_mut() = StatusCode::BAD_REQUEST;
                        return Ok(response);
                    }
                };

            let _track_desc = match serde_json::from_str::<TrackDescription>(&track_str) {
                Ok(s) => s,
                Err(err) => {
                    log::error!("StreamInit deserialize error: {}", err);
                    *response.status_mut() = StatusCode::BAD_REQUEST;
                    return Ok(response);
                }
            };

            Ok(response)
        }

        // Return the 404 Not Found for other routes.
        _ => {
            *response.status_mut() = StatusCode::NOT_FOUND;
            Ok(response)
        }
    }
}
