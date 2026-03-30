use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Threading::*;
use windows::core::*;

const SAMPLE_RATE: u32 = 16000;
const CHANNELS: u16 = 1;
const BITS_PER_SAMPLE: u16 = 16;
const FRAME_SAMPLES: usize = 3200; // 200ms at 16kHz
const FRAME_BYTES: usize = FRAME_SAMPLES * 2; // 16-bit = 2 bytes/sample

static CAPTURING: AtomicBool = AtomicBool::new(false);
static STOP_FLAG: LazyLock<Arc<AtomicBool>> =
    LazyLock::new(|| Arc::new(AtomicBool::new(false)));

pub fn start_capture() {
    if CAPTURING.swap(true, Ordering::SeqCst) {
        return;
    }

    let stop = STOP_FLAG.clone();
    stop.store(false, Ordering::SeqCst);

    std::thread::spawn(move || {
        if let Err(e) = capture_thread(stop) {
            log::error!("audio capture error: {e}");
        }
        CAPTURING.store(false, Ordering::SeqCst);
    });
}

pub fn stop_capture() {
    STOP_FLAG.store(true, Ordering::SeqCst);
}

fn capture_thread(stop: Arc<AtomicBool>) -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)?;
    }

    let result = unsafe { capture_loop(&stop) };

    unsafe { CoUninitialize() };
    result
}

unsafe fn capture_loop(stop: &AtomicBool) -> Result<()> {
    let enumerator: IMMDeviceEnumerator =
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
    let device = enumerator.GetDefaultAudioEndpoint(eCapture, eConsole)?;
    let audio_client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

    let mix_format_ptr = audio_client.GetMixFormat()?;
    let mix_format = &*mix_format_ptr;
    let device_rate = mix_format.nSamplesPerSec;
    let device_channels = mix_format.nChannels;
    let device_bits = mix_format.wBitsPerSample;

    log::info!(
        "audio device: {}Hz {}ch {}bit",
        device_rate, device_channels, device_bits
    );

    let desired_format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM as u16,
        nChannels: CHANNELS,
        nSamplesPerSec: SAMPLE_RATE,
        nAvgBytesPerSec: SAMPLE_RATE * (BITS_PER_SAMPLE as u32 / 8) * CHANNELS as u32,
        nBlockAlign: CHANNELS * (BITS_PER_SAMPLE / 8),
        wBitsPerSample: BITS_PER_SAMPLE,
        cbSize: 0,
    };

    let needs_resample = if audio_client
        .IsFormatSupported(
            AUDCLNT_SHAREMODE_SHARED,
            &desired_format,
            None,
        )
        .is_ok()
    {
        audio_client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            0,
            10_000_000,
            0,
            &desired_format,
            None,
        )?;
        false
    } else {
        audio_client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            0,
            10_000_000,
            0,
            mix_format_ptr,
            None,
        )?;
        true
    };

    let capture_client: IAudioCaptureClient = audio_client.GetService()?;

    let event = CreateEventW(None, false, false, None)?;
    audio_client.SetEventHandle(event)?;

    audio_client.Start()?;

    let mut output_buffer: Vec<u8> = Vec::with_capacity(FRAME_BYTES * 2);
    let mut timestamp: u64 = 0;

    while !stop.load(Ordering::SeqCst) {
        let wait_result = WaitForSingleObject(event, 100);
        if wait_result == WAIT_TIMEOUT {
            continue;
        }

        loop {
            let mut packet_size = 0u32;
            capture_client.GetNextPacketSize(&mut packet_size)?;
            if packet_size == 0 {
                break;
            }

            let mut buffer_ptr = std::ptr::null_mut();
            let mut num_frames = 0u32;
            let mut flags = 0u32;
            capture_client.GetBuffer(
                &mut buffer_ptr,
                &mut num_frames,
                &mut flags,
                None,
                None,
            )?;

            if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) == 0 && !buffer_ptr.is_null() {
                if needs_resample {
                    let samples = resample(
                        buffer_ptr,
                        num_frames as usize,
                        device_rate,
                        device_channels as usize,
                        device_bits,
                    );
                    output_buffer.extend_from_slice(&samples);
                } else {
                    let byte_count = num_frames as usize * 2;
                    let slice = std::slice::from_raw_parts(buffer_ptr, byte_count);
                    output_buffer.extend_from_slice(slice);
                }
            }

            capture_client.ReleaseBuffer(num_frames)?;

            while output_buffer.len() >= FRAME_BYTES {
                let frame: Vec<u8> = output_buffer.drain(..FRAME_BYTES).collect();
                crate::bridge::push_audio(&frame, timestamp);
                timestamp += (FRAME_SAMPLES as u64) * 1_000_000 / (SAMPLE_RATE as u64);
            }
        }
    }

    audio_client.Stop()?;
    let _ = CloseHandle(event);

    if !output_buffer.is_empty() {
        output_buffer.resize(FRAME_BYTES, 0);
        crate::bridge::push_audio(&output_buffer, timestamp);
    }

    log::info!("audio capture stopped");
    Ok(())
}

unsafe fn resample(
    buffer: *const u8,
    num_frames: usize,
    src_rate: u32,
    src_channels: usize,
    src_bits: u16,
) -> Vec<u8> {
    let mono_samples: Vec<f32> = (0..num_frames)
        .map(|i| {
            let mut sum = 0.0f32;
            for ch in 0..src_channels {
                let offset = (i * src_channels + ch) * (src_bits as usize / 8);
                let sample = match src_bits {
                    16 => {
                        let ptr = buffer.add(offset) as *const i16;
                        *ptr as f32 / 32768.0
                    }
                    32 => {
                        let ptr = buffer.add(offset) as *const f32;
                        let val = *ptr;
                        if val.abs() <= 1.0 { val } else { val / 2147483648.0 }
                    }
                    24 => {
                        let b = std::slice::from_raw_parts(buffer.add(offset), 3);
                        let val = ((b[2] as i32) << 24 | (b[1] as i32) << 16 | (b[0] as i32) << 8) >> 8;
                        val as f32 / 8388608.0
                    }
                    _ => 0.0,
                };
                sum += sample;
            }
            sum / src_channels as f32
        })
        .collect();

    let ratio = src_rate as f64 / SAMPLE_RATE as f64;
    let out_len = (num_frames as f64 / ratio) as usize;
    let mut output = Vec::with_capacity(out_len * 2);

    for i in 0..out_len {
        let src_pos = i as f64 * ratio;
        let idx = src_pos as usize;
        let frac = src_pos - idx as f64;

        let s0 = mono_samples.get(idx).copied().unwrap_or(0.0);
        let s1 = mono_samples.get(idx + 1).copied().unwrap_or(s0);
        let interpolated = s0 + (s1 - s0) * frac as f32;

        let clamped = interpolated.clamp(-1.0, 1.0);
        let sample_i16 = (clamped * 32767.0) as i16;
        output.extend_from_slice(&sample_i16.to_le_bytes());
    }

    output
}
