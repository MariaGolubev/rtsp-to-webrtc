use std::sync::Arc;

use axum::{
    extract::{FromRequest, State},
    response::IntoResponse,
};
use tracing::{debug, error, info, warn};
use webrtc::{
    peer_connection::sdp::session_description::RTCSessionDescription,
    rtcp::{
        goodbye::Goodbye,
        payload_feedbacks::{
            full_intra_request::FullIntraRequest, picture_loss_indication::PictureLossIndication,
        },
        receiver_report::ReceiverReport,
        sender_report::SenderReport,
        transport_feedbacks::{
            rapid_resynchronization_request::RapidResynchronizationRequest,
            transport_layer_cc::TransportLayerCc,
        },
    },
    track::track_local::TrackLocal,
};

use crate::state::AppState;

pub struct SDPOffer(pub RTCSessionDescription);

impl<S> FromRequest<S> for SDPOffer
where
    S: Send + Sync,
{
    type Rejection = axum::http::StatusCode;

    async fn from_request(
        req: axum::http::Request<axum::body::Body>,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let ct = req
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();

        if ct.split(';').next().map(|s| s.trim()) != Some("application/sdp") {
            warn!("Invalid Content-Type: '{}'", ct);
            return Err(axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE);
        }

        let bytes = axum::body::to_bytes(req.into_body(), 1024 * 16)
            .await
            .map_err(|e| {
                error!("Failed to read body: {}", e);
                axum::http::StatusCode::BAD_REQUEST
            })?;

        Ok(SDPOffer(
            RTCSessionDescription::offer(String::from_utf8_lossy(bytes.as_ref()).to_string())
                .map_err(|e| {
                    error!("Failed to parse SDP: {}", e);
                    axum::http::StatusCode::BAD_REQUEST
                })?,
        ))
    }
}

pub struct SDPAnswer(pub RTCSessionDescription, String);

impl IntoResponse for SDPAnswer {
    fn into_response(self) -> axum::response::Response {
        let sdp_str = self.0.sdp;
        let id = self.1;

        let location_value = format!("/resource/{}", id);
        axum::response::Response::builder()
            .header(axum::http::header::CONTENT_TYPE, "application/sdp")
            .header(axum::http::header::LOCATION, location_value)
            .status(axum::http::StatusCode::CREATED)
            .body(axum::body::Body::from(sdp_str))
            .unwrap()
    }
}

pub async fn whep_offer(
    State(AppState {
        api,
        peer_connections,
        video_track,
        audio_track,
    }): State<AppState>,
    SDPOffer(offer): SDPOffer,
) -> SDPAnswer {
    let pc = api
        .new_peer_connection(webrtc::peer_connection::configuration::RTCConfiguration::default())
        .await
        .unwrap();

    let pc = Arc::new(pc);

    let rtp_video_sender = pc
        .add_track(video_track.1.clone() as Arc<dyn TrackLocal + Send + Sync>)
        .await
        .unwrap();

    tokio::spawn(async move {
        let mut rtcp_buf = vec![0u8; 1500];
        while let Ok((_, _)) = rtp_video_sender.read(&mut rtcp_buf).await {}
    });

    if let Some((_, audio_track)) = audio_track {
        let rtp_audio_sender = pc
            .add_track(audio_track.clone() as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .unwrap();

        tokio::spawn(async move {
            let mut rtcp_buf = [0u8; 1500];
            while let Ok((rtcp, _atr)) = rtp_audio_sender.read(&mut rtcp_buf).await {
                for pkt in rtcp {
                    if let Some(_sr) = pkt.as_any().downcast_ref::<SenderReport>() {
                        debug!("RTCP: Sender Report (SR)");
                        continue;
                    }
                    if let Some(_rr) = pkt.as_any().downcast_ref::<ReceiverReport>() {
                        debug!("RTCP: Receiver Report (RR)");
                        continue;
                    }
                    if let Some(_pli) = pkt.as_any().downcast_ref::<PictureLossIndication>() {
                        debug!("RTCP: PLI (Picture Loss Indication)");
                        continue;
                    }
                    if let Some(_fir) = pkt.as_any().downcast_ref::<FullIntraRequest>() {
                        debug!("RTCP: FIR (Full Intra Request)");
                        continue;
                    }
                    if let Some(_tcc) = pkt.as_any().downcast_ref::<TransportLayerCc>() {
                        debug!("RTCP: TCC (Transport-wide Congestion Control)");
                        continue;
                    }
                    if let Some(_rrr) = pkt.as_any().downcast_ref::<RapidResynchronizationRequest>()
                    {
                        debug!("RTCP: Rapid Resync Request (RRR)");
                        continue;
                    }
                    if let Some(_bye) = pkt.as_any().downcast_ref::<Goodbye>() {
                        debug!("RTCP: BYE");
                        continue;
                    }

                    debug!("RTCP: Unknown / Raw packet");
                }
            }
        });
    }

    let id = uuid::Uuid::new_v4().to_string();

    // Set up peer connection state change handler
    let id_for_handler = id.clone();
    let peer_connections_clone = peer_connections.clone();
    pc.on_peer_connection_state_change(Box::new(move |state| {
        let id = id_for_handler.clone();
        let peer_connections = peer_connections_clone.clone();

        Box::pin(async move {
            use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;

            match state {
                RTCPeerConnectionState::Disconnected
                | RTCPeerConnectionState::Failed
                | RTCPeerConnectionState::Closed => {
                    info!("🔌 Connection {} state: {:?}, cleaning up", &id[..8], state);

                    if let Some((_, pc)) = peer_connections.remove(&id) {
                        let _ = pc.close().await;
                    }

                    info!(
                        "🧹 Session {} auto-removed | Remaining: {}",
                        &id[..8],
                        peer_connections.len()
                    );
                }
                _ => {}
            }
        })
    }));

    pc.set_remote_description(offer).await.unwrap();

    let answer = pc.create_answer(None).await.unwrap();

    pc.set_local_description(answer.clone()).await.unwrap();

    peer_connections.insert(id.clone(), pc);

    info!(
        "✅ Session created: {} | Sessions: {}",
        &id[..8],
        peer_connections.len()
    );

    SDPAnswer(answer, id)
}

pub async fn whep_delete(
    State(AppState {
        peer_connections, ..
    }): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> axum::http::StatusCode {
    if let Some((_, pc)) = peer_connections.remove(&id) {
        pc.close().await.unwrap();

        info!(
            "🗑️  Session deleted: {} | Remaining: {}",
            &id[..8],
            peer_connections.len()
        );

        axum::http::StatusCode::NO_CONTENT
    } else {
        warn!("⚠️  Session not found: {}", &id[..8]);
        axum::http::StatusCode::NOT_FOUND
    }
}
