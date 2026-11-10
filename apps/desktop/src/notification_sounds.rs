//! One bounded, lazy audio worker. Bundled cues never interrupt attachment playback.
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use model::notification_preferences::Sound;
use std::{
	io::Cursor,
	sync::{
		Arc,
		atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
		mpsc::{self, SyncSender},
	},
	time::{Duration, Instant},
};

// Each complete ringtone plus a short gap; shared with the automatic call timers.
pub const RING_INTERVAL: Duration = Duration::from_secs(6);
pub const OUTGOING_RING_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Default)]
pub struct Sounds {
	send: Option<SyncSender<(u64, Sound, u8)>>,
	generation: Arc<AtomicU64>,
	status: Arc<AtomicU8>,
}
impl Sounds {
	pub fn status(&self) -> &'static str {
		match self.status.load(Ordering::Acquire) {
			1 => "Playing notification sound...",
			2 => "Audio output unavailable. Check your system sound settings.",
			_ => "",
		}
	}
	/// The failure status alone, so a caller can prefer it over another worker's progress text.
	pub fn error(&self) -> &'static str {
		if self.status.load(Ordering::Acquire) == 2 {
			"Audio output unavailable. Check your system sound settings."
		} else {
			""
		}
	}
	pub fn stop(&mut self) {
		self.generation.fetch_add(1, Ordering::AcqRel);
		self.status.store(0, Ordering::Release);
	}
	pub fn play(&mut self, sound: Sound, volume: u8, ctx: &eframe::egui::Context) {
		if self.send.is_none() {
			let (send, receive) = mpsc::sync_channel::<(u64, Sound, u8)>(1);
			let generation = self.generation.clone();
			let status = self.status.clone();
			let context = ctx.clone();
			if std::thread::Builder::new()
				.name("bytwina-notification-audio".into())
				.spawn(move || {
					while let Ok((request, sound, volume)) = receive.recv() {
						if generation.load(Ordering::Acquire) != request {
							continue;
						}
						let finished = Arc::new(AtomicBool::new(false));
						match open(
							bundled(sound),
							volume,
							generation.clone(),
							request,
							status.clone(),
							finished.clone(),
						) {
							Ok((stream, duration)) => {
								let deadline = Instant::now() + duration + Duration::from_secs(2);
								while !finished.load(Ordering::Acquire) && Instant::now() < deadline
								{
									if generation.load(Ordering::Acquire) != request {
										break;
									}
									std::thread::sleep(Duration::from_millis(20));
								}
								drop(stream);
								if generation.load(Ordering::Acquire) == request {
									let _ = status.compare_exchange(
										1,
										if finished.load(Ordering::Acquire) {
											0
										} else {
											2
										},
										Ordering::AcqRel,
										Ordering::Acquire,
									);
								}
							}
							Err(()) => {
								if generation.load(Ordering::Acquire) == request {
									status.store(2, Ordering::Release);
								}
							}
						}
						context.request_repaint();
					}
				})
				.is_err()
			{
				self.status.store(2, Ordering::Release);
				return;
			}
			self.send = Some(send);
		}
		let request = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
		self.status.store(1, Ordering::Release);
		if self
			.send
			.as_ref()
			.expect("worker started")
			.try_send((request, sound, volume))
			.is_err()
		{
			self.status.store(0, Ordering::Release);
		}
	}
}
impl Drop for Sounds {
	fn drop(&mut self) {
		self.stop();
	}
}

/// The bundled cue bytes. Kept beside the decoder so both callers share one bound.
fn bundled(sound: Sound) -> &'static [u8] {
	match sound {
		Sound::Message => include_bytes!("../../../assets/sounds/discord/message.mp3"),
		Sound::CurrentChannel => {
			include_bytes!("../../../assets/sounds/discord/current-channel.mp3")
		}
		Sound::IncomingRing => include_bytes!("../../../assets/sounds/discord/incoming-ring.mp3"),
		Sound::OutgoingRing => include_bytes!("../../../assets/sounds/discord/outgoing-ring.mp3"),
		Sound::Mute => include_bytes!("../../../assets/sounds/discord/mute.mp3"),
		Sound::Unmute => include_bytes!("../../../assets/sounds/discord/unmute.mp3"),
		Sound::Deafen => include_bytes!("../../../assets/sounds/discord/deafen.mp3"),
		Sound::Undeafen => include_bytes!("../../../assets/sounds/discord/undeafen.mp3"),
		Sound::CameraOn => include_bytes!("../../../assets/sounds/discord/camera-on.mp3"),
		Sound::ScreenShareOn => {
			include_bytes!("../../../assets/sounds/discord/screen-share-on.mp3")
		}
		Sound::ScreenShareOff => {
			include_bytes!("../../../assets/sounds/discord/screen-share-off.mp3")
		}
		Sound::UserJoin => include_bytes!("../../../assets/sounds/discord/user-join.mp3"),
		Sound::UserLeave => include_bytes!("../../../assets/sounds/discord/user-leave.mp3"),
		Sound::PttStart => include_bytes!("../../../assets/sounds/discord/ptt-start.mp3"),
		Sound::PttStop => include_bytes!("../../../assets/sounds/discord/ptt-stop.mp3"),
		Sound::ViewerJoin => include_bytes!("../../../assets/sounds/discord/viewer-join.mp3"),
	}
}

/// Decode one short cue. Bundled cues are tiny; a soundboard clip may reach the service's 512 KiB.
pub(crate) const MAX_CUE_BYTES: usize = 512 * 1024;

fn append_pcm(
	pcm: &mut Vec<f32>,
	spec: &mut Option<(usize, u32)>,
	chunk: &[f32],
	channels: usize,
	rate: u32,
) -> Result<(), &'static str> {
	if !(1..=2).contains(&channels)
		|| !(8000..=96000).contains(&rate)
		|| spec.is_some_and(|spec| spec != (channels, rate))
		|| !chunk.len().is_multiple_of(channels)
		|| chunk.len() > (rate as usize * channels * 6).saturating_sub(pcm.len())
	{
		return Err("Invalid or oversized audio cue");
	}
	*spec = Some((channels, rate));
	pcm.extend(chunk.iter().map(|sample| {
		if sample.is_finite() {
			sample.clamp(-1.0, 1.0)
		} else {
			0.0
		}
	}));
	Ok(())
}

fn samples(bytes: &[u8], rate: u32, current: &impl Fn() -> bool) -> Result<Vec<[f32; 2]>, ()> {
	if !current()
		|| bytes.is_empty()
		|| bytes.len() > MAX_CUE_BYTES
		|| !(8000..=192000).contains(&rate)
	{
		return Err(());
	}
	let mut pcm = Vec::new();
	let mut spec = None;
	crate::audio::decode_stream(
		Box::new(Cursor::new(bytes.to_vec())),
		current,
		&mut |chunk, channels, rate, _| append_pcm(&mut pcm, &mut spec, chunk, channels, rate),
	)
	.map_err(|_| ())?;
	if !current() || pcm.is_empty() {
		return Err(());
	}
	let (channels, source_rate) = spec.ok_or(())?;
	let frames = pcm.len() / channels;
	let count = (frames as u64 * u64::from(rate)).div_ceil(u64::from(source_rate)) as usize;
	let mut stereo = Vec::with_capacity(count);
	// ponytail: linear rate conversion; use a band-limited resampler if quality measurements require it.
	for frame in 0..count {
		if frame % 1024 == 0 && !current() {
			return Err(());
		}
		let position = frame as f64 * f64::from(source_rate) / f64::from(rate);
		let index = (position as usize).min(frames - 1);
		let next = (index + 1).min(frames - 1);
		stereo.push(std::array::from_fn(|channel| {
			let channel = channel.min(channels - 1);
			let a = pcm[index * channels + channel];
			let b = pcm[next * channels + channel];
			a + (b - a) * (position - index as f64) as f32
		}));
	}
	if !current() {
		return Err(());
	}
	Ok(stereo)
}
/// Play one decoded cue on the default output. Bundled cues share one gate; a clip borrowed by
/// the attachment player passes its own, so replacing either never cuts the other off.
pub(crate) fn play_bytes(
	bytes: &[u8],
	volume: f32,
	gate: &CueGate,
) -> Result<(cpal::Stream, Duration), ()> {
	open(
		bytes,
		gain_percent(volume),
		gate.generation.clone(),
		gate.request,
		gate.status.clone(),
		gate.finished.clone(),
	)
}
/// The identity of one cue on the output, owned by whoever started it.
pub(crate) struct CueGate {
	generation: Arc<AtomicU64>,
	request: u64,
	status: Arc<AtomicU8>,
	finished: Arc<AtomicBool>,
}
impl CueGate {
	pub(crate) fn new(generation: Arc<AtomicU64>, request: u64) -> Self {
		Self {
			generation,
			request,
			status: Arc::new(AtomicU8::new(1)),
			finished: Arc::new(AtomicBool::new(false)),
		}
	}
	/// 1 while playing, 2 when the device failed, 0 once the final buffer drained.
	pub(crate) fn status(&self) -> u8 {
		self.status.load(Ordering::Acquire)
	}
	pub(crate) fn finished(&self) -> bool {
		self.finished.load(Ordering::Acquire)
	}
	pub(crate) fn cancelled(&self) -> bool {
		self.generation.load(Ordering::Acquire) != self.request
	}
}
fn gain_percent(volume: f32) -> u8 {
	if volume.is_finite() {
		(volume.clamp(0.0, 1.0) * 100.0).round() as u8
	} else {
		0
	}
}
/// Open bounded cue playback. Status is 1 while active, 2 on device failure; the caller marks 0
/// after draining. Bundled cues pass the shared gate; a borrowed clip passes its own.
fn open(
	bytes: &[u8],
	volume: u8,
	generation: Arc<AtomicU64>,
	request: u64,
	status: Arc<AtomicU8>,
	finished: Arc<AtomicBool>,
) -> Result<(cpal::Stream, Duration), ()> {
	if generation.load(Ordering::Acquire) != request
		|| bytes.is_empty()
		|| bytes.len() > MAX_CUE_BYTES
	{
		return Err(());
	}
	let device = cpal::default_host().default_output_device().ok_or(())?;
	let supported = platform::audio::output_config(
		&device,
		platform::audio::OutputConfigPreference::Notification,
	)
	.map_err(|_| ())?;
	let config = supported.config();
	if config.channels == 0 {
		return Err(());
	}
	let samples = samples(bytes, config.sample_rate, &|| {
		generation.load(Ordering::Acquire) == request
	})?;
	let duration = Duration::from_secs_f64(samples.len() as f64 / f64::from(config.sample_rate));
	let stream = match supported.sample_format() {
		cpal::SampleFormat::F32 => output::<f32>(
			&device,
			config,
			samples,
			volume,
			(generation, request),
			status,
			finished,
		),
		cpal::SampleFormat::I16 => output::<i16>(
			&device,
			config,
			samples,
			volume,
			(generation, request),
			status,
			finished,
		),
		cpal::SampleFormat::I32 => output::<i32>(
			&device,
			config,
			samples,
			volume,
			(generation, request),
			status,
			finished,
		),
		cpal::SampleFormat::U16 => output::<u16>(
			&device,
			config,
			samples,
			volume,
			(generation, request),
			status,
			finished,
		),
		_ => return Err(()),
	}
	.map_err(|_| ())?;
	stream.play().map_err(|_| ())?;
	Ok((stream, duration))
}
fn output<T: cpal::SizedSample + cpal::FromSample<f32>>(
	device: &cpal::Device,
	config: cpal::StreamConfig,
	samples: Vec<[f32; 2]>,
	volume: u8,
	(generation, request): (Arc<AtomicU64>, u64),
	status: Arc<AtomicU8>,
	finished: Arc<AtomicBool>,
) -> Result<cpal::Stream, cpal::Error> {
	let errors = generation.clone();
	let failed = finished.clone();
	device.build_output_stream(
		config,
		callback::<T>(config, samples, volume, generation, request, finished),
		move |_| {
			failed.store(true, Ordering::Release);
			if errors.load(Ordering::Acquire) == request {
				status.store(2, Ordering::Release);
			}
		},
		None,
	)
}
fn callback<T: cpal::SizedSample + cpal::FromSample<f32>>(
	config: cpal::StreamConfig,
	samples: Vec<[f32; 2]>,
	volume: u8,
	generation: Arc<AtomicU64>,
	request: u64,
	finished: Arc<AtomicBool>,
) -> impl FnMut(&mut [T], &cpal::OutputCallbackInfo) + Send + 'static {
	let mut position = 0;
	let mut end = None;
	let gain = (f32::from(volume) / 100.0).clamp(0.0, 1.0);
	move |data: &mut [T], info| {
		data.fill(T::from_sample(0.0));
		if generation.load(Ordering::Acquire) != request {
			return;
		}
		let timestamp = info.timestamp();
		if end.is_some_and(|end| timestamp.callback >= end) {
			finished.store(true, Ordering::Release);
			return;
		}
		for frame in data.chunks_exact_mut(usize::from(config.channels)) {
			let Some(sample) = samples.get(position) else {
				break;
			};
			if frame.len() == 1 {
				frame[0] = T::from_sample((sample[0] + sample[1]) * 0.5 * gain);
			} else {
				for (target, value) in frame.iter_mut().zip(sample) {
					*target = T::from_sample(*value * gain);
				}
			}
			position += 1;
		}
		if position == samples.len() && end.is_none() {
			// Wait until the final buffer reaches the device, including cold-start latency.
			end = timestamp.playback.checked_add(Duration::from_secs_f64(
				data.len() as f64 / f64::from(config.channels) / f64::from(config.sample_rate),
			));
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn wav(rate: u32, channels: u16, frames: u32) -> Vec<u8> {
		let size = frames * u32::from(channels) * 2;
		let mut bytes = b"RIFF".to_vec();
		bytes.extend_from_slice(&(36 + size).to_le_bytes());
		bytes.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0");
		bytes.extend_from_slice(&channels.to_le_bytes());
		bytes.extend_from_slice(&rate.to_le_bytes());
		bytes.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
		bytes.extend_from_slice(&(channels * 2).to_le_bytes());
		bytes.extend_from_slice(b"\x10\0data");
		bytes.extend_from_slice(&size.to_le_bytes());
		for _ in 0..frames {
			for channel in 0..channels {
				bytes.extend_from_slice(
					&if channel == 0 { 8192i16 } else { -16384i16 }.to_le_bytes(),
				);
			}
		}
		bytes
	}

	#[test]
	fn mono_and_stereo_sources_resample_across_rate_boundaries() {
		for rate in [8000, 24000, 44100, 48000, 96000] {
			for channels in [1, 2] {
				for output in [8000, 192000] {
					let pcm = samples(&wav(rate, channels, 128), output, &|| true).unwrap();
					assert_eq!(pcm.len(), (128 * output as usize).div_ceil(rate as usize));
					let expected = if channels == 1 {
						[0.25, 0.25]
					} else {
						[0.25, -0.5]
					};
					assert!(pcm.iter().all(|frame| *frame == expected));
				}
			}
		}
		for bytes in [
			include_bytes!("../tests/fixtures/audio-tone.mp3").as_slice(),
			include_bytes!("../tests/fixtures/voice-message.ogg").as_slice(),
		] {
			let pcm = samples(bytes, 48000, &|| true).unwrap();
			assert!(!pcm.is_empty() && pcm.len() <= 48000 * 6);
			assert!(pcm.iter().all(|frame| frame[0] == frame[1]));
			assert!(pcm.iter().any(|frame| frame[0].abs() > 0.01));
		}
		for rate in [7999, 96001] {
			assert!(samples(&wav(rate, 1, 128), 48000, &|| true).is_err());
		}
		for rate in [7999, 192001] {
			assert!(samples(&wav(8000, 1, 128), rate, &|| true).is_err());
		}
	}

	#[test]
	fn cue_duration_bytes_and_mid_decode_cancellation_are_bounded() {
		let wav = wav(8000, 1, 48000);
		assert_eq!(samples(&wav, 192000, &|| true).unwrap().len(), 192000 * 6);
		assert!(samples(&self::wav(8000, 1, 48001), 8000, &|| true).is_err());
		assert!(samples(&[], 8000, &|| true).is_err());
		let checks = std::cell::Cell::new(0);
		assert!(
			samples(&wav, 48000, &|| {
				checks.set(checks.get() + 1);
				checks.get() < 4
			})
			.is_err()
		);
		assert_eq!(checks.get(), 4);

		let mut padded = wav;
		let padding = MAX_CUE_BYTES - padded.len();
		let mut junk = b"JUNK".to_vec();
		junk.extend_from_slice(&((padding - 8) as u32).to_le_bytes());
		junk.resize(padding, 0);
		padded.splice(12..12, junk);
		padded[4..8].copy_from_slice(&((MAX_CUE_BYTES - 8) as u32).to_le_bytes());
		assert_eq!(samples(&padded, 8000, &|| true).unwrap().len(), 48000);
		padded.push(0);
		assert!(samples(&padded, 8000, &|| true).is_err());
	}

	#[test]
	fn pcm_packets_reject_format_changes_and_partial_frames() {
		let mut pcm = Vec::new();
		let mut spec = None;
		append_pcm(&mut pcm, &mut spec, &[f32::NAN, 2.0, -2.0], 1, 24000).unwrap();
		assert_eq!(pcm, [0.0, 1.0, -1.0]);
		for (chunk, channels, rate) in [
			(&[0.0, 0.0][..], 2, 24000),
			(&[0.0][..], 1, 48000),
			(&[0.0][..], 0, 24000),
		] {
			assert!(append_pcm(&mut pcm, &mut spec, chunk, channels, rate).is_err());
		}
		assert_eq!(pcm, [0.0, 1.0, -1.0]);
		assert!(append_pcm(&mut Vec::new(), &mut None, &[0.0], 2, 8000).is_err());
	}

	#[test]
	fn callback_drains_delayed_output_and_cancellation_is_request_local() {
		let config = cpal::StreamConfig {
			channels: 4,
			sample_rate: 8000,
			buffer_size: cpal::BufferSize::Default,
		};
		let generation = Arc::new(AtomicU64::new(1));
		let finished = Arc::new(AtomicBool::new(false));
		let mut render = callback(
			config,
			vec![[0.2, 0.4]; 2],
			100,
			generation.clone(),
			1,
			finished.clone(),
		);
		let info = |callback_ms, playback_ms| {
			cpal::OutputCallbackInfo::new(cpal::OutputStreamTimestamp {
				callback: cpal::StreamInstant::from_millis(callback_ms),
				playback: cpal::StreamInstant::from_millis(playback_ms),
			})
		};
		let mut data = [1.0_f32; 8];
		render(&mut data, &info(0, 1000));
		assert_eq!(data, [0.2, 0.4, 0.0, 0.0, 0.2, 0.4, 0.0, 0.0]);
		render(&mut data, &info(500, 1500));
		assert_eq!(data, [0.0; 8]);
		assert!(!finished.load(Ordering::Acquire));
		render(&mut data, &info(1001, 2001));
		assert!(finished.load(Ordering::Acquire));

		let next_finished = Arc::new(AtomicBool::new(false));
		let mut next = callback(
			cpal::StreamConfig {
				channels: 1,
				..config
			},
			vec![[0.2, 0.4]; 2],
			100,
			generation.clone(),
			2,
			next_finished.clone(),
		);
		generation.store(2, Ordering::Release);
		let mut cancelled = callback(
			config,
			vec![[1.0, 1.0]; 2],
			100,
			generation.clone(),
			1,
			finished,
		);
		cancelled(&mut data, &info(2000, 3000));
		assert_eq!(data, [0.0; 8]);
		assert!(!next_finished.load(Ordering::Acquire));
		next(&mut data[..2], &info(2000, 3000));
		assert_eq!(&data[..2], &[0.3, 0.3]);
	}

	#[test]
	fn bundled_cues_decode_in_full_at_supported_rates_and_cancel() {
		for rate in [8000, 44100, 48000, 192000] {
			let cues = [
				Sound::Message,
				Sound::CurrentChannel,
				Sound::IncomingRing,
				Sound::Mute,
				Sound::Unmute,
				Sound::Deafen,
				Sound::Undeafen,
				Sound::CameraOn,
				Sound::ScreenShareOn,
				Sound::ScreenShareOff,
				Sound::OutgoingRing,
				Sound::UserJoin,
				Sound::UserLeave,
				Sound::PttStart,
				Sound::PttStop,
				Sound::ViewerJoin,
			]
			.map(|s| samples(bundled(s), rate, &|| true).unwrap());
			assert_ne!(cues[0], cues[1]);
			assert_ne!(cues[3], cues[4]);
			assert_ne!(cues[5], cues[6]);
			assert_ne!(cues[8], cues[9]);
			assert_ne!(cues[13], cues[14]);
			let expectations = [
				(0.2, 0.5),
				(0.5, 0.9),
				(5.0, 5.6),
				(0.3, 0.6),
				(0.3, 0.6),
				(0.6, 0.9),
				(0.6, 1.0),
				(0.9, 1.1),
				(1.6, 1.9),
				(1.5, 1.8),
				(2.3, 2.6),
				(1.0, 1.2),
				(0.9, 1.1),
				(0.02, 0.25),
				(0.02, 0.25),
				(1.5, 1.8),
			];
			assert!(
				Duration::from_secs_f64(cues[10].len() as f64 / f64::from(rate))
					+ Duration::from_millis(100)
					< OUTGOING_RING_INTERVAL
			);
			for (index, (cue, (min, max))) in cues.iter().zip(expectations).enumerate() {
				let seconds = cue.len() as f64 / f64::from(rate);
				assert!(
					(min..max).contains(&seconds),
					"unexpected cue {index} duration {seconds} at {rate} Hz"
				);
				assert!(cue.len() <= rate as usize * 6);
				assert!(
					cue.iter()
						.flatten()
						.all(|s| s.is_finite() && s.abs() <= 1.0)
				);
				assert!(cue.iter().flatten().any(|s| s.abs() > 0.01));
				assert!(
					Duration::from_secs_f64(seconds) + Duration::from_millis(100) < RING_INTERVAL
				);
			}
		}
		assert!(samples(bundled(Sound::Message), 48000, &|| false).is_err());
		assert!(samples(bundled(Sound::Message), 0, &|| true).is_err());
	}
}
