//! Video and audio downloads through yt-dlp and FFmpeg.

pub mod analyze;
pub mod args;
pub mod choices;
pub mod output;
pub mod process;
pub mod tools;

#[cfg(test)]
mod tests;
