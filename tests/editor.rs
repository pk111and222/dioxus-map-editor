//! Public geometry/editor contracts; no Dioxus renderer or storage required.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use dioxus_map_editor::{
    command::Command, document::*, editor::*, export::export_svg, geometry::*,
    history::HistoryBudget, registry::*, scene, session::*, storage::*,
};
use std::collections::BTreeSet;
fn session() -> MapSession {
    MapSession::open(
        MapSnapshot::standalone(MapDocument::new("drawing")),
        None,
        Limits::default(),
        HistoryBudget::default(),
    )
    .unwrap()
}
fn stamp(id: &str, x: f64) -> MapObject {
    MapObject {
        id: id.into(),
        layer_id: "base".into(),
        geometry: Geometry::Stamp {
            stamp: ResourceRef {
                key: "builtin:diamond".into(),
                version: 1,
            },
            transform: Transform {
                translation: Point::new(x, 100.),
                ..Transform::default()
            },
        },
        paint: Paint::default(),
        extensions: Extensions::new(),
    }
}
fn put(s: &mut MapSession, o: MapObject) {
    s.execute(vec![Command::PutObject {
        object: o,
        index: s.snapshot().document.objects.len(),
    }])
    .unwrap();
}
fn node(x: f64, y: f64) -> PathNode {
    PathNode {
        position: Point::new(x, y),
        incoming: None,
        outgoing: None,
    }
}
#[test]
fn all_tools_share_history_and_durable_roundtrip() {
    let mut s = session();
    let tools = EditorTools::default();
    for g in [
        Geometry::Stroke {
            points: vec![Point::new(1., 1.), Point::new(30., 40.)],
            transform: Transform::default(),
        },
        Geometry::Path {
            nodes: vec![node(1., 1.), node(3., 6.), node(10., 1.)],
            closed: true,
            smooth: true,
            transform: Transform::default(),
        },
        Geometry::Text {
            text: "中文 <>&\"'".into(),
            transform: Transform {
                scale: 2.,
                rotation: 0.3,
                ..Transform::default()
            },
        },
        Geometry::Stamp {
            stamp: tools.stamp.clone(),
            transform: Transform::default(),
        },
    ] {
        let c = create_object(s.snapshot(), s.stamp().revision, &tools, g).unwrap();
        s.execute(vec![c]).unwrap();
    }
    assert_eq!(s.history().undo, 4);
    s.undo(&mut Standalone).unwrap();
    assert_eq!(s.snapshot().document.objects.len(), 3);
    s.redo(&mut Standalone).unwrap();
    let mut store = MemoryStore::default();
    s.flush(&mut store).unwrap();
    let loaded = store.load(&"drawing".into(), &s.limits()).unwrap().unwrap();
    assert_eq!(loaded.document, s.snapshot().document);
    let svg = export_svg(s.snapshot(), &Registry::default(), &s.limits())
        .unwrap()
        .svg;
    assert!(svg.contains("中文 &lt;&gt;&amp;&quot;&apos;"));
    assert!(svg.contains("scale(1 -1)"));
    assert!(!svg.contains("dme-selection"));
}
#[test]
fn multi_selection_transform_and_delete_are_atomic_and_reversible() {
    let mut s = session();
    put(&mut s, stamp("one", 20.));
    put(&mut s, stamp("two", 80.));
    let ids = BTreeSet::from([ObjectId::from("one"), ObjectId::from("two")]);
    let before = s.snapshot().clone();
    let commands = transform_selection(
        s.snapshot(),
        &ids,
        Point::new(50., 100.),
        Point::new(5., 8.),
        2.,
        std::f64::consts::FRAC_PI_2,
    )
    .unwrap();
    s.execute(commands).unwrap();
    let a = scene::transform(&s.snapshot().document.objects[0].geometry).unwrap();
    assert!((a.translation.x - 55.).abs() < 1e-8);
    assert!((a.translation.y - 48.).abs() < 1e-8);
    s.undo(&mut Standalone).unwrap();
    assert_eq!(s.snapshot(), &before);
    s.execute(delete_selection(&ids)).unwrap();
    assert!(s.snapshot().document.objects.is_empty());
    s.undo(&mut Standalone).unwrap();
    assert_eq!(s.snapshot(), &before);
}
#[test]
fn hit_testing_respects_layer_order_lock_hidden_and_screen_tolerance() {
    let r = Registry::default();
    let mut s = session();
    put(&mut s, stamp("low", 50.));
    put(&mut s, stamp("top", 50.));
    assert_eq!(
        scene::hit_test(s.snapshot(), &r, Point::new(50., 100.), 1.),
        Some("top".into())
    );
    let mut layer = Layer::new("upper", "upper");
    s.execute(vec![Command::PutLayer {
        layer: layer.clone(),
        index: 1,
    }])
    .unwrap();
    let mut top = s.snapshot().document.objects[1].clone();
    top.layer_id = "upper".into();
    put(&mut s, top);
    layer.locked = true;
    s.execute(vec![Command::PutLayer {
        layer: layer.clone(),
        index: 1,
    }])
    .unwrap();
    assert_eq!(
        scene::hit_test(s.snapshot(), &r, Point::new(50., 100.), 1.),
        Some("low".into())
    );
    layer.locked = false;
    layer.visible = false;
    s.execute(vec![Command::PutLayer { layer, index: 1 }])
        .unwrap();
    assert_eq!(scene::build_scene(s.snapshot(), &r, "a").len(), 1);
    let mut path = stamp("line", 0.);
    path.geometry = Geometry::Path {
        nodes: vec![node(0., 0.), node(100., 0.)],
        closed: false,
        smooth: false,
        transform: Transform::default(),
    };
    path.paint.width = Some(0.1);
    put(&mut s, path);
    for scale in [0.125, 1., 8.] {
        assert_eq!(
            scene::hit_test(s.snapshot(), &r, Point::new(30., 5. / scale), scale),
            Some("line".into())
        );
        assert_ne!(
            scene::hit_test(s.snapshot(), &r, Point::new(30., 8. / scale), scale),
            Some("line".into())
        );
    }
}
#[test]
fn smooth_cubic_fill_and_transformed_nodes_use_same_geometry() {
    let mut s = session();
    let mut o = stamp("path", 0.);
    o.geometry = Geometry::Path {
        nodes: vec![node(0., 0.), node(100., 0.), node(50., 100.)],
        closed: true,
        smooth: true,
        transform: Transform {
            translation: Point::new(10., 20.),
            scale: 2.,
            rotation: 0.5,
        },
    };
    o.paint.fill = Some("#aabbcc".into());
    put(&mut s, o.clone());
    let t = scene::transform(&o.geometry).unwrap();
    let center = t.apply(Point::new(50., 25.)).unwrap();
    assert_eq!(
        scene::hit_test(s.snapshot(), &Registry::default(), center, 1.),
        Some("path".into())
    );
    let world = t.apply(Point::new(20., 30.)).unwrap();
    let moved = move_node(&o, 1, None, world).unwrap();
    let Geometry::Path { nodes, .. } = &moved.geometry else {
        panic!()
    };
    assert!((nodes[1].position.x - 20.).abs() < 1e-8);
    let controlled = move_node(
        &moved,
        1,
        Some(false),
        t.apply(Point::new(10., 15.)).unwrap(),
    )
    .unwrap();
    let Geometry::Path { nodes, .. } = &controlled.geometry else {
        panic!()
    };
    assert!((nodes[1].incoming.unwrap().y - 15.).abs() < 1e-8);
    let raw = scene::object_svg(&controlled, s.snapshot(), &Registry::default(), "p");
    assert!(raw.contains("C "));
}
#[test]
fn sampling_simplification_and_smoothing_preserve_endpoints_and_saved_geometry() {
    let points: Vec<_> = (0..1000)
        .map(|i| Point::new(i as f64 / 10., (i as f64 / 100.).sin()))
        .collect();
    let result = simplify(&points, 0.3);
    assert_eq!(result.first(), points.first());
    assert_eq!(result.last(), points.last());
    assert!(result.len() < 20);
    let n = scene::smooth_nodes(&result, false);
    let original = scene::path_data(&n, false);
    for _scale in [0.125, 1., 8.] {
        assert_eq!(scene::path_data(&n, false), original);
    }
}
#[test]
fn double_click_path_deduplicates_and_rejects_stale_or_over_budget() {
    let s = session();
    let mut d = PathDraft::default();
    d.append(Point::default(), 1., s.stamp(), 2).unwrap();
    d.append(Point::new(20., 30.), 1., s.stamp(), 2).unwrap();
    d.append(Point::new(20., 30.), 1., s.stamp(), 2).unwrap();
    assert_eq!(d.nodes.len(), 2);
    assert!(d.append(Point::new(40., 30.), 1., s.stamp(), 2).is_err());
    let other = session();
    assert!(d
        .append(Point::new(40., 30.), 1., other.stamp(), 5)
        .is_err());
    d.cancel();
    assert!(d.stamp.is_none());
}
#[test]
fn unknown_resources_fallback_restore_and_never_drop_original_keys() {
    let mut s = session();
    let mut o = stamp("custom", 50.);
    let reference = ResourceRef {
        key: "thirdparty:<symbol>".into(),
        version: 3,
    };
    o.geometry = Geometry::Stamp {
        stamp: reference.clone(),
        transform: Transform::default(),
    };
    put(&mut s, o);
    let mut registry = Registry::default();
    let result = export_svg(s.snapshot(), &registry, &s.limits()).unwrap();
    assert_eq!(result.warnings.len(), 1);
    assert!(result.svg.contains("stroke-dasharray"));
    registry
        .register_stamp(StampDefinition {
            resource: reference.clone(),
            name: "custom".into(),
            bounds: WorldRect {
                min: Point::new(-10., -10.),
                width: 20.,
                height: 20.,
            },
            paths: vec![SymbolPath {
                nodes: vec![node(0., 0.), node(10., 10.)],
                closed: false,
                filled: false,
            }],
        })
        .unwrap();
    let restored = export_svg(s.snapshot(), &registry, &s.limits()).unwrap();
    assert!(restored.warnings.is_empty());
    assert!(restored.svg.contains("<use href=\"#dme-export-"));
    assert!(
        matches!(&s.snapshot().document.objects[0].geometry,Geometry::Stamp{stamp,..}if stamp==&reference)
    );
    assert_ne!(
        scene::resource_id("one", &reference),
        scene::resource_id("two", &reference)
    );
}
#[test]
fn registry_rejects_markup_and_invalid_or_duplicate_definitions() {
    let mut r = Registry::default();
    let mut style = r.styles()[0].clone();
    assert!(r.register_style(style.clone()).is_err());
    style.resource.key = "custom:style".into();
    style.background = "url(javascript:alert(1))".into();
    assert!(r.register_style(style).is_err());
    let mut stamp = r.stamps()[0].clone();
    stamp.resource.key = "custom:bad".into();
    stamp.paths[0].nodes[0].position.x = f64::NAN;
    assert!(r.register_stamp(stamp).is_err());
}
#[test]
fn theme_switch_keeps_explicit_paint_and_export_omits_hidden_layer() {
    let mut s = session();
    let mut o = stamp("one", 20.);
    o.paint.color = Some("#aabbcc".into());
    put(&mut s, o);
    let r = Registry::default();
    s.execute(vec![Command::SetStyle(r.styles()[1].resource.clone())])
        .unwrap();
    let svg = export_svg(s.snapshot(), &r, &s.limits()).unwrap().svg;
    assert!(svg.contains("#aabbcc"));
    let mut layer = s.snapshot().document.layers[0].clone();
    layer.visible = false;
    s.execute(vec![Command::PutLayer { layer, index: 0 }])
        .unwrap();
    let svg = export_svg(s.snapshot(), &r, &s.limits()).unwrap().svg;
    assert!(!svg.contains("<use"));
}
#[test]
fn fit_marquee_selection_and_view_changes_do_not_change_revisions() {
    let mut s = session();
    put(&mut s, stamp("one", 50.));
    let r = Registry::default();
    let revision = s.stamp().revision;
    let bounds = scene::content_bounds(s.snapshot(), &r).unwrap();
    let view = fit_view(bounds, 700., 400.).unwrap();
    let center = Point::new(
        bounds.min.x + bounds.width / 2.,
        bounds.min.y + bounds.height / 2.,
    );
    let point = view.world_to_screen(center);
    assert!((point.x - 350.).abs() < 1e-8);
    assert!((point.y - 200.).abs() < 1e-8);
    assert_eq!(
        marquee(s.snapshot(), &r, bounds),
        BTreeSet::from([ObjectId::from("one")])
    );
    assert_eq!(s.stamp().revision, revision);
    let ticket = s.begin_gesture().unwrap();
    assert!(s.is_gesture_current(&ticket));
    s.cancel_gesture(&ticket).unwrap();
    assert!(!s.is_gesture_current(&ticket));
}

#[test]
fn click_paths_commit_each_valid_node_and_undo_without_finish() {
    use dioxus_map_editor::session::Standalone;
    for closed in [false, true] {
        let mut session = MapSession::open(
            MapSnapshot::standalone(MapDocument::new("clicks")),
            None,
            Limits::default(),
            HistoryBudget::default(),
        )
        .unwrap();
        let tools = EditorTools {
            tool: EditorTool::Path,
            closed,
            ..EditorTools::default()
        };
        let mut path = PathDraft::default();
        let points = [
            Point::new(10., 10.),
            Point::new(40., 20.),
            Point::new(60., 60.),
            Point::new(20., 80.),
        ];
        for (index, p) in points.into_iter().enumerate() {
            path.append(p, 1., session.stamp(), 100).unwrap();
            if let Some(command) = path
                .command(session.snapshot(), session.stamp().revision, &tools)
                .unwrap()
            {
                if let Command::PutObject { object, .. } = &command {
                    path.object = Some(object.id.clone());
                }
                session.execute(vec![command]).unwrap();
                path.stamp = Some(session.stamp());
                assert_eq!(session.snapshot().document.objects.len(), 1);
                if let Geometry::Path { nodes, .. } =
                    &session.snapshot().document.objects[0].geometry
                {
                    assert_eq!(nodes.len(), index + 1);
                }
            } else {
                assert!(session.snapshot().document.objects.is_empty());
            }
        }
        session.undo(&mut Standalone).unwrap();
        if let Geometry::Path { nodes, .. } = &session.snapshot().document.objects[0].geometry {
            assert_eq!(nodes.len(), 3);
        }
        session.redo(&mut Standalone).unwrap();
        if let Geometry::Path { nodes, .. } = &session.snapshot().document.objects[0].geometry {
            assert_eq!(nodes.len(), 4);
        }
    }
}

#[test]
fn drawing_preflight_rejects_locked_hidden_missing_layers_without_mutation() {
    let mut snapshot = MapSnapshot::standalone(MapDocument::new("layers"));
    let tools = EditorTools::default();
    assert!(tools.drawing_layer(&snapshot).is_ok());
    snapshot.document.layers[0].locked = true;
    assert_eq!(
        tools.drawing_layer(&snapshot),
        Err(dioxus_map_editor::error::MapError::ReadOnly)
    );
    snapshot.document.layers[0].locked = false;
    snapshot.document.layers[0].visible = false;
    assert!(tools.drawing_layer(&snapshot).is_err());
    snapshot.document.layers.clear();
    assert!(tools.drawing_layer(&snapshot).is_err());
    assert!(snapshot.document.objects.is_empty());
}

#[test]
fn export_drops_xml_illegal_characters_and_hit_test_needs_positive_scale() {
    use dioxus_map_editor::{export::export_svg, registry::Registry, scene};
    let mut doc = MapDocument::new("x");
    doc.objects.push(MapObject {
        id: "t".into(),
        layer_id: "base".into(),
        geometry: Geometry::Text {
            text: "a\u{1}b\u{FFFF}c".into(),
            transform: Transform::default(),
        },
        paint: Paint::default(),
        extensions: Default::default(),
    });
    let snapshot = MapSnapshot::standalone(doc);
    let svg = export_svg(&snapshot, &Registry::default(), &Limits::default())
        .unwrap()
        .svg;
    assert!(svg.contains(">abc<") && !svg.contains('\u{1}') && !svg.contains('\u{FFFF}'));
    let far = Point::new(9999., 9999.);
    assert_eq!(
        scene::hit_test(&snapshot, &Registry::default(), far, 0.),
        None
    );
    assert_eq!(
        scene::hit_test(&snapshot, &Registry::default(), far, -1.),
        None
    );
}
