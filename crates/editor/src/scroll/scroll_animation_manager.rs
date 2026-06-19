use crate::{
    DisplayPoint, DisplayRow, EditorSettings, ScrollAnchor, WorkspaceId,
    display_map::{DisplaySnapshot, ToDisplayPoint},
};
use gpui::{App, Point, point};
use language::Bias;
use settings::Settings;
use std::time::Instant;

pub(crate) enum UpdateResponse {
    Finished {
        destination_anchor: ScrollAnchor,
        destination_top_row: u32,
        state: PersistentState,
    },
    Nothing,
    RequiresAnimationFrame {
        intermediate_anchor: ScrollAnchor,
        intermediate_top_row: u32,
    },
}

#[derive(Clone)]
pub(crate) struct PersistentState {
    pub(crate) map: DisplaySnapshot,
    pub(crate) workspace_id: Option<WorkspaceId>,
    pub(crate) local: bool,
    pub(crate) autoscroll: bool,
}

pub(crate) struct Anim {
    start: f64,
    delta: f64,
    destination_top_row: u32,
    destination_anchor: ScrollAnchor,
    start_moment: Instant,
    duration: f64,
    state: PersistentState,
}

impl Anim {
    pub(crate) fn new(
        from: Point<f64>,
        destination_top_row: u32,
        destination_anchor: ScrollAnchor,
        map: DisplaySnapshot,
        workspace_id: Option<WorkspaceId>,
        local: bool,
        autoscroll: bool,
        duration: f64,
    ) -> Anim {
        let start = from.y;
        let end = destination_anchor.offset.y
            + destination_anchor.anchor.to_display_point(&map).row().0 as f64;
        let delta = end - start;
        Anim {
            start,
            delta,
            destination_top_row,
            destination_anchor,
            start_moment: Instant::now(),
            duration,
            state: PersistentState {
                map,
                workspace_id,
                local,
                autoscroll,
            },
        }
    }
}

pub(crate) struct ScrollAnimationManager {
    anim: Option<Anim>,
    scroll_duration: f64,
}

impl ScrollAnimationManager {
    pub(crate) fn new(cx: &App) -> Self {
        ScrollAnimationManager {
            anim: None,
            scroll_duration: EditorSettings::get_global(cx)
                .smooth_scroll_duration
                .max(0.) as f64,
        }
    }

    pub(crate) fn start(&mut self, anim: Anim) {
        // Trackpads and fast wheels emit scroll events faster than the
        // configured animation duration. Restarting the animation on every
        // event would cancel each one before it could run more than a few
        // percent of its duration, throttling visible scroll to a tiny
        // fraction of the requested delta. Instead, when a new scroll event
        // arrives in the same direction as the in-flight animation, extend
        // the existing animation's destination and duration proportionally
        // so motion continues at the same rate; reverse direction still
        // restarts so reversals feel snappy.
        if let Some(existing) = self.anim.as_mut()
            && let Some((extended_delta, extended_duration)) = extend_in_flight(
                existing.start,
                existing.delta,
                existing.duration,
                existing.start_moment.elapsed().as_secs_f64(),
                anim.start,
                anim.delta,
            )
        {
            existing.delta = extended_delta;
            existing.duration = extended_duration;
            existing.destination_top_row = anim.destination_top_row;
            existing.destination_anchor = anim.destination_anchor;
            existing.state = anim.state;
            return;
        }
        self.anim = Some(anim);
    }

    pub(crate) fn set_duration(&mut self, new_dur: f32) {
        self.scroll_duration = new_dur.max(0.) as f64;
    }

    pub(crate) fn scroll_duration(&self) -> f64 {
        self.scroll_duration
    }

    pub(crate) fn has_anim(&self) -> bool {
        self.anim.is_some()
    }

    pub(crate) fn get_state(&self) -> Option<PersistentState> {
        self.anim.as_ref().map(|v| v.state.clone())
    }

    fn make_final_results(
        &self,
        intermediate_scroll_top: f64,
        map: &DisplaySnapshot,
    ) -> (ScrollAnchor, u32) {
        // the logic here is roughly the same as what you'd find in
        // [ScrollManager::set_scroll_position()]
        // the idea is to build objects that [ScrollManager::set_anchor()] can exploit
        // using our calculated intermediate_scroll_top
        let scroll_top_buffer_point =
            DisplayPoint::new(DisplayRow(intermediate_scroll_top as u32), 0).to_point(map);
        let new_top_anchor = map
            .buffer_snapshot()
            .anchor_at(scroll_top_buffer_point, Bias::Right);

        (
            ScrollAnchor {
                anchor: new_top_anchor,
                offset: point(
                    // no horizontal scrolling yet ...
                    self.anim.as_ref().unwrap().destination_anchor.offset.x,
                    intermediate_scroll_top - new_top_anchor.to_display_point(map).row().0 as f64,
                ),
            },
            scroll_top_buffer_point.row,
        )
    }

    pub(crate) fn update(&mut self) -> UpdateResponse {
        if let Some(anim) = &self.anim {
            let time_since_start = anim.start_moment.elapsed().as_secs_f64();
            if time_since_start >= anim.duration {
                let anim = self.anim.take().unwrap();
                UpdateResponse::Finished {
                    destination_top_row: anim.destination_top_row,
                    destination_anchor: anim.destination_anchor,
                    state: anim.state,
                }
            } else {
                let new_scroll_top = anim.start + (anim.delta * time_since_start / anim.duration);

                let (intermediate_anchor, intermediate_top_row) =
                    self.make_final_results(new_scroll_top, &anim.state.map);

                UpdateResponse::RequiresAnimationFrame {
                    intermediate_anchor,
                    intermediate_top_row,
                }
            }
        } else {
            UpdateResponse::Nothing
        }
    }
}

/// Decide whether an in-flight animation should be extended by a new event
/// arriving in the same direction. Returns the extended `(delta, duration)`
/// pair, or `None` if the caller should replace the animation instead.
///
/// `existing_*` describes the in-flight animation (origin start, total delta,
/// configured duration). `elapsed` is how far that animation has progressed.
/// `new_start` / `new_delta` describe the new scroll request (start is the
/// current absolute y position; delta is the requested change from there).
fn extend_in_flight(
    existing_start: f64,
    existing_delta: f64,
    existing_duration: f64,
    elapsed: f64,
    new_start: f64,
    new_delta: f64,
) -> Option<(f64, f64)> {
    if existing_duration <= 0.0 || existing_delta.abs() <= f64::EPSILON {
        return None;
    }
    let current_pos = existing_start + existing_delta * elapsed / existing_duration;
    let new_end = new_start + new_delta;
    let additional = new_end - current_pos;
    // Same direction: extend. Reversal (or no movement): caller replaces.
    if additional.signum() != existing_delta.signum() {
        return None;
    }
    let rate = existing_delta / existing_duration;
    let remaining = additional / rate;
    Some((new_end - existing_start, elapsed + remaining))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extends_same_direction_keeps_rate_continuous() {
        // Existing animation: scroll 3 lines down over 0.25s (rate = -12 lines/s).
        // 50ms in: position = 10 + (-3) * 0.05 / 0.25 = 9.4
        // New event requests another 1 line down from 9.4 (new_start = 9.4, new_delta = -1).
        // expected new_end = 8.4, additional = -1
        // expected remaining at rate -12 = 1/12 ≈ 0.0833
        // expected extended duration = 0.05 + 0.0833 = 0.1333
        // expected extended delta = 8.4 - 10 = -1.6
        let (delta, duration) = extend_in_flight(
            10.0, // existing_start
            -3.0, // existing_delta
            0.25, // existing_duration
            0.05, // elapsed
            9.4,  // new_start
            -1.0, // new_delta
        )
        .expect("should extend in same direction");

        assert!((delta - (-1.6)).abs() < 1e-6, "delta = {delta}");
        assert!((duration - 0.1333).abs() < 1e-3, "duration = {duration}");
        // Rate preserved.
        assert!(((delta / duration) - (-12.0)).abs() < 0.1);
    }

    #[test]
    fn does_not_extend_on_direction_reversal() {
        // Scrolling down (-3) then user reverses to scroll up.
        let result = extend_in_flight(
            10.0, // existing_start
            -3.0, // existing_delta (down)
            0.25, // existing_duration
            0.05, // elapsed
            9.4,  // new_start (current position)
            1.0,  // new_delta (up)
        );
        assert!(result.is_none(), "reversal should restart, not extend");
    }

    #[test]
    fn does_not_extend_zero_rate_animation() {
        // Existing animation didn't move (delta == 0). No rate to preserve.
        let result = extend_in_flight(10.0, 0.0, 0.25, 0.05, 10.0, -1.0);
        assert!(result.is_none());
    }

    #[test]
    fn does_not_extend_finished_animation() {
        let result = extend_in_flight(
            10.0, -3.0, 0.0, // zero duration == already finished
            0.0, 9.4, -1.0,
        );
        assert!(result.is_none());
    }
}
