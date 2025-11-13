use std::sync::Arc;

use axum::{
    extract::{FromRequest, State},
    response::IntoResponse,
};
// use clap::Parser;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing::{error, info, trace, warn};
// use retina::{client::SetupOptions, codec::CodecItem};
// use tokio_stream::StreamExt;
use webrtc::{
    Error as WebRTCError,
    api::{
        API, APIBuilder,
        interceptor_registry::register_default_interceptors,
        media_engine::{MIME_TYPE_VP8, MediaEngine},
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

// #[derive(Parser)]
// struct Source {
// /// `rtsp://` URL to connect to.
// #[clap(long)]
// url: rtsp_url::RTSPUrl,

// /// Username to send if the server requires authentication.
// #[clap(long)]
// username: Option<String>,

// /// Password; requires username.
// #[clap(long, requires = "username")]
// password: Option<String>,

// /// When to issue a `TEARDOWN` request: `auto`, `always`, or `never`.
// #[arg(default_value_t, long)]
// teardown: retina::client::TeardownPolicy,

// /// The transport to use: `tcp` or `udp` (experimental).
// #[arg(default_value_t, long)]
// transport: retina::client::Transport,
// }

#[derive(Clone)]
struct AppState {
    pub api: Arc<API>,
    pub peer_connections: dashmap::DashMap<String, Arc<webrtc::peer_connection::RTCPeerConnection>>,
    pub video_tracks: Arc<dashmap::DashMap<String, Arc<TrackLocalStaticRTP>>>,
}

impl AppState {
    pub fn new(api: API) -> Self {
        Self {
            api: Arc::new(api),
            peer_connections: dashmap::DashMap::new(),
            video_tracks: Arc::new(dashmap::DashMap::new()),
        }
    }
}

#[tokio::main]
async fn main() {
    // Инициализация tracing
    tracing_subscriber::fmt()
        .with_level(true)
        .with_ansi(true)
        .init();

    info!("Starting RTSP to WebRTC server");

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

    let app_state = AppState::new(api);

    // Запускаем UDP слушатель для приёма RTP пакетов
    let video_tracks = Arc::clone(&app_state.video_tracks);
    tokio::spawn(async move {
        let udp_listener = tokio::net::UdpSocket::bind("127.0.0.1:5004")
            .await
            .expect("Failed to bind UDP socket on port 5004");

        info!("UDP listener started on 127.0.0.1:5004");
        info!("Send RTP packets to this address using GStreamer or ffmpeg");

        let mut inbound_rtp_packet = vec![0u8; 1600]; // UDP MTU
        let mut packet_count = 0u64;
        loop {
            match udp_listener.recv_from(&mut inbound_rtp_packet).await {
                Ok((n, source)) => {
                    packet_count += 1;
                    if packet_count % 100 == 0 {
                        trace!(
                            "Received {} RTP packets (last from {}), broadcasting to {} tracks",
                            packet_count,
                            source,
                            video_tracks.len()
                        );
                    }

                    // Отправляем пакет всем активным video tracks
                    for entry in video_tracks.iter() {
                        let track = entry.value();
                        if let Err(err) = track.write(&inbound_rtp_packet[..n]).await {
                            if WebRTCError::ErrClosedPipe != err {
                                error!("video_track write error: {}", err);
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("UDP recv error: {}", e);
                    break;
                }
            }
        }
    });

    // Настройка CORS для разрешения запросов с любых источников
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = axum::Router::new()
        .route("/whep", axum::routing::post(whep_offer))
        .route("/whep/resource/{id}", axum::routing::delete(whep_delete))
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

    let listener = tokio::net::TcpListener::bind("localhost:8080")
        .await
        .unwrap();

    info!("🚀 WHEP server started on http://localhost:8080");
    info!("📡 POST SDP offers to http://localhost:8080/whep");
    info!("🗑️ DELETE sessions at http://localhost:8080/whep/resource/{{id}}");

    axum::serve(listener, app).await.unwrap();
}

struct SDPOffer(pub RTCSessionDescription);

impl FromRequest<AppState> for SDPOffer {
    type Rejection = axum::http::StatusCode;

    async fn from_request(
        req: axum::http::Request<axum::body::Body>,
        _state: &AppState,
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
        video_tracks,
    }): State<AppState>,
    SDPOffer(offer): SDPOffer,
) -> SDPAnswer {
    let pc = api
        .new_peer_connection(webrtc::peer_connection::configuration::RTCConfiguration::default())
        .await
        .unwrap();

    let pc = Arc::new(pc);

    // Создаём video track для отправки видео через WebRTC
    let video_track = Arc::new(TrackLocalStaticRTP::new(
        RTCRtpCodecCapability {
            mime_type: MIME_TYPE_VP8.to_owned(),
            ..Default::default()
        },
        "video".to_owned(),
        "webrtc-rs".to_owned(),
    ));

    // Добавляем track в PeerConnection
    let rtp_sender = pc
        .add_track(Arc::clone(&video_track) as Arc<dyn TrackLocal + Send + Sync>)
        .await
        .unwrap();

    // Читаем входящие RTCP пакеты (необходимо для NACK и других функций)
    tokio::spawn(async move {
        let mut rtcp_buf = vec![0u8; 1500];
        while let Ok((_, _)) = rtp_sender.read(&mut rtcp_buf).await {}
    });

    let id = uuid::Uuid::new_v4().to_string();

    // Добавляем track в список активных треков
    video_tracks.insert(id.clone(), Arc::clone(&video_track));

    // Устанавливаем обработчик изменения состояния соединения
    let id_for_handler = id.clone();
    let peer_connections_clone = peer_connections.clone();
    let video_tracks_clone = Arc::clone(&video_tracks);
    pc.on_peer_connection_state_change(Box::new(move |state| {
        let id = id_for_handler.clone();
        let peer_connections = peer_connections_clone.clone();
        let video_tracks = video_tracks_clone.clone();

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
                    video_tracks.remove(&id);

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
        "✅ Session created: {} | Sessions: {} | Tracks: {}",
        &id[..8],
        peer_connections.len(),
        video_tracks.len()
    );

    SDPAnswer(answer, id)
}

async fn whep_delete(
    State(AppState {
        peer_connections,
        video_tracks,
        ..
    }): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> axum::http::StatusCode {
    if let Some((_, pc)) = peer_connections.remove(&id) {
        pc.close().await.unwrap();
        // Удаляем video track из списка активных треков
        video_tracks.remove(&id);

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

// let source = Source::parse();

// {
//     let creds = match (source.username, source.password) {
//         (Some(user), pass) => Some(retina::client::Credentials {
//             username: user,
//             password: pass.unwrap_or_default(),
//         }),

//         _ => None,
//     };

//     let upstream_session_group = Arc::new(retina::client::SessionGroup::default());

//     let _session = retina::client::Session::describe(
//         source.url.into(),
//         retina::client::SessionOptions::default()
//             .creds(creds)
//             .teardown(source.teardown)
//             .session_group(upstream_session_group)
//             .user_agent("RTSP to WebRTC example".to_owned()),
//     )
//     .await
//     .unwrap();
// }

// let stream_i = session
//     .streams()
//     .iter()
//     .position(|s| s.media() == "video" && s.encoding_name() == "h264")
//     .unwrap();

// session
//     .setup(
//         stream_i,
//         SetupOptions::default().transport(source.transport),
//     )
//     .await
//     .unwrap();

// let mut session = session
//     .play(retina::client::PlayOptions::default())
//     .await
//     .unwrap()
//     .demuxed()
//     .unwrap();

// while let Some(item) = session.next().await {
//     match item {
//         Ok(CodecItem::VideoFrame(video_frame)) => {
//             println!("Received video frame: {:#?}", video_frame);
//         }
//         Ok(CodecItem::AudioFrame(audio_frame)) => {
//             println!("Received audio frame: {:#?}", audio_frame);
//         }
//         Ok(CodecItem::MessageFrame(message_frame)) => {
//             println!(
//                 "Received message frame: timestamp={}, size={}",
//                 message_frame.timestamp(),
//                 message_frame.data().len()
//             );
//         }
//         Ok(CodecItem::Rtcp(rtcp)) => {
//             println!("Received RTCP packet: {:?}", rtcp);
//         }
//         Ok(_) => unimplemented!(),
//         Err(e) => {
//             eprintln!("Error receiving packet: {:?}", e);
//         }
//     }
// }
