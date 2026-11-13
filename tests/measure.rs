//! Measurement helpers: bearings, lengths, scale and anchor lookup.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use dioxus_map_editor::{
    document::{AnchorPosition, ExternalAnchor, MapDocument, MapSnapshot},
    geometry::{PathNode, Point},
    measure::*,
    session::MapSession,
};

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn bearings_follow_north_up_clockwise() {
    let o = Point::new(0., 0.);
    assert!(close(bearing(o, Point::new(0., 5.)).unwrap(), 0.));
    assert!(close(bearing(o, Point::new(5., 0.)).unwrap(), 90.));
    assert!(close(bearing(o, Point::new(0., -5.)).unwrap(), 180.));
    assert!(close(bearing(o, Point::new(-5., 0.)).unwrap(), 270.));
    assert_eq!(bearing(o, o), None);
    assert_eq!(Compass::from_bearing(44.), Some(Compass::NorthEast));
    assert_eq!(Compass::from_bearing(350.), Some(Compass::North));
    assert_eq!(Compass::from_bearing(f64::NAN), None);
    assert!(close(Compass::SouthWest.degrees(), 225.));
    assert!(close(bearing_difference(350., 10.), 20.));
}

#[test]
fn lengths_match_straight_and_curved_paths() {
    let line = [Point::new(0., 0.), Point::new(3., 4.), Point::new(3., 10.)];
    assert!(close(polyline_length(&line), 11.));
    let nodes: Vec<PathNode> = line
        .iter()
        .map(|p| PathNode {
            position: *p,
            incoming: None,
            outgoing: None,
        })
        .collect();
    assert!(close(path_length(&nodes, false, 8), 11.));
    // Quarter circle of radius 10 as one cubic: length ≈ 15.708.
    let k = 0.552_284_75 * 10.;
    let arc = [
        PathNode {
            position: Point::new(10., 0.),
            incoming: None,
            outgoing: Some(Point::new(10., k)),
        },
        PathNode {
            position: Point::new(0., 10.),
            incoming: Some(Point::new(k, 10.)),
            outgoing: None,
        },
    ];
    assert!((path_length(&arc, false, 64) - std::f64::consts::FRAC_PI_2 * 10.).abs() < 0.01);
}

#[test]
fn scale_is_an_undoable_document_extension() {
    let mut session = MapSession::open(
        MapSnapshot::standalone(MapDocument::new("m")),
        None,
        Default::default(),
        Default::default(),
    )
    .unwrap();
    let scale = MapScale {
        units_per_world: 0.5,
        unit: "km".into(),
    };
    session
        .execute(vec![MapScale::command(Some(&scale)).unwrap()])
        .unwrap();
    let read = MapScale::read(&session.snapshot().document.extensions).unwrap();
    assert_eq!(read, scale);
    let m = measure(session.snapshot(), Point::new(0., 0.), Point::new(30., 40.));
    assert!(close(m.world, 50.) && close(m.real.unwrap(), 25.));
    assert_eq!(m.compass, Some(Compass::NorthEast));
    session
        .undo(&mut dioxus_map_editor::session::Standalone)
        .unwrap();
    assert_eq!(
        MapScale::read(&session.snapshot().document.extensions),
        None
    );
    assert!(MapScale::command(Some(&MapScale {
        units_per_world: 0.,
        unit: "km".into()
    }))
    .is_err());
}

#[test]
fn anchors_resolve_only_on_their_own_map() {
    let mut s = MapSnapshot::standalone(MapDocument::new("m"));
    for (id, canvas) in [("here", "m"), ("elsewhere", "other")] {
        s.anchors.insert(
            id.into(),
            ExternalAnchor {
                id: id.into(),
                name: id.into(),
                parent_id: Some("m".into()),
                position: Some(AnchorPosition {
                    point: Point::new(1., 2.),
                    z: None,
                    canvas: canvas.into(),
                }),
                metadata: Default::default(),
            },
        );
    }
    assert_eq!(
        anchor_position(&s, &"here".into()),
        Some(Point::new(1., 2.))
    );
    assert_eq!(anchor_position(&s, &"elsewhere".into()), None);
}
