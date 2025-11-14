use clap::Parser;

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

#[derive(Parser)]
pub struct Source {
    /// `rtsp://` URL to connect to.
    #[clap(long)]
    pub url: RTSPUrl,

    /// Username to send if the server requires authentication.
    #[clap(long)]
    pub username: Option<String>,

    /// Password; requires username.
    #[clap(long, requires = "username")]
    pub password: Option<String>,

    /// When to issue a `TEARDOWN` request: `auto`, `always`, or `never`.
    #[arg(default_value_t, long)]
    pub teardown: retina::client::TeardownPolicy,

    /// The transport to use: `tcp` or `udp` (experimental).
    #[arg(default_value_t, long)]
    pub transport: retina::client::Transport,
}
