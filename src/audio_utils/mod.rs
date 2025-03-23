use std::sync::{Arc, Mutex};

use anyhow::Result;
use tokio::task::LocalSet;

use audio_to_ts::AudioToTs;
use ts_to_audio::TsToAudio;

pub mod audio_to_ts;
pub mod ts_to_audio;

/// The usual frame size.
///
/// Use 48 kHz, 20 ms frames (50 per second) and mono data (1 channel).
/// This means 1920 samples and 7.5 kiB.
const USUAL_FRAME_SIZE: usize = 48000 / 50;

/// The maximum size of an opus frame is 1275 as from RFC6716.
const MAX_OPUS_FRAME_SIZE: usize = 1275;

#[derive(Clone)]
pub struct AudioData {
	pub a2ts: Arc<Mutex<AudioToTs>>,
	pub ts2a: Arc<Mutex<TsToAudio>>,
}

///
/// audio_utils初始化参数
/// 	sdl2::init().unwrap()
/// 		初始化 SDL 库，如果失败则触发 panic!。
/// 
/// 	sdl_context.audio().unwrap()
/// 		获取 SDL 的音频子系统，如果失败则触发 panic!。
/// 
/// 	video_subsystem.enable_screen_saver()
/// 		启用屏幕保护程序（SDL 默认会禁用它）。
/// 
/// 	TsToAudio::new(...)
/// 		创建一个 TsToAudio 实例，用于处理从传输层到音频层的数据。
/// 
/// 	AudioToTs::new(...)
///			创建一个 AudioToTs 实例，用于处理从音频层到传输层的数据。
/// 
/// 	Ok(AudioData { a2ts, ts2a })
/// 		返回一个 AudioData 结构体，包含 a2ts 和 ts2a 两个实例。
/// 
pub(crate) fn start(local_set: &LocalSet) -> Result<AudioData> {

	// SDL about
	let sdl_context: sdl2::Sdl = sdl2::init().unwrap();
	let audio_subsystem: sdl2::AudioSubsystem = sdl_context.audio().unwrap();
	// SDL automatically disables the screensaver, enable it again
	if let Ok(video_subsystem) = sdl_context.video() {
		video_subsystem.enable_screen_saver();
	}

	let ts2a = TsToAudio::new(audio_subsystem.clone(), local_set)?;
	let a2ts = AudioToTs::new(audio_subsystem, local_set)?;
	
	// // No SDL mode
	// let ts2a = TsToAudio::new( local_set)?;
	// let a2ts = AudioToTs::new( local_set)?;

	Ok(AudioData { a2ts, ts2a })
}

pub(crate) fn start_nosdl(local_set: &LocalSet) -> Result<AudioData> {

	// SDL about
	let sdl_context: sdl2::Sdl = sdl2::init().unwrap();
	let audio_subsystem: sdl2::AudioSubsystem = sdl_context.audio().unwrap();
	// SDL automatically disables the screensaver, enable it again
	if let Ok(video_subsystem) = sdl_context.video() {
		video_subsystem.enable_screen_saver();
	}

	let ts2a = TsToAudio::new(audio_subsystem.clone(), local_set)?;
	let a2ts = AudioToTs::new(audio_subsystem, local_set)?;
	
	// // No SDL mode
	// let ts2a = TsToAudio::new( local_set)?;
	// let a2ts = AudioToTs::new( local_set)?;

	Ok(AudioData { a2ts, ts2a })
}
