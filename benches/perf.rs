//! Hot-path benchmarks: `cargo bench --bench perf`.

use std::hint::black_box;
use std::path::{Path, PathBuf};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use rmp::audio::decoder::Decoder;
use rmp::audio::dsp::{EqParams, Equalizer};
use rmp::audio::resample::Resampler;
use rmp::audio::spectrum::Spectrum;
use rmp::library::Track;
use rmp::theme::Theme;
use rmp::ui::library_panel::{self, Cache};

const RATE: u32 = 48000;

fn sine(frames: usize, rate: u32) -> Vec<f32> {
    (0..frames)
        .flat_map(|i| {
            let s = (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin() * 0.5;
            [s, s]
        })
        .collect()
}

fn eq(c: &mut Criterion) {
    let mut g = c.benchmark_group("eq");
    let input = sine(1024, RATE);
    let on = EqParams {
        enabled: true,
        preamp_db: -3.0,
        bands_db: [6.0, 4.0, 2.0, -1.0, -2.0, 0.5, 2.0, 4.0, 5.0, 6.0],
    };
    let mut e = Equalizer::new(RATE as f64);
    // Let the smoothing converge so this measures the steady state.
    let mut warm = sine(RATE as usize, RATE);
    e.process(&mut warm, &on);
    let mut buf = input.clone();
    g.bench_function("process_1024_enabled", |b| {
        b.iter(|| {
            buf.copy_from_slice(&input);
            e.process(black_box(&mut buf), &on);
        })
    });
    let off = EqParams::default();
    let mut e = Equalizer::new(RATE as f64);
    g.bench_function("process_1024_bypass", |b| {
        b.iter(|| {
            buf.copy_from_slice(&input);
            e.process(black_box(&mut buf), &off);
        })
    });
    g.finish();
}

fn spectrum(c: &mut Criterion) {
    let mut s = Spectrum::new(RATE);
    let (mut tx, mut rx) = rtrb::RingBuffer::new(8192);
    // One 30 fps frame worth of mono samples.
    let frame: Vec<f32> = sine(1600, RATE).into_iter().step_by(2).collect();
    c.bench_function("spectrum/feed_update", |b| {
        b.iter(|| {
            for &v in &frame {
                let _ = tx.push(v);
            }
            s.feed(&mut rx);
            s.update(0.033, true);
            black_box(&s.bars);
        })
    });
}

fn resample(c: &mut Criterion) {
    let input = sine(1152, 44100);
    let mut r = Resampler::new(44100, RATE).unwrap();
    let mut out = Vec::with_capacity(8192);
    c.bench_function("resample/44k1_to_48k_1152", |b| {
        b.iter(|| {
            out.clear();
            r.process(black_box(&input), &mut out).unwrap();
            black_box(&out);
        })
    });
}

/// 16-bit stereo PCM WAV.
fn write_wav(path: &Path, rate: u32, frames: usize) {
    let data: Vec<u8> = sine(frames, rate)
        .iter()
        .flat_map(|&s| ((s * 32767.0) as i16).to_le_bytes())
        .collect();
    let mut b = Vec::with_capacity(44 + data.len());
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 4).to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(data.len() as u32).to_le_bytes());
    b.extend_from_slice(&data);
    std::fs::write(path, b).unwrap();
}

fn decode(c: &mut Criterion) {
    let dir = std::env::temp_dir().join(format!("rmp-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("10s.wav");
    write_wav(&path, 44100, 44100 * 10);
    let mut out = Vec::with_capacity(1 << 16);
    c.bench_function("decode/wav_10s", |b| {
        b.iter(|| {
            let mut d = Decoder::open(&path).unwrap();
            let mut frames = 0;
            loop {
                out.clear();
                if !d.next_frames(&mut out).unwrap() {
                    break;
                }
                frames += out.len() / 2;
            }
            black_box(frames)
        })
    });
    std::fs::remove_dir_all(dir).unwrap();
}

/// 500 artists x 4 albums x 5 tracks.
fn library() -> Vec<Track> {
    let mut v = Vec::with_capacity(10_000);
    for ar in 0..500 {
        for al in 0..4 {
            for t in 0..5 {
                v.push(Track {
                    path: PathBuf::from(format!("/music/Artist {ar}/Album {al}/{t:02}.flac")),
                    title: Some(format!("Song Number {t} of Album {al}")),
                    artist: Some(format!("Artist {ar}")),
                    album: Some(format!("Album {al}")),
                    track_no: Some(t + 1),
                    year: Some(1990 + al),
                    genre: Some("Rock".into()),
                    duration_ms: Some(200_000),
                    ..Default::default()
                });
            }
        }
    }
    v
}

fn library_panel(c: &mut Criterion) {
    let lib = library();
    let mut g = c.benchmark_group("library");
    for (name, query) in [("refresh_all", ""), ("refresh_query", "artist 12")] {
        let mut cache = Cache::default();
        let mut generation = 0u64;
        g.bench_function(name, |b| {
            b.iter(|| {
                generation += 1;
                library_panel::refresh(&mut cache, &lib, generation, query);
                black_box(cache.matches)
            })
        });
    }

    // Typing in the search box: same library, new query each time.
    let mut cache = Cache::default();
    let mut flip = false;
    g.bench_function("refresh_keystroke", |b| {
        b.iter(|| {
            flip = !flip;
            let q = if flip { "artist 12" } else { "artist 1" };
            library_panel::refresh(&mut cache, &lib, 1, q);
            black_box(cache.matches)
        })
    });

    let theme = Theme::winamp();
    for (name, query) in [("tree_frame_all", ""), ("tree_frame_query", "artist 12")] {
        let mut cache = Cache::default();
        library_panel::refresh(&mut cache, &lib, 1, query);
        let searching = !query.is_empty();
        let ctx = egui::Context::default();
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(420.0, 800.0),
            )),
            ..Default::default()
        };
        // Font atlas and layout caches are built on the first frames.
        for _ in 0..3 {
            ctx.run_ui(input(), |ui| {
                library_panel::tree(ui, &mut cache, &lib, searching, &theme);
            })
            .drop_without_applying_deltas();
        }
        g.bench_function(name, |b| {
            b.iter_batched(
                input,
                |i| {
                    ctx.run_ui(i, |ui| {
                        black_box(library_panel::tree(ui, &mut cache, &lib, searching, &theme));
                    })
                    .drop_without_applying_deltas()
                },
                BatchSize::SmallInput,
            )
        });
    }
    g.finish();
}

fn all(c: &mut Criterion) {
    eq(c);
    spectrum(c);
    resample(c);
    decode(c);
    library_panel(c);
}

criterion_group!(benches, all);
criterion_main!(benches);
