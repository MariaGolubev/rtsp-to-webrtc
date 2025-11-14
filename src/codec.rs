// Video codec priorities (lower number = higher priority)
pub const VIDEO_CODEC_PRIORITY: &[(&str, u32)] =
    &[("h265", 1), ("h264", 2), ("vp9", 3), ("vp8", 4)];

// Audio codec priorities (lower number = higher priority)
pub const AUDIO_CODEC_PRIORITY: &[(&str, u32)] =
    &[("opus", 1), ("pcmu", 2), ("pcma", 3), ("g722", 4)];

// Get codec priority from the list
pub fn get_codec_priority(encoding_name: &str, priority_list: &[(&str, u32)]) -> u32 {
    priority_list
        .iter()
        .find(|(name, _)| *name == encoding_name)
        .map(|(_, priority)| *priority)
        .unwrap_or(100)
}
