//! Sonidos de la interfaz, sintetizados al momento (sin archivos), igual que renderer/sounds.js:
//! un tick corto al recorrer la rueda, un pop al elegir una cuenta y un acorde al apretar Jugar.
//! El audio vive en un hilo propio que se abre la primera vez que algo suena.
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const RATE: u32 = 44_100;

#[derive(Clone, Copy)]
enum Wave {
    Sine,
    Triangle,
}

/// Un tono corto: la frecuencia baja/sube de `from` a `to` y el volumen cae en `dur` segundos.
struct Blip {
    wave: Wave,
    from: f32,
    to: f32,
    dur: f32,
    gain: f32,
    delay: f32,
}

fn blip(wave: Wave, from: f32, to: f32, dur: f32, gain: f32) -> Blip {
    Blip { wave, from, to, dur, gain, delay: 0. }
}

/// Mezcla los tonos en muestras mono (mismas curvas exponenciales que el WebAudio de Electron).
fn render(blips: &[Blip], volume: f32) -> Vec<f32> {
    let total = blips.iter().map(|b| b.delay + b.dur + 0.02).fold(0., f32::max);
    let mut out = vec![0f32; (total * RATE as f32) as usize];
    for b in blips {
        let peak = (b.gain * volume).max(0.0002);
        let start = (b.delay * RATE as f32) as usize;
        let n = ((b.dur + 0.02) * RATE as f32) as usize;
        let mut phase = 0f32;
        for i in 0..n {
            let t = i as f32 / RATE as f32;
            // Frecuencia: rampa exponencial hasta el 60 % de la duración.
            let ramp = (t / (b.dur * 0.6)).min(1.);
            let freq = b.from * (b.to / b.from).powf(ramp);
            // Volumen: sube en 4 ms y cae exponencialmente hasta el final.
            let env = if t < 0.004 {
                0.0001 * (peak / 0.0001).powf(t / 0.004)
            } else if t < b.dur {
                peak * (0.0001 / peak).powf((t - 0.004) / (b.dur - 0.004))
            } else {
                0.
            };
            phase = (phase + freq / RATE as f32).fract();
            let wave = match b.wave {
                Wave::Sine => (phase * std::f32::consts::TAU).sin(),
                Wave::Triangle => 4. * (phase - (phase + 0.5).floor()).abs() - 1.,
            };
            if let Some(s) = out.get_mut(start + i) {
                *s += wave * env;
            }
        }
    }
    out
}

pub struct Sounds {
    pub enabled: bool,
    pub volume: f32,
    tx: Option<Sender<Vec<f32>>>,
    last_tick: Option<Instant>,
}

fn global() -> &'static Mutex<Sounds> {
    static SOUNDS: OnceLock<Mutex<Sounds>> = OnceLock::new();
    SOUNDS.get_or_init(|| Mutex::new(Sounds { enabled: true, volume: 0.5, tx: None, last_tick: None }))
}

/// Volumen y sonido encendido, leídos de config.json al arrancar.
pub fn configure(enabled: bool, volume: f32) {
    let mut s = global().lock().unwrap();
    s.enabled = enabled;
    s.volume = volume.clamp(0., 1.);
}

pub fn state() -> (bool, f32) {
    let s = global().lock().unwrap();
    (s.enabled, s.volume)
}

fn play(blips: Vec<Blip>) {
    let mut s = global().lock().unwrap();
    if !s.enabled || s.volume <= 0. {
        return;
    }
    let samples = render(&blips, s.volume);
    if s.tx.is_none() {
        let (tx, rx) = channel::<Vec<f32>>();
        std::thread::spawn(move || {
            // Sin dispositivo de audio, los sonidos simplemente no suenan.
            let Ok((_stream, handle)) = rodio::OutputStream::try_default() else { return };
            while let Ok(samples) = rx.recv() {
                let _ = handle.play_raw(rodio::buffer::SamplesBuffer::new(1, RATE, samples));
            }
        });
        s.tx = Some(tx);
    }
    if let Some(tx) = &s.tx {
        let _ = tx.send(samples);
    }
}

/// Clic seco al pasar cada cuenta; no más de uno cada 30 ms aunque se recorra muy rápido.
pub fn tick() {
    {
        let mut s = global().lock().unwrap();
        if s.last_tick.is_some_and(|t| t.elapsed() < Duration::from_millis(30)) {
            return;
        }
        s.last_tick = Some(Instant::now());
    }
    let from = 2300. + rand::random::<f32>() * 250.;
    play(vec![blip(Wave::Triangle, from, 1300., 0.035, 0.05)]);
}

/// Pop tipo burbuja: tono que sube rápido.
pub fn pop() {
    play(vec![blip(Wave::Sine, 300., 760., 0.13, 0.16), blip(Wave::Triangle, 1800., 2400., 0.03, 0.03)]);
}

/// Acorde al apretar Jugar.
pub fn play_chord() {
    play(vec![
        blip(Wave::Sine, 523., 540., 0.16, 0.12),
        Blip { delay: 0.08, ..blip(Wave::Sine, 784., 800., 0.24, 0.12) },
        Blip { delay: 0.16, ..blip(Wave::Sine, 1046., 1060., 0.34, 0.08) },
    ]);
}
