
use clap::{Arg, Command};
use anyhow::{bail, Result};
use futures::prelude::*;
use tracing::{debug, info};

// use tokio::signal;
use tokio::sync::mpsc;
use tokio::time::{self, Duration};

use tsclientlib::data::{self, Channel, Client};
use tsclientlib::prelude::*;
use tsclientlib::{ClientId, ChannelId, Connection, DisconnectOptions, Identity, StreamItem};

// audio play
use tokio::task::LocalSet;
mod audio_utils;
use tsproto_packets::packets::{AudioData, CodecType, OutAudio};
use rand::Rng;
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct ConnectionId(u64);

struct AudioBuffer {
    buffer: Arc<Mutex<VecDeque<u8>>>,
}

impl AudioBuffer {
    fn new() -> Self {
        AudioBuffer {
            buffer: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    fn push(&self, data: &[u8]) {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.extend(data.iter().cloned());
    }

    fn pop(&self, size: usize) -> Option<Vec<u8>> {
        let mut buffer = self.buffer.lock().unwrap();
        if buffer.len() >= size {
            let mut result = Vec::with_capacity(size);
            for _ in 0..size {
                if let Some(byte) = buffer.pop_front() {
                    result.push(byte);
                }
            }
            Some(result)
        } else {
            None
        }
    }
}

fn convert_to_pcm_16khz_8bit(audio_data: &[u8]) -> Vec<u8> {
    // 这里实现音频数据的转换逻辑
    // 例如，使用 `cpal` 或 `rodio` 库进行转换
    // 这里只是一个示例，实际实现可能需要更复杂的处理
    audio_data.to_vec()
}

/// `channels` have to be ordered.
fn print_channels(clients: &[&Client], channels: &[&Channel], parent: ChannelId, depth: usize) {
	let indention = "  ".repeat(depth);
	for channel in channels {
		if channel.parent == parent {
			println!("{}- {}", indention, channel.name);
			// Print all clients in this channel
			for client in clients {
				if client.channel == channel.id {
					println!("{}  {}", indention, client.name);
				}
			}

			print_channels(clients, channels, channel.id, depth + 1);
		}
	}
}

fn print_channel_tree(con: &data::Connection) {
	let mut channels: Vec<_> = con.channels.values().collect();
	let mut clients: Vec<_> = con.clients.values().collect();
	// This is not the real sorting order, the order is the ChannelId of the
	// channel on top of this one, but we don't care for this example.
	channels.sort_by_key(|ch| ch.order.0);
	clients.sort_by_key(|c| c.talk_power);
	println!("{}", con.server.name);
	print_channels(&clients, &channels, ChannelId(0), 0);
}

#[tokio::main] // 启用异步运行时
async fn main() -> Result<()> {
    let matches = Command::new("Ts3EzApi")
        .version("1.0")
        .author("Your Name <your.email@example.com>")
        .about("TeamSpeak 3 Easy API")
        .arg(
            Arg::new("ip")
                .short('i')
                .long("ip")
                .value_name("IP_ADDRESS")
                .help("Sets the IP address of the TeamSpeak 3 server")
                .default_value("127.0.0.1"), // 默认值
        )
        .arg(
            Arg::new("name")
                .short('n')
                .long("name")
                .value_name("NAME")
                .help("Sets the name of the client")
                .default_value("SuperSB"), // 默认值
        )
        .arg(
            Arg::new("io")
                .short('o')
                .long("io")
                .value_name("IO_PORT")
                .help("Sets the I/O port of the TeamSpeak 3 server")
                .default_value("43500"), // 默认值
        )
        .get_matches();

    // 获取参数值，如果未提供则使用默认值
    let ip: &String = matches.get_one::<String>("ip").unwrap();
    let name: &String = matches.get_one::<String>("name").unwrap();
    let io_port= matches.get_one::<String>("io").unwrap();

    // 打印解析结果
    println!("IP Address: {}", ip);
    println!("Client Name: {}", name);
    println!("I/O Port: {}", io_port);

    // 準備參數
	let con_id = ConnectionId(0);
	let local_set = LocalSet::new();
	let audiodata = audio_utils::start(&local_set)?;
    
    // 开始创建链接
	let con_config = Connection::build(ip.as_str());     

	// （可选）设置此客户端的密钥，否则将生成新密钥。
	let id = Identity::new_from_str(
		"MG0DAgeAAgEgAiAIXJBlj1hQbaH0Eq0DuLlCmH8bl+veTAO2+\
		k9EQjEYSgIgNnImcmKo7ls5mExb6skfK2Tw+u54aeDr0OP1ITs\
		C/50CIA8M5nmDBnmDM/gZ//4AAAAAAAAAAAAAAAAAAAAZRzOI").unwrap();
    
    // 密钥绑定到连接信息上
	let con_config = con_config.identity(id);
    
    // 连接...
    let mut con = con_config.connect()?;
    
    let r = con
        .events()
        .try_filter(|e| future::ready(matches!(e, StreamItem::BookEvents(_))))
        .next()
        .await;
    if let Some(r) = r {
        r?;
    }

    // 音频输入设备准备
	let (send, mut recv) = mpsc::channel(5);
	{
		let mut a2t = audiodata.a2ts.lock().unwrap();
		a2t.set_listener(send);
		a2t.set_volume(1.0f32);
		a2t.set_playing(true);
	}

	con.get_state().unwrap().server.set_subscribed(true).send(&mut con)?;

    // 等待一段时间
    let mut events = con.events().try_filter(|_| future::ready(false));
    tokio::select! {
        _ = time::sleep(Duration::from_secs(1)) => {}
        _ = events.next() => {
            bail!("Disconnected");
        }
    };
    drop(events);

	// Print channel tree
	print_channel_tree(con.get_state().unwrap());

	// 修改名字
	{
		let state = con.get_state().unwrap();
		// let name = state.clients[&state.own_client].name.clone();
		state
			.client_update()
			.set_input_muted(true)
			.set_name(&format!("寂寞小旋"))
			.send(&mut con)?;
	}

    let audio_buffer = AudioBuffer::new();
    // 音频播放
	loop {
		let t2a = audiodata.ts2a.clone();
		let events = con.events().try_for_each(|e| async {
			if let StreamItem::Audio(packet) = e {
                // 获取流
                println!("Stream");
                // // process_audio_packet(&packet, &audio_buffer);
				// let from = ClientId(match packet.data().data() {
				// 	AudioData::S2C { from, .. } => *from,
				// 	AudioData::S2CWhisper { from, .. } => *from,
				// 	_ => panic!("Can only handle S2C packets but got a C2S packet"),
				// });
				// let mut t2a = t2a.lock().unwrap();
				// if let Err(error) = t2a.play_packet((con_id, from), packet) {
				// 	debug!(%error, "Failed to play packet");
				// }
			}
			Ok(())
		});

		// Wait for ctrl + c
		tokio::select! {
			send_audio = recv.recv() => {
				if let Some(packet) = send_audio {
					con.send_audio(packet)?;
				} else {
					info!("Audio sending stream was canceled");
					break;
				}
			}
			_ = tokio::signal::ctrl_c() => { break; }
			r = events => {
				r?;
				bail!("Disconnected");
			}
		};
	}

    // 断开连接
    con.disconnect(DisconnectOptions::new())?;
    con.events().for_each(|_| future::ready(())).await;

    Ok(())
}