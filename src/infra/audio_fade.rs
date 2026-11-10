//! Fade-out and fade-in ramps applied to rodio sinks.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rodio::Sink;

pub(crate) fn spawn_fade_out(sink: Arc<Sink>, duration: Duration) {
    if duration.is_zero() {
        sink.stop();
        return;
    }
    std::thread::Builder::new()
        .name("audio-fade-out".to_string())
        .spawn(move || {
            let steps: u32 = 20;
            let step_duration = duration / steps;
            let initial_vol = sink.volume();
            for step in 1..=steps {
                std::thread::sleep(step_duration);
                let factor = (steps - step) as f32 / steps as f32;
                sink.set_volume(initial_vol * factor);
            }
            sink.stop();
        })
        .ok();
}

pub(crate) fn spawn_fade_in(
    sink: Arc<Sink>,
    target_vol: f32,
    duration: Duration,
    generation: Arc<AtomicU64>,
    expected_gen: u64,
) {
    if duration.is_zero() || target_vol <= 0.0 {
        sink.set_volume(target_vol);
        return;
    }
    std::thread::Builder::new()
        .name("audio-fade-in".to_string())
        .spawn(move || {
            let steps: u32 = 20;
            let step_duration = duration / steps;
            for step in 1..=steps {
                std::thread::sleep(step_duration);
                if generation.load(Ordering::SeqCst) != expected_gen {
                    return;
                }
                let factor = step as f32 / steps as f32;
                sink.set_volume(target_vol * factor);
            }
            if generation.load(Ordering::SeqCst) == expected_gen {
                sink.set_volume(target_vol);
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_out_zero_duration_stops_sink() {
        let stream = match rodio::OutputStreamBuilder::open_default_stream() {
            Ok(stream) => stream,
            Err(_) => return,
        };
        let sink = Arc::new(Sink::connect_new(stream.mixer()));
        spawn_fade_out(sink.clone(), Duration::ZERO);
        assert!(sink.empty());
    }

    #[test]
    fn fade_in_zero_duration_sets_target_volume() {
        let stream = match rodio::OutputStreamBuilder::open_default_stream() {
            Ok(stream) => stream,
            Err(_) => return,
        };
        let sink = Arc::new(Sink::connect_new(stream.mixer()));
        spawn_fade_in(sink.clone(), 0.5, Duration::ZERO, Arc::new(AtomicU64::new(0)), 0);
        assert!((sink.volume() - 0.5).abs() < 1e-6);
    }
}
