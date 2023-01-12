use crate::util::curve::{Curve, CurveBuffers};

use super::stars::OsuDifficultyAttributes;

use rosu_pp::{
    parse::{HitObject, HitObjectKind, Pos2},
    Beatmap,
};

const LEGACY_LAST_TICK_OFFSET: f64 = 36.0;
const BASE_SCORING_DISTANCE: f64 = 100.0;

pub(crate) struct OsuObject {
    pub(crate) time: f32,
    pub(crate) pos: Pos2,
    pub(crate) end_pos: Pos2,
    // circle: Some(0.0) | slider: Some(_) | spinner: None
    pub(crate) travel_dist: Option<f32>,
}

impl OsuObject {
    pub(crate) fn new(
        h: &HitObject,
        map: &Beatmap,
        radius: f32,
        scaling_factor: f32,
        ticks: &mut Vec<f64>,
        attrs: &mut OsuDifficultyAttributes,
        curve_bufs: &mut CurveBuffers,
    ) -> Self {
        attrs.max_combo += 1; // hitcircle, slider head, or spinner

        match &h.kind {
            HitObjectKind::Circle => Self {
                time: h.start_time as f32,
                pos: h.pos,
                end_pos: h.pos,
                travel_dist: Some(0.0),
            },
            HitObjectKind::Slider {
                pixel_len,
                repeats,
                control_points,
                ..
            } => {
                attrs.n_sliders += 1;

                let timing_point = map.timing_point_at(h.start_time);
                let difficulty_point = map.difficulty_point_at(h.start_time).unwrap_or_default();

                let scoring_dist =
                    BASE_SCORING_DISTANCE * map.slider_mult * difficulty_point.slider_vel;
                let vel = scoring_dist / timing_point.beat_len;

                // Key values which are computed here
                let mut end_pos = h.pos;
                let mut travel_dist = 0.0;

                let approx_follow_circle_radius = radius * 3.0;

                let tick_dist_mult = if map.version < 8 {
                    difficulty_point.slider_vel.recip()
                } else {
                    1.0
                };

                let mut tick_dist = if difficulty_point.generate_ticks {
                    scoring_dist / map.tick_rate * tick_dist_mult
                } else {
                    f64::INFINITY
                };

                let span_count = (*repeats + 1) as f64;

                // Build the curve w.r.t. the curve points
                let curve = Curve::new(control_points, *pixel_len, curve_bufs);

                let end_time = h.start_time + span_count * curve.dist() / vel;
                let total_duration = end_time - h.start_time;
                let span_duration = total_duration / span_count;

                // Called on each slider object except for the head.
                // Increases combo and adjusts `end_pos` and `travel_dist`
                // w.r.t. the object position at the given time on the slider curve.
                let mut compute_vertex = |time: f64| {
                    attrs.max_combo += 1;

                    let mut progress = (time - h.start_time) / span_duration;

                    if progress % 2.0 >= 1.0 {
                        progress = 1.0 - progress % 1.0;
                    } else {
                        progress %= 1.0;
                    }

                    let curr_pos = h.pos + curve.position_at(progress);

                    let diff = curr_pos - end_pos;
                    let mut dist = diff.length();

                    if dist > approx_follow_circle_radius {
                        dist -= approx_follow_circle_radius;
                        end_pos += diff.normalize() * dist;
                        travel_dist += dist;
                    }
                };

                let max_len = 100_000.0;

                let len = curve.dist().min(max_len);
                tick_dist = tick_dist.clamp(0.0, len);
                let min_dist_from_end = vel * 10.0;

                let mut curr_dist = tick_dist;

                if tick_dist != 0.0 {
                    ticks.reserve((len / tick_dist) as usize);

                    // Ticks of the first span
                    while curr_dist < len - min_dist_from_end {
                        let progress = curr_dist / len;

                        let curr_time = h.start_time + progress * span_duration;
                        compute_vertex(curr_time);
                        ticks.push(curr_time);

                        curr_dist += tick_dist;
                    }

                    // Other spans
                    for span_idx in 1..=*repeats {
                        let span_idx_f64 = span_idx as f64;

                        // Repeat point
                        let curr_time = h.start_time + span_duration * span_idx_f64;
                        compute_vertex(curr_time);

                        let span_offset = span_idx_f64 * span_duration;

                        // Ticks
                        if span_idx & 1 == 1 {
                            let base = h.start_time + h.start_time + span_duration;

                            for time in ticks.iter().rev() {
                                compute_vertex(span_offset + base - time);
                            }
                        } else {
                            for time in ticks.iter() {
                                compute_vertex(span_offset + time);
                            }
                        }
                    }

                    ticks.clear();
                }

                // Slider tail
                let final_span_start_time = h.start_time + *repeats as f64 * span_duration;
                let final_span_end_time = (h.start_time + total_duration / 2.0)
                    .max(final_span_start_time + span_duration - LEGACY_LAST_TICK_OFFSET);
                compute_vertex(final_span_end_time);

                travel_dist *= scaling_factor;

                Self {
                    time: h.start_time as f32,
                    pos: h.pos,
                    end_pos,
                    travel_dist: Some(travel_dist),
                }
            }
            HitObjectKind::Spinner { .. } | HitObjectKind::Hold { .. } => Self {
                time: h.start_time as f32,
                pos: h.pos,
                end_pos: h.pos,
                travel_dist: None,
            },
        }
    }

    #[inline]
    pub(crate) fn is_spinner(&self) -> bool {
        self.travel_dist.is_none()
    }
}
