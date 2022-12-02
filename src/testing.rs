use anyhow::Result;
use hyper::{
    service::{make_service_fn, service_fn},
    Body, Client, Method, Request, Response, StatusCode,
};
//use hyper_tls::HttpsConnector;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::str::FromStr;
use hyper::client::HttpConnector;
use tokio::sync::{mpsc, Mutex};
use webrtc::ice_transport::ice_candidate::RTCIceCandidate;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

// testing
// simple_webrtc requires signaling to initiate the WebRTC connection and to add/remove tracks
// a signaling server is provided for development purposes. This will allow the developers to
// test audio/video transmission without integrating this library into another application
//
// Hyper (the web server) doesn't have a good way to share data when the service function
// isn't a closure so the unboudned channel, used to exchange signaling data, is stored statically.

lazy_static! {
    static ref SIGNAL_CHAN: Mutex<Option<mpsc::UnboundedSender<PeerSignal>>> = Mutex::new(None);
}

#[derive(Serialize, Deserialize)]
pub struct SigSdp {
    pub src: String,
    pub sdp: RTCSessionDescription,
}

#[derive(Serialize, Deserialize)]
pub struct SigIce {
    pub src: String,
    pub ice: RTCIceCandidate,
}

pub enum PeerSignal {
    Ice(SigIce),
    Sdp(SigSdp),
    CallInitiated(SigSdp),
    CallTerminated(String),
    CallRejected(String),
}

/// when a signal is received by the web server, it is transmitted via this channel
pub async fn set_signal_tx_chan(chan: mpsc::UnboundedSender<PeerSignal>) {
    let chan = Some(chan);
    let mut lock = SIGNAL_CHAN.lock().await;
    *lock = chan;
}

pub async fn send_connect(dest: &str, sig: SigSdp) -> Result<()> {
    let payload = serde_json::to_string(&sig)?;
    send_signal(dest, "connect", payload).await
}

pub async fn send_disconnect(remote_host: &str, id: &str) -> Result<()> {
    send_signal(remote_host, "disconnect", id.into()).await
}

pub async fn send_ice_candidate(remote_host: &str, sig: SigIce) -> Result<()> {
    let payload = serde_json::to_string(&sig)?;
    send_signal(remote_host, "ice-candidate", payload).await
}

pub async fn send_sdp(remote_host: &str, sig: SigSdp) -> Result<()> {
    let payload = serde_json::to_string(&sig)?;
    send_signal(remote_host, "sdp", payload).await
}

async fn send_signal(remote_host: &str, route: &str, payload: String) -> Result<()> {
    let http = HttpConnector::new();
    let client = Client::builder().build::<_, hyper::Body>(http);

    let req = match Request::builder()
        .method(Method::POST)
        .uri(format!("http://{}/{}", remote_host, route))
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
            let sig_str = match std::str::from_utf8(&hyper::body::to_bytes(req.into_body()).await?)
            {
                Ok(s) => s.to_owned(),
                Err(err) => {
                    log::error!(" error parsing payload: {}", err);
                    *response.status_mut() = StatusCode::BAD_REQUEST;
                    return Ok(response);
                }
            };

            let sig = match serde_json::from_str::<SigSdp>(&sig_str) {
                Ok(s) => s,
                Err(err) => {
                    log::error!("deserialize error: {}", err);
                    *response.status_mut() = StatusCode::BAD_REQUEST;
                    return Ok(response);
                }
            };

            {
                let opt = SIGNAL_CHAN.lock().await;
                if let Some(ch) = &*opt {
                    if let Err(e) = ch.send(PeerSignal::CallInitiated(sig)) {
                        log::error!("failed to send signal: {}", e);
                    }
                }
            }
            Ok(response)
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
            {
                let opt = SIGNAL_CHAN.lock().await;
                if let Some(ch) = &*opt {
                    if let Err(e) = ch.send(PeerSignal::CallTerminated(peer_id)) {
                        log::error!("failed to send signal: {}", e);
                    }
                }
            }
            Ok(response)
        }
        (&Method::POST, "/sdp") => {
            let sig_str = match std::str::from_utf8(&hyper::body::to_bytes(req.into_body()).await?)
            {
                Ok(s) => s.to_owned(),
                Err(err) => {
                    log::error!(" error parsing payload: {}", err);
                    *response.status_mut() = StatusCode::BAD_REQUEST;
                    return Ok(response);
                }
            };
            let sig = match serde_json::from_str::<SigSdp>(&sig_str) {
                Ok(s) => s,
                Err(err) => {
                    log::error!("deserialize error: {}", err);
                    *response.status_mut() = StatusCode::BAD_REQUEST;
                    return Ok(response);
                }
            };

            {
                let opt = SIGNAL_CHAN.lock().await;
                if let Some(ch) = &*opt {
                    if let Err(e) = ch.send(PeerSignal::Sdp(sig)) {
                        log::error!("failed to send signal: {}", e);
                    }
                }
            }
            Ok(response)
        }
        // this route was being used in the webrtc offer-answer example
        // without it, no ICE candiates were gathered. perhaps because of intermittent service from Google's STUN server
        (&Method::POST, "/ice-candidate") => {
            let sig_str =
                match std::str::from_utf8(&hyper::body::to_bytes(req.into_body()).await?) {
                    Ok(s) => s.to_owned(),
                    Err(err) => {
                        log::error!(" error parsing payload: {}", err);
                        *response.status_mut() = StatusCode::BAD_REQUEST;
                        return Ok(response);
                    }
                };

            let sig = match serde_json::from_str::<SigIce>(&sig_str) {
                Ok(s) => s,
                Err(err) => {
                    log::error!("deserialize error: {}", err);
                    *response.status_mut() = StatusCode::BAD_REQUEST;
                    return Ok(response);
                }
            };

            {
                let opt = SIGNAL_CHAN.lock().await;
                if let Some(ch) = &*opt {
                    if let Err(e) = ch.send(PeerSignal::Ice(sig)) {
                        log::error!("failed to send signal: {}", e);
                    }
                }
            }
            Ok(response)
        }
        // Return the 404 Not Found for other routes.
        _ => {
            *response.status_mut() = StatusCode::NOT_FOUND;
            Ok(response)
        }
    }
}
