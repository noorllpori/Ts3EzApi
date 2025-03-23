
use clap::{Arg, Command};
use anyhow::{bail, Context, Error, Result,anyhow};
use futures::prelude::*;
use tracing::{debug, info};

use std::fs::OpenOptions;
use std::io::{self,Read, Write};

// socket
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use futures_util::{StreamExt, SinkExt};
use std::net::SocketAddr;

// use tokio::signal;
use tokio::sync::mpsc;
use tokio::time::{self, Duration};

use tsclientlib::data::{self, Channel, Client};
use tsclientlib::prelude::*;
use tsclientlib::{ClientId, ChannelId, Connection, DisconnectOptions, Identity, StreamItem};

// audio play
use tokio::task::LocalSet;
mod audio_utils;
mod audio_stream_utils;
use tsproto_packets::packets::{InAudioBuf, CodecType,AudioData};
use audiopus::{ Channels, SampleRate};
use audiopus::coder::Decoder;
// use rand::Rng;
// use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct ConnectionId(u64);

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

fn decode_packet(packet: InAudioBuf) -> Result<Vec<f32>, anyhow::Error> {
    // let packet_data: Option<packet::Packet<'_>>;
    let audio_data: &tsproto_packets::packets::AudioData<'_> = packet.data().data();
    
    // 检查是否是 Opus 编码
    if audio_data.codec() != tsproto_packets::packets::CodecType::OpusMusic &&
       audio_data.codec() != tsproto_packets::packets::CodecType::OpusVoice {
        return Err(anyhow!("Unsupported codec"));
    }

    // 获取 Opus 编码的音频数据
    let encoded_data: Option<audiopus::packet::Packet<'_>> = Some(
        packet.data().data().data().try_into()
            .context("Failed to convert packet data")?
    );
    
    // 创建 Opus 解码器
    let mut decoder = Decoder::new(SampleRate::Hz48000, Channels::Stereo)
        .map_err(|e| anyhow!("Decoder creation failed: {}", e))?;

    let mut pcm_buffer = vec![0.0; 48000]; // 预分配缓冲区
    let len = decoder.decode_float(
        encoded_data,
        (&mut pcm_buffer[..])
            .try_into()
            .map_err(|e| anyhow::anyhow!("Failed to convert pcm_buffer slice: {:?}", e))?,
        false
    )?;
    
    pcm_buffer.truncate(len * 2);

    pcm_buffer.drain(0..74);
    
    Ok(pcm_buffer)
}

fn decode_packet_i16(packet: InAudioBuf) -> Result<Vec<i16>, anyhow::Error> {
    // let packet_data: Option<packet::Packet<'_>>;
    let audio_data: &tsproto_packets::packets::AudioData<'_> = packet.data().data();
    
    // 检查是否是 Opus 编码
    if audio_data.codec() != tsproto_packets::packets::CodecType::OpusMusic &&
       audio_data.codec() != tsproto_packets::packets::CodecType::OpusVoice {
        return Err(anyhow!("Unsupported codec"));
    }

    // 获取 Opus 编码的音频数据
    let encoded_data: Option<audiopus::packet::Packet<'_>> = Some(
        packet.data().data().data().try_into()
            .context("Failed to convert packet data")?
    );
    
    // 创建 Opus 解码器
    let mut decoder = Decoder::new(SampleRate::Hz48000, Channels::Stereo)
        .map_err(|e| anyhow!("Decoder creation failed: {}", e))?;

    let mut pcm_buffer = vec![0i16; 48000]; // 预分配缓冲区
    let len = decoder.decode(
        encoded_data,
        (&mut pcm_buffer[..])
            .try_into()
            .map_err(|e| anyhow::anyhow!("Failed to convert pcm_buffer slice: {:?}", e))?,
        false
    )?;
    
    pcm_buffer.truncate(len * 2);
    
    Ok(pcm_buffer)
}

fn vec_f32_to_i16_linear(vec: &[f32]) -> Vec<i16> {
    vec.iter()
        .map(|&x| {
            let scaled = x * 65535.0 * 1.6;
            // let scaled = x * 32767.5 +  32767.5 ;
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

fn format_vec_f32(vec: &[f32]) -> String {
    vec.iter()
        .map(|&x| format!("{:.8}f", x)) 
        .collect::<Vec<String>>()
        .join(", ") // 用逗号和空格分隔
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
    let io_port: &String= matches.get_one::<String>("io").unwrap();

    // 打印解析结果
    println!("IP Address: {}", ip);
    println!("Client Name: {}", name);
    println!("I/O Port: {}", io_port);

    // // 开始构建 Socket 服务端
    // let addrw: String = format!("127.0.0.1:{}", io_port);

    // // 绑定 TCP 监听器
    // let listener = TcpListener::bind(&addrw).await.unwrap();
    // println!("WebSocket server listening on: {}", addrw);

    // loop {
    //     // 等待客户端连接
    //     let (stream, addr) = listener.accept().await.unwrap();
    //     println!("New connection from: {}", addr);

    //     // 处理 WebSocket 连接
    //     tokio::spawn(async move {
    //         // 尝试将 TCP 流升级为 WebSocket 连接
    //         if let Ok(ws_stream) = accept_async(stream).await {
    //             println!("WebSocket connection established with: {}", addr);

    //             let (mut write, mut read) = ws_stream.split();

    //             // 处理 WebSocket 消息
    //             while let Some(Ok(message)) = read.next().await {
    //                 println!("Received message from {}: {:?}", addr, message);

    //                 // 发送响应
    //                 if let Err(e) = write.send(message).await {
    //                     println!("Error sending message to {}: {}", addr, e);
    //                     break;
    //                 }
    //             }
    //         } else {
    //             println!("Failed to upgrade TCP connection to WebSocket with: {}", addr);
    //         }

    //         println!("Connection closed: {}", addr);
    //     });
    // }


    // 準備參數
	let con_id = ConnectionId(0);
	let local_set = LocalSet::new();
	// let audiodata = audio_utils::start(&local_set)?;
	let audiodata = audio_stream_utils::start_nosdl(&local_set)?;
    
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

    // 音频播放
	loop {
		let t2a = audiodata.ts2a.clone();
		let events = con.events().try_for_each(|e| async {
            // println!("Loop");
			if let StreamItem::Audio(packet) = e {

                // 推送到播放设备
				let from: ClientId = ClientId(match packet.data().data() {
					AudioData::S2C { from, .. } => *from,
					AudioData::S2CWhisper { from, .. } => *from,
					_ => panic!("Can only handle S2C packets but got a C2S packet"),
				});
				let mut t2a = t2a.lock().unwrap();
				if let Err(error) = t2a.play_packet((con_id, from), packet) {
					debug!(%error, "Failed to play packet");
				}
                
                // 获取当前的音频数据
                let buffer_i16 = t2a.get_buff_i16();
                println!("Audio buffer (i16): {:?}", &buffer_i16[..buffer_i16.len().min(30)]); // 打印前 30 个样本

                // // 获取到流
                // let _empty = packet.data().data().data().len() <= 1;
                // let codec = packet.data().data().codec(); 
                // let packet_len = packet.data().data().data().len();
                // // 检查编解码类型
                // if codec != CodecType::OpusMusic && codec != CodecType::OpusVoice {
                //     println!("未知编码");
                // }
                // // 处理音频数据               
                // let pack_encode: std::result::Result<Vec<i16>, Error> = decode_packet_i16(packet);
                // match pack_encode {
                //     Ok(decoded_data) => {
                //         println!("Audio buffer: {:?}",  &decoded_data[..decoded_data.len().min(30)]);
                //         // write_vec_i16_to_file(&decoded_data, "test.pcm");
                //     }
                //     Err(e) => {
                //         // 处理错误
                //         eprintln!("Failed to decode packet: {}", e);
                //     }
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