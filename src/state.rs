use std::sync::Arc;
use webrtc::{api::API, track::track_local::track_local_static_rtp::TrackLocalStaticRTP};

#[derive(Clone)]
pub struct AppState {
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
