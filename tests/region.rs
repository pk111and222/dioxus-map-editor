//! Region edge geometry must be durable, deterministic and share exact curves.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use dioxus_map_editor::{
    command::Command, document::*, editor::*, export::export_svg, geometry::*, region::*,
    registry::Registry, session::*,
};
fn node(x: f64, y: f64) -> PathNode {
    PathNode {
        position: Point::new(x, y),
        incoming: None,
        outgoing: None,
    }
}
fn source() -> MapObject {
    MapObject {
        id: "coast".into(),
        layer_id: "base".into(),
        geometry: Geometry::Path {
            nodes: vec![
                node(0., 0.),
                node(100., 0.),
                node(100., 100.),
                node(0., 100.),
            ],
            smooth: false,
            closed: true,
            transform: Transform::default(),
        },
        paint: Paint::default(),
        extensions: Extensions::new(),
    }
}
fn snapshot() -> MapSnapshot {
    let mut s = MapSnapshot::standalone(MapDocument::new("edges"));
    s.document.objects.push(source());
    s
}
fn controls(nodes: &[PathNode]) -> Vec<Point> {
    nodes
        .iter()
        .flat_map(|n| {
            [Some(n.position), n.incoming, n.outgoing]
                .into_iter()
                .flatten()
        })
        .collect()
}
#[test]
fn natural_edges_are_repeatable_bounded_and_saved_without_load_randomness() {
    let mut s = MapSession::open(snapshot(), None, Limits::default(), Default::default()).unwrap();
    let mut tools = EditorTools {
        closed: true,
        smooth: true,
        ..Default::default()
    };
    tools.region.roughness = 7.;
    tools.region.detail = 8.;
    let samples = vec![
        Point::new(130., 20.),
        Point::new(240., 20.),
        Point::new(220., 130.),
        Point::new(120., 120.),
    ];
    let a = brush_commands(
        s.snapshot(),
        s.stamp().revision,
        &tools,
        &samples,
        2.,
        &s.limits(),
    )
    .unwrap();
    let object = |commands: Vec<Command>| {
        commands
            .into_iter()
            .find_map(|c| {
                if let Command::PutObject { object, .. } = c {
                    Some(object)
                } else {
                    None
                }
            })
            .unwrap()
    };
    assert_eq!(
        object(
            brush_commands(
                s.snapshot(),
                s.stamp().revision,
                &tools,
                &samples,
                2.,
                &s.limits()
            )
            .unwrap()
        ),
        object(
            brush_commands(
                s.snapshot(),
                s.stamp().revision,
                &tools,
                &samples,
                2.,
                &s.limits()
            )
            .unwrap()
        )
    );
    tools.region.seed = 987;
    assert_ne!(
        object(
            brush_commands(
                s.snapshot(),
                s.stamp().revision,
                &tools,
                &samples,
                2.,
                &s.limits()
            )
            .unwrap()
        ),
        object(
            brush_commands(
                s.snapshot(),
                s.stamp().revision,
                &EditorTools {
                    region: RegionSettings {
                        seed: 1,
                        ..tools.region.clone()
                    },
                    ..tools.clone()
                },
                &samples,
                2.,
                &s.limits()
            )
            .unwrap()
        )
    );
    s.execute(a).unwrap();
    let saved = s.snapshot().clone();
    s.undo(&mut Standalone).unwrap();
    s.redo(&mut Standalone).unwrap();
    assert_eq!(&saved, s.snapshot());
    let json = serde_json::to_string(&saved.document).unwrap();
    let doc: MapDocument = serde_json::from_str(&json).unwrap();
    assert_eq!(doc, saved.document);
    assert!(export_svg(&saved, &Registry::default(), &s.limits())
        .unwrap()
        .svg
        .contains("C "));
    let limits = Limits {
        max_object_nodes: 12,
        ..Default::default()
    };
    assert!(brush_commands(
        s.snapshot(),
        s.stamp().revision,
        &tools,
        &samples,
        2.,
        &limits
    )
    .is_err());
}
#[test]
fn snap_reads_locked_visible_guides_and_ignores_hidden_self_and_ribbons() {
    let mut s = snapshot();
    s.document.layers[0].locked = true;
    let settings = RegionSettings {
        alignment: EdgeAlignment::Snap,
        snap_distance: 8.,
        ..Default::default()
    };
    let input = vec![node(103., 20.), node(105., 75.), node(150., 60.)];
    let out = region_nodes(&s, &input, false, &settings, None, &Limits::default()).unwrap();
    assert!((out[0].position.x - 100.).abs() < 1e-8);
    assert_eq!(out[2].position, input[2].position);
    assert!(snap_point(&s, Point::new(103., 20.), &settings, Some(&"coast".into())).is_none());
    s.document.layers[0].visible = false;
    assert!(snap_point(&s, Point::new(103., 20.), &settings, None).is_none());
    s.document.layers[0].visible = true;
    s.document.objects[0]
        .extensions
        .insert("map:ribbon".into(), serde_json::json!({"version":1}));
    assert!(snap_point(&s, Point::new(103., 20.), &settings, None).is_none());
}
#[test]
fn shared_border_copies_source_corners_instead_of_diagonal_chord() {
    let s = snapshot();
    let settings = RegionSettings {
        alignment: EdgeAlignment::Shared,
        snap_distance: 5.,
        ..Default::default()
    };
    let input = vec![node(98., 20.), node(20., 102.), node(150., 160.)];
    let out = region_nodes(&s, &input, false, &settings, None, &Limits::default()).unwrap();
    assert!(out
        .iter()
        .any(|n| (n.position.x - 100.).abs() < 1e-5 && (n.position.y - 100.).abs() < 1e-5));
    // Naturalizing the other edges must never displace the shared corner.
    let rough = RegionSettings {
        roughness: 6.,
        ..settings
    };
    let out = region_nodes(&s, &input, false, &rough, None, &Limits::default()).unwrap();
    assert!(out
        .iter()
        .any(|n| (n.position.x - 100.).abs() < 1e-5 && (n.position.y - 100.).abs() < 1e-5));
}
#[test]
fn transformed_cubic_shared_segment_preserves_bezier_controls_in_both_directions() {
    let mut s = snapshot();
    let transform = Transform {
        translation: Point::new(40., 50.),
        scale: 1.8,
        rotation: 0.4,
    };
    let a = PathNode {
        outgoing: Some(Point::new(30., 40.)),
        ..node(0., 0.)
    };
    let b = PathNode {
        incoming: Some(Point::new(70., 40.)),
        ..node(100., 0.)
    };
    s.document.objects[0].geometry = Geometry::Path {
        nodes: vec![a.clone(), b.clone(), node(100., -100.), node(0., -100.)],
        closed: true,
        smooth: false,
        transform,
    };
    let start = transform.apply(a.position).unwrap();
    let end = transform.apply(b.position).unwrap();
    let far = transform.apply(Point::new(50., 150.)).unwrap();
    let settings = RegionSettings {
        alignment: EdgeAlignment::Shared,
        snap_distance: 1.,
        ..Default::default()
    };
    for reverse in [false, true] {
        let (p, q) = if reverse { (end, start) } else { (start, end) };
        let input = vec![node(p.x, p.y), node(q.x, q.y), node(far.x, far.y)];
        let out = region_nodes(&s, &input, false, &settings, None, &Limits::default()).unwrap();
        let c = if reverse {
            b.incoming.unwrap()
        } else {
            a.outgoing.unwrap()
        };
        let d = if reverse {
            a.outgoing.unwrap()
        } else {
            b.incoming.unwrap()
        };
        assert!((out[0].outgoing.unwrap().x - transform.apply(c).unwrap().x).abs() < 1e-6);
        assert!((out[1].incoming.unwrap().y - transform.apply(d).unwrap().y).abs() < 1e-6);
        assert!(controls(&out).iter().all(|p| p.is_finite()));
    }
}
#[test]
fn invalid_settings_and_click_expansion_respect_host_budget() {
    let s = snapshot();
    let input = vec![node(200., 0.), node(300., 0.), node(300., 100.)];
    let mut settings = RegionSettings {
        roughness: f64::NAN,
        ..Default::default()
    };
    assert!(region_nodes(&s, &input, true, &settings, None, &Limits::default()).is_err());
    settings.roughness = 5.;
    let tools = EditorTools {
        closed: true,
        region: settings,
        ..Default::default()
    };
    let draft = PathDraft {
        nodes: input,
        ..Default::default()
    };
    assert!(draft
        .command_with_limits(
            &s,
            0,
            &tools,
            &Limits {
                max_object_nodes: 8,
                ..Default::default()
            }
        )
        .is_err());
}

#[test]
fn partial_cubic_border_matches_source_at_every_sample() {
    fn at(c: [Point; 4], t: f64) -> Point {
        let u = 1. - t;
        Point::new(
            u * u * u * c[0].x
                + 3. * u * u * t * c[1].x
                + 3. * u * t * t * c[2].x
                + t * t * t * c[3].x,
            u * u * u * c[0].y
                + 3. * u * u * t * c[1].y
                + 3. * u * t * t * c[2].y
                + t * t * t * c[3].y,
        )
    }
    let mut s = snapshot();
    let c = [
        Point::new(0., 0.),
        Point::new(30., 40.),
        Point::new(70., 40.),
        Point::new(100., 0.),
    ];
    s.document.objects[0].geometry = Geometry::Path {
        nodes: vec![
            PathNode {
                outgoing: Some(c[1]),
                ..node(0., 0.)
            },
            PathNode {
                incoming: Some(c[2]),
                ..node(100., 0.)
            },
            node(100., -100.),
            node(0., -100.),
        ],
        closed: true,
        smooth: false,
        transform: Transform::default(),
    };
    let settings = RegionSettings {
        alignment: EdgeAlignment::Shared,
        snap_distance: 1.,
        ..Default::default()
    };
    for (a, b) in [(0.2, 0.7), (0.7, 0.2)] {
        let p = at(c, a);
        let q = at(c, b);
        let input = vec![node(p.x, p.y), node(q.x, q.y), node(50., 150.)];
        let out = region_nodes(&s, &input, false, &settings, None, &Limits::default()).unwrap();
        let shared = [
            out[0].position,
            out[0].outgoing.unwrap(),
            out[1].incoming.unwrap(),
            out[1].position,
        ];
        for i in 0..=20 {
            let t = i as f64 / 20.;
            let actual = at(shared, t);
            let expected = at(c, a + (b - a) * t);
            assert!((actual.x - expected.x).hypot(actual.y - expected.y) < 0.001);
        }
    }
}
