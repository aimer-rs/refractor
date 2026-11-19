//! The spinner shown while the language server works: a Lottie animation that is moved on by a timer a few times a
//! second, and not played by the framework.
//!
//! A playing animation asks for a frame every time the screen can show one, and each of those is a frame of the whole
//! window, which costs a good part of a core in the debug build for a mark that is sixteen pixels across. Moved on by a
//! timer, only the mark changes, and a spinner does not need more than this to be seen turning.

use aimer::lottie::{Composition, Lottie, LottieScene};
use aimer::{State, StateUpdater, StateView, StatefulWidget, Venus, Widget};
use std::sync::Arc;
use std::time::Duration;

/// How long between one picture of the spinner and the next.
const FRAME: Duration = Duration::from_millis(66);
/// How long the animation takes to turn once, in seconds.
const PERIOD: f32 = 1.5;

thread_local! {
    /// The animation, which is parsed once.
    static SCENE: Arc<LottieScene> = {
        let composition = Composition::parse(include_str!("../../../assets/animations/loading.json")).expect("the animation of the spinner is a Lottie document");
        Arc::new(LottieScene::new(composition))
    };
}

/// How far through its turn the animation is after one more picture: a fraction from 0 up to, and not including, 1.
fn advance(progress: f32) -> f32 {
    let next = progress + FRAME.as_secs_f32() / PERIOD;
    next - next.floor()
}

/// A spinner `size` logical pixels across.
#[derive(Clone, StatefulWidget)]
pub struct Spinner {
    size: f32,
}

impl Spinner {
    pub fn new(size: u32) -> Self {
        Self { size: size as f32 }
    }
}

struct SpinnerState {
    size: f32,
}

impl State<Spinner> for SpinnerState {
    fn create(widget: Spinner) -> Self {
        Self {
            size: widget.size,
        }
    }

    fn on_mount(&mut self, updater: StateUpdater<Self>) {
       
    }

    fn adopt_config_from(&mut self, new: Self) {
        self.size = new.size;
    }

    fn build(this: StateView<Self>, _ctx: &aimer::BuildContext) -> impl Widget {
        let scene = SCENE.with(Arc::clone);
        Lottie::new()
            .width(this.size)
            .height(this.size)
            .playing(true)
            .box_animation(scene)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_animation_goes_on_by_the_same_part_of_its_turn_each_picture_and_begins_again_at_the_end()
    {
        let step = FRAME.as_secs_f32() / PERIOD;
        assert!((advance(0.0) - step).abs() < 1e-6);
        assert!((advance(0.5) - (0.5 + step)).abs() < 1e-6);
        let wrapped = advance(1.0 - step / 2.0);
        assert!((0.0..step).contains(&wrapped), "{wrapped}");
        // However many pictures there are, it stays inside its turn.
        let mut progress = 0.0;
        for _ in 0..1000 {
            progress = advance(progress);
            assert!((0.0..1.0).contains(&progress), "{progress}");
        }
    }

    #[test]
    fn a_turn_is_made_of_about_twenty_two_pictures_which_is_enough_to_be_seen_turning() {
        let pictures = PERIOD / FRAME.as_secs_f32();
        assert!((20.0..25.0).contains(&pictures), "{pictures}");
    }

    #[test]
    fn the_animation_is_a_document_that_the_player_reads_without_leaving_anything_out() {
        let composition =
            Composition::parse(include_str!("../../../assets/animations/loading.json"))
                .expect("a Lottie document");
        assert!(
            composition.diagnostics.is_empty(),
            "the player skipped part of it: {:?}",
            composition.diagnostics
        );
    }
}
