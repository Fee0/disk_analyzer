use std::time::{Duration, Instant};

use dscan_core::{FrameArc, Transition, TransitionKind};

const REVEAL: Duration = Duration::from_millis(750);
const ZOOM: Duration = Duration::from_millis(480);
const MORPH: Duration = Duration::from_millis(420);

type Easing = fn(f64) -> f64;

fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

fn ease_in_out_cubic(t: f64) -> f64 {
    if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 }
}

fn timing(kind: TransitionKind) -> (Duration, Easing) {
    match kind {
        TransitionKind::Reveal => (REVEAL, ease_out_cubic),
        TransitionKind::ZoomIn | TransitionKind::ZoomOut => (ZOOM, ease_in_out_cubic),
        TransitionKind::Morph => (MORPH, ease_in_out_cubic),
    }
}

pub struct MovingChart {
    pub arcs: Vec<FrameArc>,
    pub total: u64,
    pub style_progress: f64,
}

impl MovingChart {
    fn at(transition: &Transition, eased_progress: f64) -> Self {
        MovingChart {
            arcs: transition.frame(eased_progress),
            total: transition.total(eased_progress),
            style_progress: eased_progress,
        }
    }
}

pub struct ChartAnimation {
    transition: Transition,
    started: Option<Instant>,
    current: MovingChart,
}

pub enum Advance {
    Running,
    Finished,
}

impl ChartAnimation {
    pub fn new(transition: Transition) -> Self {
        let current = MovingChart::at(&transition, 0.0);
        ChartAnimation { transition, started: None, current }
    }

    pub fn advance(&mut self, now: Instant) -> Advance {
        let (duration, ease) = timing(self.transition.kind());
        let started = *self.started.get_or_insert(now);
        let linear = now.saturating_duration_since(started).as_secs_f64() / duration.as_secs_f64();
        if linear >= 1.0 {
            return Advance::Finished;
        }
        self.current = MovingChart::at(&self.transition, ease(linear));
        Advance::Running
    }

    pub fn current_frame(&self) -> &MovingChart {
        &self.current
    }

    #[cfg(test)]
    pub fn jump_to(&mut self, eased_progress: f64) {
        self.current = MovingChart::at(&self.transition, eased_progress);
    }

    #[cfg(test)]
    pub fn kind(&self) -> TransitionKind {
        self.transition.kind()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easings_start_at_rest_end_at_rest_and_never_overshoot() {
        for ease in [ease_out_cubic, ease_in_out_cubic] {
            assert_eq!(ease(0.0), 0.0);
            assert!((ease(1.0) - 1.0).abs() < 1e-12);
            let samples: Vec<f64> = (0..=100).map(|i| ease(i as f64 / 100.0)).collect();
            assert!(samples.windows(2).all(|w| w[1] >= w[0]), "progress only moves forward");
        }
        assert!(ease_out_cubic(0.5) > 0.5, "a reveal decelerates into place");
        assert!((ease_in_out_cubic(0.5) - 0.5).abs() < 1e-12, "zoom and morph are symmetric");
    }

    #[test]
    fn the_clock_starts_on_the_first_frame_and_finishes_after_the_duration() {
        let layout = dscan_core::Layout::default();
        let mut animation = ChartAnimation::new(Transition::reveal(&layout));
        let first_frame = Instant::now();
        assert!(matches!(animation.advance(first_frame), Advance::Running));
        assert!(matches!(animation.advance(first_frame + REVEAL / 2), Advance::Running));
        assert!(animation.current_frame().style_progress > 0.5);
        assert!(matches!(animation.advance(first_frame + REVEAL), Advance::Finished));
    }
}
