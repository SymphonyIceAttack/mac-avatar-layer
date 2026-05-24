#![cfg(target_os = "macos")]

use crate::config::MicrophoneConfig;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat, Stream};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

pub struct MicrophoneInput {
    level_bits: Arc<AtomicU32>,
    _stream: Stream,
}

impl MicrophoneInput {
    pub fn from_config(config: &MicrophoneConfig) -> Option<Self> {
        if !config.enabled {
            return None;
        }

        match Self::new() {
            Ok(input) => Some(input),
            Err(error) => {
                eprintln!("{}", microphone_startup_failure_message(&error));
                None
            }
        }
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level_bits.load(Ordering::Relaxed)).clamp(0.0, 1.0)
    }

    fn new() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| "No default input device is available".to_string())?;
        let config = device
            .default_input_config()
            .map_err(|error| format!("Failed to read default input config: {error}"))?;
        let level_bits = Arc::new(AtomicU32::new(0.0_f32.to_bits()));

        let stream = match config.sample_format() {
            SampleFormat::F32 => build_stream::<f32>(&device, &config.into(), &level_bits),
            SampleFormat::I16 => build_stream::<i16>(&device, &config.into(), &level_bits),
            SampleFormat::U16 => build_stream::<u16>(&device, &config.into(), &level_bits),
            format => Err(format!("Unsupported microphone sample format: {format:?}")),
        }?;

        stream
            .play()
            .map_err(|error| format!("Failed to start microphone stream: {error}"))?;
        println!("Microphone input enabled: driving ParamMouthOpenY from RMS volume");

        Ok(Self {
            level_bits,
            _stream: stream,
        })
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    level_bits: &Arc<AtomicU32>,
) -> Result<Stream, String>
where
    T: cpal::Sample + cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let channels = config.channels.max(1) as usize;
    let level_bits = Arc::clone(level_bits);
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let level = rms_level(data, channels);
                level_bits.store(level.to_bits(), Ordering::Relaxed);
            },
            |error| eprintln!("{}", microphone_stream_error_message(&error.to_string())),
            None,
        )
        .map_err(|error| format!("Failed to build microphone stream: {error}"))
}

fn microphone_startup_failure_message(error: &str) -> String {
    format!(
        "Microphone input is enabled but could not start.\n\
         Cause: {error}\n\
         macOS: open System Settings > Privacy & Security > Microphone, then allow the terminal, Codex, or packaged app that launches MacAvatarLayer.\n\
         To keep the avatar running without microphone input, set [input.microphone].enabled = false in the active profile config.\n\
         Development config: mac-avatar-layer.dev.toml\n\
         Build config: mac-avatar-layer.build.toml"
    )
}

fn microphone_stream_error_message(error: &str) -> String {
    format!(
        "Microphone stream error: {error}\n\
         If mouth tracking stops, check macOS Microphone permission or disable [input.microphone] in the active profile config."
    )
}

fn rms_level<T>(data: &[T], channels: usize) -> f32
where
    T: cpal::Sample,
    f32: cpal::FromSample<T>,
{
    if data.is_empty() {
        return 0.0;
    }

    let frame_count = (data.len() / channels).max(1);
    let mut sum = 0.0_f32;
    for frame in data.chunks(channels) {
        let mixed = frame
            .iter()
            .map(|sample| f32::from_sample(*sample))
            .sum::<f32>()
            / frame.len().max(1) as f32;
        sum += mixed * mixed;
    }

    (sum / frame_count as f32).sqrt().clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_failure_message_points_to_macos_permission_and_configs() {
        let message = microphone_startup_failure_message("permission denied");

        assert!(message.contains("permission denied"));
        assert!(message.contains("System Settings > Privacy & Security > Microphone"));
        assert!(message.contains("[input.microphone].enabled = false"));
        assert!(message.contains("mac-avatar-layer.dev.toml"));
        assert!(message.contains("mac-avatar-layer.build.toml"));
    }

    #[test]
    fn stream_error_message_mentions_permission_or_disable_path() {
        let message = microphone_stream_error_message("device removed");

        assert!(message.contains("device removed"));
        assert!(message.contains("macOS Microphone permission"));
        assert!(message.contains("[input.microphone]"));
    }
}
