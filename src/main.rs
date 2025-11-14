mod cli;
mod codec;
mod state;
mod whep;

use std::sync::Arc;

use clap::Parser;
use retina::{
    client::{PacketItem, SetupOptions},
    rtp::ReceivedPacket,
};
use tokio_stream::StreamExt;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing::{debug, error, info, trace, warn};
use webrtc::{
    Error as WebRTCError,
    api::{
        APIBuilder, interceptor_registry::register_default_interceptors, media_engine::MediaEngine,
    },
    interceptor::registry::Registry,
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::{TrackLocalWriter, track_local_static_rtp::TrackLocalStaticRTP},
};

use cli::Source;
use codec::{AUDIO_CODEC_PRIORITY, VIDEO_CODEC_PRIORITY, get_codec_priority};
use state::AppState;
use whep::{whep_delete, whep_offer};

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_level(true)
        .with_ansi(true)
        .init();

    info!("Starting RTSP to WebRTC server");

    let source = Source::parse();

    let mut session = {
        let creds = match (source.username, source.password) {
            (Some(user), pass) => Some(retina::client::Credentials {
                username: user,
                password: pass.unwrap_or_default(),
            }),

            _ => None,
        };

        let upstream_session_group = Arc::new(retina::client::SessionGroup::default());

        retina::client::Session::describe(
            source.url.into(),
            retina::client::SessionOptions::default()
                .creds(creds)
                .teardown(source.teardown)
                .session_group(upstream_session_group)
                .user_agent("RTSP to WebRTC example".to_owned()),
        )
        .await
        .unwrap()
    };

    let (video_track, audio_track) = {
        let mut available_video_streams = Vec::new();
        let mut available_audio_streams = Vec::new();

        for (index, stream) in session.streams().iter().enumerate() {
            if stream.media() == "video"
                && VIDEO_CODEC_PRIORITY
                    .iter()
                    .any(|(name, _)| *name == stream.encoding_name())
            {
                available_video_streams.push((index, stream));
            } else if stream.media() == "audio"
                && AUDIO_CODEC_PRIORITY
                    .iter()
                    .any(|(name, _)| *name == stream.encoding_name())
            {
                available_audio_streams.push((index, stream));
            }
        }

        if available_video_streams.is_empty() {
            error!("No supported video streams found (h264 required)");
            return;
        }

        // Sort video streams: first by resolution (higher is better), then by codec priority
        available_video_streams.sort_by(|(_, a), (_, b)| {
            use retina::codec::ParametersRef;

            let resolution_a = match a.parameters() {
                Some(ParametersRef::Video(v)) => {
                    let (w, h) = v.pixel_dimensions();
                    w * h
                }
                _ => 0,
            };

            let resolution_b = match b.parameters() {
                Some(ParametersRef::Video(v)) => {
                    let (w, h) = v.pixel_dimensions();
                    w * h
                }
                _ => 0,
            };

            // First compare by resolution (higher to lower)
            match resolution_b.cmp(&resolution_a) {
                std::cmp::Ordering::Equal => {
                    // If resolution is the same, compare by codec priority
                    get_codec_priority(a.encoding_name(), VIDEO_CODEC_PRIORITY)
                        .cmp(&get_codec_priority(b.encoding_name(), VIDEO_CODEC_PRIORITY))
                }
                other => other,
            }
        });

        // Sort audio streams by codec priority only
        available_audio_streams.sort_by(|(_, a), (_, b)| {
            get_codec_priority(a.encoding_name(), AUDIO_CODEC_PRIORITY)
                .cmp(&get_codec_priority(b.encoding_name(), AUDIO_CODEC_PRIORITY))
        });

        let video_track = {
            let video_stream = available_video_streams[0];
            {
                use retina::codec::ParametersRef;
                let (width, height) = match video_stream.1.parameters() {
                    Some(ParametersRef::Video(v)) => v.pixel_dimensions(),
                    _ => (0, 0),
                };
                info!(
                    "Selected video stream #{}: {} {}x{}",
                    video_stream.0,
                    video_stream.1.encoding_name(),
                    width,
                    height
                );
            }
            let track = TrackLocalStaticRTP::new(
                RTCRtpCodecCapability {
                    mime_type: format!("video/{}", video_stream.1.encoding_name()),
                    ..Default::default()
                },
                "video".to_owned(),
                "webrtc-rs".to_owned(),
            );
            (video_stream.0, Arc::new(track))
        };

        let audio_track = if !available_audio_streams.is_empty() {
            let audio_stream = available_audio_streams[0];
            info!(
                "Selected audio stream #{}: {}",
                audio_stream.0,
                audio_stream.1.encoding_name()
            );

            let track = TrackLocalStaticRTP::new(
                RTCRtpCodecCapability {
                    mime_type: format!("audio/{}", audio_stream.1.encoding_name()),
                    ..Default::default()
                },
                "audio".to_owned(),
                "webrtc-rs".to_owned(),
            );
            Some((audio_stream.0, Arc::new(track)))
        } else {
            None
        };
        (video_track, audio_track)
    };

    session
        .setup(
            video_track.0,
            SetupOptions::default().transport(source.transport.clone()),
        )
        .await
        .unwrap();

    if let Some(audio_stream) = audio_track.as_ref() {
        session
            .setup(
                audio_stream.0,
                SetupOptions::default().transport(source.transport),
            )
            .await
            .unwrap();
    }

    let api = {
        // Create a MediaEngine object to configure the supported codec
        let mut m = MediaEngine::default();

        m.register_default_codecs().unwrap();

        // Create a InterceptorRegistry. This is the user configurable RTP/RTCP Pipeline.
        // This provides NACKs, RTCP Reports and other features. If you use `webrtc.NewPeerConnection`
        // this is enabled by default. If you are manually managing You MUST create a InterceptorRegistry
        // for each PeerConnection.
        let mut registry = Registry::new();

        // Use the default set of Interceptors
        registry = register_default_interceptors(registry, &mut m).unwrap();

        // Create the API object with the MediaEngine
        APIBuilder::new()
            .with_media_engine(m)
            .with_interceptor_registry(registry)
            .build()
    };

    let mut session = session
        .play(retina::client::PlayOptions::default())
        .await
        .unwrap();

    let app_state = AppState::new(api, video_track, audio_track);

    {
        let cloned_app_state = app_state.clone();
        tokio::spawn(async move {
            // Create buffers for packets with channels
            let (video_tx, mut video_rx) = tokio::sync::mpsc::channel::<ReceivedPacket>(100);
            let (audio_tx, mut audio_rx) = tokio::sync::mpsc::channel::<ReceivedPacket>(100);

            // Task for writing video packets
            let video_track_clone = cloned_app_state.video_track.1.clone();
            tokio::spawn(async move {
                while let Some(rtp) = video_rx.recv().await {
                    if let Err(err) = video_track_clone.write(rtp.raw()).await {
                        if WebRTCError::ErrClosedPipe != err {
                            trace!("video_track write error: {}", err);
                        } else {
                            break;
                        }
                    }
                }
            });

            // Task for writing audio packets (if available)
            if let Some((_, audio_track)) = &cloned_app_state.audio_track {
                let audio_track_clone = audio_track.clone();
                tokio::spawn(async move {
                    while let Some(rtp) = audio_rx.recv().await {
                        if let Err(err) = audio_track_clone.write(rtp.raw()).await {
                            if WebRTCError::ErrClosedPipe != err {
                                trace!("audio_track write error: {}", err);
                            } else {
                                break;
                            }
                        }
                    }
                });
            }

            // Main loop for reading packets from RTSP
            while let Some(item) = session.next().await {
                match item {
                    Ok(PacketItem::Rtp(rtp)) => {
                        let stream_id = rtp.stream_id();

                        // Send packet to the corresponding channel without blocking
                        if stream_id == cloned_app_state.video_track.0 {
                            if video_tx.try_send(rtp).is_err() {
                                trace!("Video buffer full, dropping packet");
                            }
                        } else if let Some((audio_stream_id, _)) = &cloned_app_state.audio_track {
                            if stream_id == *audio_stream_id {
                                if audio_tx.try_send(rtp).is_err() {
                                    trace!("Audio buffer full, dropping packet");
                                }
                            } else {
                                warn!("Received RTP for unknown stream ID: {}", stream_id);
                            }
                        } else {
                            warn!("Received RTP for unknown stream ID: {}", stream_id);
                        }
                    }
                    Ok(PacketItem::Rtcp(rtcp)) => {
                        debug!(
                            "Received RTCP compound packet from stream {}",
                            rtcp.stream_id()
                        );
                        for pkt in rtcp.pkts() {
                            match pkt.as_typed() {
                                Ok(Some(retina::rtcp::TypedPacketRef::SenderReport(sr))) => {
                                    debug!(
                                        "  RTCP SR: ssrc={:#x}, ntp={}, rtp={}",
                                        sr.ssrc(),
                                        sr.ntp_timestamp().0,
                                        sr.rtp_timestamp()
                                    );
                                }
                                Ok(Some(retina::rtcp::TypedPacketRef::ReceiverReport(rr))) => {
                                    debug!("  RTCP RR: ssrc={:#x}", rr.ssrc());
                                }
                                Ok(Some(_)) => {
                                    debug!("  RTCP: other typed packet");
                                }
                                Ok(None) => {
                                    debug!("  RTCP: payload_type={}", pkt.payload_type());
                                }
                                Err(e) => {
                                    warn!("  RTCP parse error: {}", e);
                                }
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        error!("Error receiving packet: {:?}", e);
                    }
                }
            }
        });
    }

    // Configure CORS to allow requests from any origin
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = axum::Router::new()
        .route("/whep", axum::routing::post(whep_offer))
        .route("/whep/resource/{id}", axum::routing::delete(whep_delete))
        .fallback_service(tower_http::services::ServeDir::new("static"))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::http::Request<_>| {
                    tracing::info_span!(
                        "http_request",
                        method = %request.method(),
                        uri = %request.uri(),
                        version = ?request.version(),
                    )
                })
                .on_response(
                    |response: &axum::http::Response<_>,
                     latency: std::time::Duration,
                     _span: &tracing::Span| {
                        tracing::info!(
                            status = %response.status(),
                            latency_ms = %latency.as_millis(),
                            "response"
                        );
                    },
                ),
        )
        .layer(cors)
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();

    info!("🚀 WHEP server started on http://localhost:8080");
    info!("📡 POST SDP offers to http://localhost:8080/whep");
    info!("🗑️ DELETE sessions at http://localhost:8080/whep/resource/{{id}}");

    axum::serve(listener, app).await.unwrap();
}
