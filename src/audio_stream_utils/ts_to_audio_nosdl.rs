use std::sync::{Arc, Mutex};

use anyhow::{format_err, Result};
use futures::prelude::*;
use sdl2::audio::{AudioCallback, AudioDevice, AudioSpecDesired, AudioStatus};
use sdl2::AudioSubsystem;
use tokio::task::LocalSet;
use tokio::time::{self, Duration};
use tokio_stream::wrappers::IntervalStream;
use tracing::{debug, error, instrument};
use tsclientlib::ClientId;
use tsproto_packets::packets::InAudioBuf;

use super::*;
use crate::ConnectionId;

type Id = (ConnectionId, ClientId);
type AudioHandler = tsclientlib::audio::AudioHandler<Id>;

use std::fs::OpenOptions;
use std::io::{self, Write};

pub struct TsToAudio {
	audio_subsystem: AudioSubsystem,
	device: AudioDevice<SdlCallback>,
	data: Arc<Mutex<AudioHandler>>,    
	buffer_i16: Arc<Mutex<Vec<i16>>>,
}

struct SdlCallback {
	data: Arc<Mutex<AudioHandler>>,
    buffer_i16: Arc<Mutex<Vec<i16>>>, 
}

fn vec_f32_to_i16_linear(buffer: &mut [f32]) -> Vec<i16> {
    buffer
        .iter_mut()
        .map(|x| {
            let scaled = *x * 65535.0 * 1.6;
            // let scaled = *x * 32767.5 + 32767.5;
            scaled.clamp(-65535.0, 65535.0) as i16
        })
        .collect()
}


fn write_vec_i16_to_file(data: &[i16], filename: &str) -> io::Result<()> {
    // 打开文件（追加模式，如果文件不存在则创建）
    let mut file = OpenOptions::new()
        .write(true)
        .append(true)
        .create(true)
        .open(filename)?;

    // 将 Vec<i16> 转换为字节数组
    let bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(
            data.as_ptr() as *const u8,
            data.len() * std::mem::size_of::<i16>(),
        )
    };

    // 将字节数组写入文件
    file.write_all(bytes)?;

    Ok(())
}

impl TsToAudio {
	pub fn new(audio_subsystem: AudioSubsystem, local_set: &LocalSet) -> Result<Arc<Mutex<Self>>> {
		let data = Arc::new(Mutex::new(AudioHandler::new()));
        let buffer_i16 = Arc::new(Mutex::new(Vec::new())); // 初始化 buffer_i16

		let device = Self::open_playback(&audio_subsystem, data.clone(), buffer_i16.clone())?;

        let res = Arc::new(Mutex::new(Self {
            audio_subsystem,
            device,
            data,
            buffer_i16,
        }));

		Self::start(res.clone(), local_set);

		Ok(res)
	}

	#[instrument(skip(audio_subsystem, data))]
	fn open_playback(
        audio_subsystem: &AudioSubsystem,
        data: Arc<Mutex<AudioHandler>>,
        buffer_i16: Arc<Mutex<Vec<i16>>>,
	) -> Result<AudioDevice<SdlCallback>> {
		let desired_spec = AudioSpecDesired {
			freq: Some(48000),
			channels: Some(2),
			samples: Some(USUAL_FRAME_SIZE as u16),
		};

		audio_subsystem
			.open_playback(None, &desired_spec, move |spec| {
				// This spec will always be the desired spec, the sdl wrapper passes
				// zero as `allowed_changes`.
				debug!(?spec, driver = audio_subsystem.current_audio_driver(), "Got playback spec");
				SdlCallback { data, buffer_i16 }
			})
			.map_err(|e| format_err!("SDL error: {}", e))
	}

	#[instrument(skip(t2a, local_set))]
	fn start(t2a: Arc<Mutex<Self>>, local_set: &LocalSet) {
		local_set.spawn_local(
			IntervalStream::new(time::interval(Duration::from_secs(1))).for_each(move |_| {
				let mut t2a = t2a.lock().unwrap();

				if t2a.device.status() == AudioStatus::Stopped {
					// Try to reconnect to audio
					match Self::open_playback(&t2a.audio_subsystem, t2a.data.clone(), t2a.buffer_i16.clone()) {
						Ok(d) => {
							t2a.device = d;
							debug!("Reconnected to playback device");
						}
						Err(error) => {
							error!(%error, "Failed to open playback device");
						}
					};
				}

				let data_empty = t2a.data.lock().unwrap().get_queues().is_empty();
				if t2a.device.status() == AudioStatus::Paused && !data_empty {
					debug!("Resuming playback");
					t2a.device.resume();
				} else if t2a.device.status() == AudioStatus::Playing && data_empty {
					debug!("Pausing playback");
					t2a.device.pause();
				}
				future::ready(())
			}),
		);
	}

	#[instrument(skip(self, id, packet))]
	pub(crate) fn play_packet(&mut self, id: Id, packet: InAudioBuf) -> Result<()> {
		let mut data = self.data.lock().unwrap();
		data.handle_packet(id, packet)?;

		if self.device.status() == AudioStatus::Paused {
			debug!("Resuming playback");
			self.device.resume();
		}
		Ok(())
	}

    pub fn get_buff_i16(&self) -> Vec<i16> {
		self.buffer_i16.lock().unwrap().clone() 
    }

}

// impl AudioCallback for SdlCallback {
// 	type Channel = f32;
// 	fn callback(&mut self, buffer: &mut [Self::Channel]) {
// 		// Clear buffer
// 		for d in &mut *buffer {
// 			*d = 0.0;
// 		}
// 		let mut data = self.data.lock().unwrap();
// 		data.fill_buffer(buffer);
// 	}
// }

impl AudioCallback for SdlCallback {
	type Channel = f32;
    fn callback(&mut self, buffer: &mut [Self::Channel]) {
        // Clear buffer
        for d in &mut *buffer {
            *d = 0.0;
        }

        let mut data = self.data.lock().unwrap();
        data.fill_buffer(buffer);

        let buffer_i16 = vec_f32_to_i16_linear(buffer);
        *self.buffer_i16.lock().unwrap() = buffer_i16;
		// println!("Audio buffer: {:?}",  &buffer_i16[..buffer_i16.len().min(10)]);
		// write_vec_i16_to_file(&buffer_i16, "test.pcm");
	}
}