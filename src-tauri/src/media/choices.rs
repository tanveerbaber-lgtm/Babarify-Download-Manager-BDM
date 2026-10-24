//! The short list of choices the on-video panel offers, built from an analysis.

use super::analyze::MediaInfo;
use crate::browser::protocol::{MediaChoice, MediaChoices};
use crate::models::{AudioFormat, Container, MediaOptions};

/// Most video qualities shown; the rest stay available in BDM's dialog.
const MAX_VIDEO_CHOICES: usize = 8;

/// Choices for one video, or a message explaining why there are none.
pub fn media_choices(info: &MediaInfo) -> Result<MediaChoices, String> {
    if info.drm_protected {
        return Err("This video is protected by DRM. BDM doesn't download DRM-protected content.".into());
    }
    if info.is_live {
        return Err("Live streams aren't supported yet.".into());
    }
    if info.kind == "playlist" {
        return Err("This page is a playlist. Use the BDM button in the toolbar to choose videos.".into());
    }
    if info.formats.is_empty() {
        return Err("BDM found no downloadable video on this page.".into());
    }

    let mut choices: Vec<MediaChoice> = info
        .qualities
        .iter()
        .take(MAX_VIDEO_CHOICES)
        .map(|q| MediaChoice {
            label: match q.fps {
                Some(fps) if fps > 30.0 => format!("MP4 · {} {}fps", q.label, fps.round()),
                _ => format!("MP4 · {}", q.label),
            },
            size_bytes: q.estimated_bytes,
            kind: "video".into(),
            options: MediaOptions::Video { max_height: Some(q.height), container: Container::Mp4 },
        })
        .collect();

    // Sites that don't report heights still get a "best" option.
    if choices.is_empty() && info.formats.iter().any(|f| f.content != "audio") {
        choices.push(MediaChoice {
            label: "MP4 · Best Quality".into(),
            size_bytes: None,
            kind: "video".into(),
            options: MediaOptions::Video { max_height: None, container: Container::Mp4 },
        });
    }

    if info.audio.is_some() || info.formats.iter().any(|f| f.content != "video") {
        let size = info.audio.as_ref().and_then(|a| a.estimated_bytes);
        for (label, format) in [("MP3 · Audio Only", AudioFormat::Mp3), ("M4A · Audio Only", AudioFormat::M4a)] {
            choices.push(MediaChoice {
                label: label.into(),
                size_bytes: size,
                kind: "audio".into(),
                options: MediaOptions::Audio { format },
            });
        }
    }

    if choices.is_empty() {
        return Err("BDM found no downloadable video on this page.".into());
    }
    Ok(MediaChoices {
        title: info.title.clone(),
        site: info.site.clone(),
        duration: info.duration,
        license: info.license.clone(),
        choices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::analyze::parse;

    fn fixture(name: &str) -> serde_json::Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testsupport/fixtures").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn lists_video_qualities_then_audio() {
        let info = parse(&fixture("video.json"), "https://www.youtube.com/watch?v=aqz-KE-bpKQ").unwrap();
        let c = media_choices(&info).unwrap();
        assert_eq!(c.license.as_deref(), Some("Creative Commons Attribution license (reuse allowed)"));
        let labels: Vec<&str> = c.choices.iter().map(|x| x.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "MP4 · 2160p (4K) 60fps",
                "MP4 · 1440p (2K) 60fps",
                "MP4 · 1080p 60fps",
                "MP4 · 720p 60fps",
                "MP4 · 360p",
                "MP3 · Audio Only",
                "M4A · Audio Only",
            ]
        );
        assert_eq!(c.choices[2].options, MediaOptions::Video { max_height: Some(1080), container: Container::Mp4 });
        assert_eq!(c.choices[2].size_bytes, Some(257_619_653 + 10_271_496));
        assert_eq!(c.choices[5].options, MediaOptions::Audio { format: AudioFormat::Mp3 });
    }

    #[test]
    fn explains_when_nothing_can_be_offered() {
        let drm = serde_json::json!({"_type": "video", "title": "x", "formats": [
            {"format_id": "a", "vcodec": "avc1", "acodec": "none", "height": 720, "has_drm": true}
        ]});
        assert!(media_choices(&parse(&drm, "u").unwrap()).unwrap_err().contains("DRM"));
        let playlist = parse(&fixture("playlist.json"), "u").unwrap();
        assert!(media_choices(&playlist).unwrap_err().contains("playlist"));
    }

    #[test]
    fn plain_media_urls_get_a_best_option() {
        let json = serde_json::json!({"_type": "video", "title": "clip", "url": "https://cdn/x.mp4", "ext": "mp4", "format_id": "0"});
        let c = media_choices(&parse(&json, "u").unwrap()).unwrap();
        assert_eq!(c.choices[0].label, "MP4 · Best Quality");
        assert_eq!(c.choices.len(), 3, "best video + MP3 + M4A");
    }
}
