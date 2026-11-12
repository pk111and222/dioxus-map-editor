//! Geometry and gesture invariants shared by both renderers.
use dioxus_map_editor::geometry::{Point, Viewport};
use dioxus_map_editor::interaction::{
    CancelReason, GestureResult, Interaction, Tool, MAX_STROKE_POINTS,
};

fn near(a: Point, b: Point) {
    assert!((a.x - b.x).abs() < 1e-8, "{a:?} != {b:?}");
    assert!((a.y - b.y).abs() < 1e-8, "{a:?} != {b:?}");
}

#[test]
fn world_screen_roundtrip_inverts_y() {
    let mut view = Viewport::default();
    assert!(view.zoom_at(Point::new(90.0, 130.0), 2.7).is_ok());
    assert!(view.pan(Point::new(-81.0, 200.0)).is_ok());
    for point in [
        Point::new(-1234.0, -99.0),
        Point::new(0.0, 0.0),
        Point::new(812.0, 999.0),
    ] {
        near(view.screen_to_world(view.world_to_screen(point)), point);
    }
    assert!(view.world_to_screen(Point::new(0.0, 1.0)).y < view.origin().y);
}

#[test]
fn pointer_zoom_is_anchored_even_at_limits() {
    let mut view = Viewport::default();
    let pointer = Point::new(201.0, 99.0);
    let world = view.screen_to_world(pointer);
    for factor in [1.5, 0.5, 1e8, 1e-9, 4.0] {
        assert!(view.zoom_at(pointer, factor).is_ok());
        near(view.screen_to_world(pointer), world);
        assert!((Viewport::MIN_SCALE..=Viewport::MAX_SCALE).contains(&view.scale()));
    }
}

#[test]
fn invalid_transform_is_atomic() {
    let mut view = Viewport::default();
    let before = view;
    for factor in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(view.zoom_at(Point::new(2.0, 3.0), factor).is_err());
        assert_eq!(view, before);
    }
    assert!(view.pan(Point::new(f64::NAN, 0.0)).is_err());
    assert_eq!(view, before);
}

#[test]
fn stroke_completes_once_with_release_endpoint() {
    let mut input = Interaction::default();
    let mut view = Viewport::default();
    assert!(input.begin(7, Tool::Brush, Point::new(50.0, 50.0), view));
    input.update(7, Point::new(51.0, 51.0), true, &mut view);
    let result = input.finish(7, Point::new(53.0, 51.0), &mut view);
    let GestureResult::Stroke(stroke) = result else {
        panic!("expected stroke")
    };
    assert_eq!(stroke.points.len(), 3);
    near(
        stroke.points[2],
        view.screen_to_world(Point::new(53.0, 51.0)),
    );
    assert_eq!(
        input.finish(7, Point::new(53.0, 51.0), &mut view),
        GestureResult::None
    );
    assert!(!input.is_active());
}

#[test]
fn foreign_pointer_cannot_modify_or_release_gesture() {
    let mut input = Interaction::default();
    let mut view = Viewport::default();
    assert!(input.begin(7, Tool::Brush, Point::new(50.0, 50.0), view));
    assert!(!input.begin(8, Tool::Brush, Point::new(90.0, 90.0), view));
    input.update(8, Point::new(90.0, 90.0), false, &mut view);
    input.finish(8, Point::new(90.0, 90.0), &mut view);
    assert_eq!(input.pointer_id(), Some(7));
    assert_eq!(input.preview_nodes(), 1);
}

#[test]
fn every_cancellation_discards_preview_and_allows_restart() {
    let mut input = Interaction::default();
    let mut view = Viewport::default();
    for reason in [
        CancelReason::Escape,
        CancelReason::PointerCancel,
        CancelReason::WindowBoundary,
        CancelReason::FocusLost,
        CancelReason::LayoutChanged,
        CancelReason::LostRelease,
        CancelReason::Reset,
    ] {
        assert!(input.begin(1, Tool::Brush, Point::new(5.0, 6.0), view));
        input.update(1, Point::new(30.0, 40.0), true, &mut view);
        assert_eq!(
            input.cancel(reason, &mut view),
            GestureResult::Cancelled(reason)
        );
        assert_eq!(input.preview(), "");
        assert_eq!(input.preview_nodes(), 0);
        assert_eq!(input.cancel(reason, &mut view), GestureResult::None);
    }
}

#[test]
fn lost_release_does_not_create_a_bridge_to_next_move() {
    let mut input = Interaction::default();
    let mut view = Viewport::default();
    input.begin(1, Tool::Brush, Point::new(0.0, 0.0), view);
    assert_eq!(
        input.update(1, Point::new(500.0, 500.0), false, &mut view),
        GestureResult::Cancelled(CancelReason::LostRelease)
    );
    assert_eq!(
        input.finish(1, Point::new(500.0, 500.0), &mut view),
        GestureResult::None
    );
}

#[test]
fn cancelled_pan_restores_start_view() {
    let mut input = Interaction::default();
    let mut view = Viewport::default();
    let before = view;
    input.begin(1, Tool::Pan, Point::new(5.0, 6.0), view);
    input.update(1, Point::new(30.0, 40.0), true, &mut view);
    assert_ne!(view, before);
    input.cancel(CancelReason::FocusLost, &mut view);
    assert_eq!(view, before);
}

#[test]
fn finished_pan_includes_release_position() {
    let mut input = Interaction::default();
    let mut view = Viewport::default();
    let origin = view.origin();
    input.begin(1, Tool::Pan, Point::new(5.0, 6.0), view);
    input.update(1, Point::new(30.0, 40.0), true, &mut view);
    assert_eq!(
        input.finish(1, Point::new(35.0, 50.0), &mut view),
        GestureResult::None
    );
    near(view.origin(), Point::new(origin.x + 30.0, origin.y + 44.0));
}

#[test]
fn sampling_threshold_uses_screen_pixels() {
    for factor in [Viewport::MIN_SCALE, Viewport::MAX_SCALE] {
        let mut input = Interaction::default();
        let mut view = Viewport::default();
        assert!(view.zoom_at(Point::default(), factor).is_ok());
        input.begin(1, Tool::Brush, Point::new(0.0, 0.0), view);
        input.update(1, Point::new(0.4, 0.0), true, &mut view);
        assert_eq!(input.preview_nodes(), 1);
        input.update(1, Point::new(0.8, 0.0), true, &mut view);
        assert_eq!(input.preview_nodes(), 2);
    }
}

#[test]
fn budget_completes_without_unbounded_growth() {
    let mut input = Interaction::default();
    let mut view = Viewport::default();
    input.begin(1, Tool::Brush, Point::new(0.0, 0.0), view);
    for x in 1..MAX_STROKE_POINTS {
        let result = input.update(1, Point::new(x as f64, 0.0), true, &mut view);
        if x == MAX_STROKE_POINTS - 1 {
            let GestureResult::BudgetReached(stroke) = result else {
                panic!("expected budget result")
            };
            assert_eq!(stroke.points.len(), MAX_STROKE_POINTS);
        }
    }
    assert!(!input.is_active());
}

#[test]
fn instances_are_isolated() {
    let mut first = Interaction::default();
    let second = Interaction::default();
    assert!(first.begin(1, Tool::Brush, Point::default(), Viewport::default()));
    assert!(!second.is_active());
    assert_eq!(second.preview(), "");
}

#[test]
fn invalid_samples_do_not_corrupt_svg() {
    let mut input = Interaction::default();
    let mut view = Viewport::default();
    assert!(!input.begin(1, Tool::Brush, Point::new(f64::NAN, 0.0), view));
    assert!(input.begin(1, Tool::Brush, Point::default(), view));
    input.update(1, Point::new(f64::INFINITY, 1.0), true, &mut view);
    assert_eq!(input.preview_nodes(), 1);
    assert!(!input.preview().contains("inf"));
    assert!(!input.preview().contains("NaN"));
}
