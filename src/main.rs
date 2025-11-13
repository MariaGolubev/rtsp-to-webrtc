use std::sync::Arc;

use axum::{
    extract::{FromRequest, State},
    response::IntoResponse,
};
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
use tracing::{error, info, trace, warn};
use webrtc::{
    Error as WebRTCError,
    api::{
        API, APIBuilder, interceptor_registry::register_default_interceptors,
        media_engine::MediaEngine,
    },
    interceptor::registry::Registry,
    peer_connection::sdp::session_description::RTCSessionDescription,
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::{
        TrackLocal, TrackLocalWriter, track_local_static_rtp::TrackLocalStaticRTP,
    },
};

mod rtsp_url {

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct RTSPUrl(pub url::Url);

    impl std::ops::Deref for RTSPUrl {
        type Target = url::Url;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl From<RTSPUrl> for url::Url {
        fn from(rtsp_url: RTSPUrl) -> Self {
            rtsp_url.0
        }
    }

    #[derive(Debug, thiserror::Error, PartialEq, Eq)]
    pub enum RTSPUrlParseError {
        #[error("invalid scheme, expected 'rtsp'")]
        InvalidScheme,
        #[error(transparent)]
        UrlParseError(#[from] url::ParseError),
    }

    impl std::str::FromStr for RTSPUrl {
        type Err = RTSPUrlParseError;

        fn from_str(s: &str) -> Result<Self, Self::Err> {
            let url = s.parse::<url::Url>()?;
            if url.scheme() != "rtsp" {
                return Err(RTSPUrlParseError::InvalidScheme);
            }
            Ok(RTSPUrl(url))
        }
    }

    impl std::fmt::Display for RTSPUrl {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }
}

#[derive(Parser)]
struct Source {
    /// `rtsp://` URL to connect to.
    #[clap(long)]
    url: rtsp_url::RTSPUrl,

    /// Username to send if the server requires authentication.
    #[clap(long)]
    username: Option<String>,

    /// Password; requires username.
    #[clap(long, requires = "username")]
    password: Option<String>,

    /// When to issue a `TEARDOWN` request: `auto`, `always`, or `never`.
    #[arg(default_value_t, long)]
    teardown: retina::client::TeardownPolicy,

    /// The transport to use: `tcp` or `udp` (experimental).
    #[arg(default_value_t, long)]
    transport: retina::client::Transport,
}

#[derive(Clone)]
struct AppState {
    pub api: Arc<API>,
    pub peer_connections: dashmap::DashMap<String, Arc<webrtc::peer_connection::RTCPeerConnection>>,
    pub video_track: (usize, Arc<TrackLocalStaticRTP>),
    pub audio_track: Option<(usize, Arc<TrackLocalStaticRTP>)>,
}

impl AppState {
    pub fn new(
        api: API,
        video_track: (usize, Arc<TrackLocalStaticRTP>),
        audio_track: Option<(usize, Arc<TrackLocalStaticRTP>)>,
    ) -> Self {
        Self {
            api: Arc::new(api),
            peer_connections: dashmap::DashMap::new(),
            video_track,
            audio_track,
        }
    }
}

// Video codec priorities (lower number = higher priority)
const VIDEO_CODEC_PRIORITY: &[(&str, u32)] = &[("h265", 1), ("h264", 2), ("vp9", 3), ("vp8", 4)];

// Audio codec priorities (lower number = higher priority)
const AUDIO_CODEC_PRIORITY: &[(&str, u32)] = &[("opus", 1), ("pcmu", 2), ("pcma", 3)];

// Get codec priority from the list
fn get_codec_priority(encoding_name: &str, priority_list: &[(&str, u32)]) -> u32 {
    priority_list
        .iter()
        .find(|(name, _)| *name == encoding_name)
        .map(|(_, priority)| *priority)
        .unwrap_or(100)
}

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
                        trace!("Received RTCP packet: {:?}", rtcp);
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

struct SDPOffer(pub RTCSessionDescription);

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

struct SDPAnswer(pub RTCSessionDescription, String);

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

async fn whep_offer(
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
            let mut rtcp_buf = vec![0u8; 1500];
            while let Ok((_, _)) = rtp_audio_sender.read(&mut rtcp_buf).await {}
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

async fn whep_delete(
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
