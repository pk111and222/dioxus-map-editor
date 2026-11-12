//! Map-mode intents share the session, validation, undo and persistence boundary.
#![allow(clippy::unwrap_used)]
use dioxus_map_editor::{
    command::*, document::*, editor::*, export::export_svg, geometry::*, history::*, map::*,
    registry::*, scene, session::*, storage::*,
};
use std::collections::{BTreeMap, BTreeSet};
fn base() -> MapSnapshot {
    let mut s = MapSnapshot::standalone(MapDocument::new("map"));
    for (id, point, parent) in [
        ("a", Some(Point::new(10., 20.)), "map"),
        ("b", None, "map"),
        ("wrong", None, "other"),
    ] {
        s.anchors.insert(
            id.into(),
            ExternalAnchor {
                id: id.into(),
                name: format!("名字 {id}"),
                parent_id: Some(parent.into()),
                position: point.map(|point| AnchorPosition {
                    point,
                    z: Some(7.),
                    canvas: "map".into(),
                }),
                metadata: Extensions::new(),
            },
        );
    }
    s
}
#[test]
fn atlas_style_artwork_is_shared_by_scene_export_and_resource_versions() {
    let mut snapshot = base();
    snapshot.document.style = ResourceRef {
        key: "builtin:atlas".into(),
        version: 1,
    };
    snapshot.document.objects.push(MapObject {
        id: "island".into(),
        layer_id: "base".into(),
        geometry: Geometry::Path {
            nodes: vec![
                Point::new(20., 20.),
                Point::new(150., 20.),
                Point::new(80., 150.),
            ]
            .into_iter()
            .map(|position| PathNode {
                position,
                incoming: None,
                outgoing: None,
            })
            .collect(),
            closed: true,
            smooth: true,
            transform: Transform::default(),
        },
        paint: Paint {
            role: "land".into(),
            color: Some("#123456".into()),
            ..Paint::default()
        },
        extensions: Extensions::new(),
    });
    let registry = Registry::default();
    for key in ["builtin:pine", "builtin:ridge", "builtin:citadel"] {
        let definition = registry
            .stamp(&ResourceRef {
                key: key.into(),
                version: 1,
            })
            .unwrap()
            .clone();
        let mut custom = definition;
        custom.resource.key = format!("test:{key}");
        let mut copy = registry.clone();
        copy.register_stamp(custom).unwrap();
    }
    let svg = export_svg(&snapshot, &registry, &Limits::default())
        .unwrap()
        .svg;
    let scene = scene::build_scene(&snapshot, &registry, "export");
    assert!(svg.contains(&scene[0].svg));
    assert_eq!(svg.matches("data-coast").count(), 3);
    assert!(svg.contains("#123456"));
    assert!(svg.contains("patternUnits"));
    assert!(!scene::paper_svg(&snapshot, &registry, "export").is_empty());
    snapshot.document.style = ResourceRef {
        key: "builtin:paper".into(),
        version: 1,
    };
    assert!(!export_svg(&snapshot, &registry, &Limits::default())
        .unwrap()
        .svg
        .contains("data-coast"));
    assert!(scene::paper_svg(&snapshot, &registry, "export").is_empty());
    let mut invalid = registry.styles().last().unwrap().clone();
    invalid.resource.key = "test:invalid".into();
    invalid.artwork.coast_width = f64::NAN;
    assert!(registry.clone().register_style(invalid).is_err());
}
fn open(s: MapSnapshot) -> MapSession {
    MapSession::open(s, None, Limits::default(), HistoryBudget::default()).unwrap()
}
struct Host {
    anchors: BTreeMap<AnchorId, ExternalAnchor>,
    reject: bool,
}
impl AnchorTransaction for Host {
    fn accept(&mut self, patches: &[AnchorPatch]) -> dioxus_map_editor::error::MapResult<()> {
        if self.reject {
            return Err(dioxus_map_editor::error::MapError::HostRejected(
                "rejected".into(),
            ));
        }
        let mut next = self.anchors.clone();
        for p in patches {
            let a = next.get_mut(&p.id).unwrap();
            if a.parent_id != p.parent_id || a.position != p.before {
                return Err(dioxus_map_editor::error::MapError::Stale);
            }
            a.position = p.after.clone();
        }
        self.anchors = next;
        Ok(())
    }
}
#[test]
fn node_name_icon_offset_and_unknown_extensions_survive_history_and_roundtrip() {
    let mut s = open(base());
    let t = EditorTools {
        stamp: ResourceRef {
            key: "builtin:town".into(),
            version: 1,
        },
        ..EditorTools::default()
    };
    s.execute(
        create_node(
            s.snapshot(),
            0,
            &t,
            Point::new(100., 200.),
            "城镇 <>&".into(),
        )
        .unwrap(),
    )
    .unwrap();
    let o = &s.snapshot().document.objects[0];
    assert_eq!(node_label(o).unwrap().name, "城镇 <>&");
    let mut o = o.clone();
    o.extensions
        .insert("third:keep".into(), serde_json::json!({"x":1}));
    set_node_label(&mut o, "新城".into(), Point::new(20., 30.)).unwrap();
    s.execute(vec![Command::PutObject {
        object: o,
        index: 0,
    }])
    .unwrap();
    s.undo(&mut Standalone).unwrap();
    assert_eq!(
        node_label(&s.snapshot().document.objects[0]).unwrap().name,
        "城镇 <>&"
    );
    s.redo(&mut Standalone).unwrap();
    let raw = StoredMap::from_snapshot(s.snapshot())
        .encode(&s.limits())
        .unwrap();
    assert_eq!(
        StoredMap::decode(&raw, &s.limits()).unwrap().document,
        s.snapshot().document
    );
    let svg = export_svg(s.snapshot(), &Registry::default(), &s.limits())
        .unwrap()
        .svg;
    assert!(svg.contains("新城"));
}
#[test]
fn host_projection_no_duplication_status_and_persistence_does_not_copy_names() {
    let mut s = open(base());
    assert_eq!(projected_markers(s.snapshot()).len(), 1);
    assert_eq!(
        anchor_status(s.snapshot(), &s.snapshot().anchors[&AnchorId::from("b")]),
        AnchorStatus::Unlocated
    );
    assert_eq!(
        anchor_status(
            s.snapshot(),
            &s.snapshot().anchors[&AnchorId::from("wrong")]
        ),
        AnchorStatus::Invalid
    );
    let mut h = Host {
        anchors: s.snapshot().anchors.clone(),
        reject: false,
    };
    let c = place_anchor(
        s.snapshot(),
        0,
        &EditorTools::default(),
        &"a".into(),
        Point::new(30., 40.),
    )
    .unwrap();
    s.execute_with_host(c, &mut h).unwrap();
    assert!(projected_markers(s.snapshot()).is_empty());
    assert_eq!(
        scene::build_scene(s.snapshot(), &Registry::default(), "p").len(),
        1
    );
    let raw = StoredMap::from_snapshot(s.snapshot())
        .encode(&s.limits())
        .unwrap();
    assert!(!raw.contains("名字"));
    assert!(!raw.contains("anchor:a"));
    assert_eq!(
        s.snapshot().anchors[&AnchorId::from("a")]
            .position
            .as_ref()
            .unwrap()
            .z,
        Some(7.)
    );
}
#[test]
fn host_rejection_move_unlocate_lock_and_undo_are_atomic() {
    let mut s = open(base());
    let before = s.snapshot().clone();
    let mut h = Host {
        anchors: before.anchors.clone(),
        reject: true,
    };
    let commands = place_anchor(
        s.snapshot(),
        0,
        &EditorTools::default(),
        &"b".into(),
        Point::new(40., 60.),
    )
    .unwrap();
    assert!(s.execute_with_host(commands.clone(), &mut h).is_err());
    assert_eq!(s.snapshot(), &before);
    assert_eq!(h.anchors, before.anchors);
    h.reject = false;
    s.execute_with_host(commands, &mut h).unwrap();
    let id = s.snapshot().document.objects[0].id.clone();
    let move_c = transform_selection(
        s.snapshot(),
        &BTreeSet::from([id]),
        Point::default(),
        Point::new(10., 5.),
        1.,
        0.,
    )
    .unwrap();
    s.execute_with_host(move_c, &mut h).unwrap();
    assert_eq!(
        h.anchors[&AnchorId::from("b")]
            .position
            .as_ref()
            .unwrap()
            .point,
        Point::new(50., 65.)
    );
    s.undo(&mut h).unwrap();
    s.execute_with_host(
        vec![Command::MoveAnchor {
            id: "b".into(),
            point: None,
        }],
        &mut h,
    )
    .unwrap();
    assert!(h.anchors[&AnchorId::from("b")].position.is_none());
    s.undo(&mut h).unwrap();
    let mut layer = s.snapshot().document.layers[0].clone();
    layer.locked = true;
    s.execute(vec![Command::PutLayer { layer, index: 0 }])
        .unwrap();
    let stamp = s.stamp();
    assert!(s
        .execute_with_host(
            vec![Command::MoveAnchor {
                id: "b".into(),
                point: None
            }],
            &mut h
        )
        .is_err());
    assert_eq!(s.stamp(), stamp);
}
#[test]
fn scatter_is_seeded_event_resampling_independent_and_one_undo() {
    let mut s = open(base());
    let t = EditorTools {
        stamp: ResourceRef {
            key: "builtin:tree".into(),
            version: 1,
        },
        ..EditorTools::default()
    };
    let settings = ScatterSettings {
        density: 1.,
        spacing: 20.,
        ..ScatterSettings::default()
    };
    let coarse = [Point::new(0., 0.), Point::new(100., 0.)];
    let fine = (0..=10)
        .map(|i| Point::new(i as f64 * 10., 0.))
        .collect::<Vec<_>>();
    let c = scatter(s.snapshot(), 0, &t, &coarse, &settings, &s.limits()).unwrap();
    let d = scatter(s.snapshot(), 0, &t, &fine, &settings, &s.limits()).unwrap();
    let mut other = open(base());
    s.execute(c).unwrap();
    other.execute(d).unwrap();
    assert_eq!(s.snapshot(), other.snapshot());
    assert_eq!(s.snapshot().document.objects.len(), 6);
    assert_eq!(s.history().undo, 1);
    let saved = s.snapshot().clone();
    s.undo(&mut Standalone).unwrap();
    assert!(s.snapshot().document.objects.is_empty());
    s.redo(&mut Standalone).unwrap();
    assert_eq!(s.snapshot(), &saved);
    let raw = StoredMap::from_snapshot(s.snapshot())
        .encode(&s.limits())
        .unwrap();
    assert_eq!(
        StoredMap::decode(&raw, &s.limits()).unwrap().document,
        s.snapshot().document
    );
}
#[test]
fn scatter_invalid_and_capacity_rejections_never_commit_partial_planting() {
    let mut s = open(base());
    let mut settings = ScatterSettings {
        spacing: f64::NAN,
        ..ScatterSettings::default()
    };
    let route = [Point::default(), Point::new(300., 0.)];
    assert!(scatter(
        s.snapshot(),
        0,
        &EditorTools::default(),
        &route,
        &settings,
        &s.limits()
    )
    .is_err());
    settings.spacing = 1.;
    let limits = Limits {
        max_commands: 2,
        ..s.limits()
    };
    assert!(scatter(
        s.snapshot(),
        0,
        &EditorTools::default(),
        &route,
        &settings,
        &limits
    )
    .is_err());
    assert_eq!(s.stamp().revision, 0);
    let t = EditorTools {
        layer: "missing".into(),
        ..EditorTools::default()
    };
    let c = scatter(
        s.snapshot(),
        0,
        &t,
        &[Point::default()],
        &settings,
        &s.limits(),
    )
    .unwrap();
    assert!(s.execute(c).is_err());
    assert!(s.snapshot().document.objects.is_empty());
}
#[test]
fn presets_share_path_commands_editable_nodes_and_style_resolver() {
    let mut s = open(base());
    for preset in [
        MapPreset::Land,
        MapPreset::Water,
        MapPreset::Lake,
        MapPreset::Mountain,
        MapPreset::Forest,
        MapPreset::Boundary,
        MapPreset::River,
        MapPreset::Road,
        MapPreset::Trail,
    ] {
        let mut t = EditorTools::default();
        preset.apply(&mut t);
        let c = create_object(
            s.snapshot(),
            s.stamp().revision,
            &t,
            Geometry::Path {
                nodes: vec![
                    PathNode {
                        position: Point::default(),
                        incoming: None,
                        outgoing: None,
                    },
                    PathNode {
                        position: Point::new(80., 100.),
                        incoming: None,
                        outgoing: None,
                    },
                    PathNode {
                        position: Point::new(160., 0.),
                        incoming: None,
                        outgoing: None,
                    },
                ],
                closed: t.closed,
                smooth: t.smooth,
                transform: Transform::default(),
            },
        )
        .unwrap();
        s.execute(vec![c]).unwrap();
        assert_eq!(
            s.snapshot().document.objects.last().unwrap().paint.role,
            preset.role()
        );
    }
    let r = Registry::default();
    let before = s.snapshot().document.objects.clone();
    s.execute(vec![Command::SetStyle(r.styles()[1].resource.clone())])
        .unwrap();
    assert_eq!(s.snapshot().document.objects, before);
    let svg = export_svg(s.snapshot(), &r, &s.limits()).unwrap().svg;
    assert!(svg.contains("stroke-dasharray=\"8 6\""));
    assert!(svg.contains("#b5dce6"));
}
#[test]
fn roads_snap_at_creation_only_and_ignore_non_node_stamps() {
    let mut s = open(base());
    let p = Point::new(200., 200.);
    s.execute(create_node(s.snapshot(), 0, &EditorTools::default(), p, "城".into()).unwrap())
        .unwrap();
    assert_eq!(snap_endpoint(s.snapshot(), Point::new(205., 204.), 1.), p);
    assert_ne!(snap_endpoint(s.snapshot(), Point::new(205., 204.), 8.), p);
    let mut t = EditorTools::default();
    MapPreset::Road.apply(&mut t);
    let points = [
        snap_endpoint(s.snapshot(), Point::new(205., 204.), 1.),
        Point::new(400., 200.),
    ];
    let c = create_object(
        s.snapshot(),
        s.stamp().revision,
        &t,
        Geometry::Path {
            nodes: points
                .into_iter()
                .map(|position| PathNode {
                    position,
                    incoming: None,
                    outgoing: None,
                })
                .collect(),
            closed: false,
            smooth: true,
            transform: Transform::default(),
        },
    )
    .unwrap();
    s.execute(vec![c]).unwrap();
    let road = s.snapshot().document.objects[1].clone();
    s.execute(
        transform_selection(
            s.snapshot(),
            &BTreeSet::from([s.snapshot().document.objects[0].id.clone()]),
            p,
            Point::new(50., 0.),
            1.,
            0.,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(s.snapshot().document.objects[1], road);
}

#[test]
fn projection_lock_cannot_be_bypassed_by_materialization_and_hidden_hosts_do_not_snap() {
    let mut s = open(base());
    let mut h = Host {
        anchors: s.snapshot().anchors.clone(),
        reject: false,
    };
    let mut layer = s.snapshot().document.layers[0].clone();
    layer.locked = true;
    s.execute(vec![
        Command::PutLayer {
            layer: layer.clone(),
            index: 0,
        },
        Command::PutLayer {
            layer: Layer {
                id: "free".into(),
                name: "free".into(),
                visible: true,
                locked: false,
                extensions: Extensions::new(),
            },
            index: 1,
        },
    ])
    .unwrap();
    let t = EditorTools {
        layer: "free".into(),
        ..EditorTools::default()
    };
    let before = s.snapshot().clone();
    let commands = place_anchor(
        s.snapshot(),
        s.stamp().revision,
        &t,
        &"a".into(),
        Point::new(100., 100.),
    )
    .unwrap();
    assert!(s.execute_with_host(commands, &mut h).is_err());
    assert_eq!(s.snapshot(), &before);
    assert!(s
        .execute_with_host(
            vec![Command::MoveAnchor {
                id: "a".into(),
                point: None
            }],
            &mut h
        )
        .is_err());
    let commands = set_anchor_appearance(
        s.snapshot(),
        s.stamp().revision,
        &"a".into(),
        default_appearance(),
    )
    .unwrap();
    assert!(s.execute(commands).is_err());
    layer.visible = false;
    s.execute(vec![Command::PutLayer { layer, index: 0 }])
        .unwrap();
    let p = Point::new(12., 22.);
    assert_eq!(snap_endpoint(s.snapshot(), p, 1.), p);
    assert!(scene::content_bounds(s.snapshot(), &Registry::default()).is_none());
}

#[test]
fn sampled_routes_are_textured_regions_with_atomic_history_and_durable_export() {
    let registry = Registry::default();
    for preset in [
        MapPreset::River,
        MapPreset::Road,
        MapPreset::Trail,
        MapPreset::Land,
    ] {
        let mut session = open(MapSnapshot::standalone(MapDocument::new("brush-map")));
        let mut tools = EditorTools::default();
        preset.apply(&mut tools);
        assert_eq!(tools.tool, EditorTool::Brush);
        let samples = [
            Point::new(20., 20.),
            Point::new(50., 40.),
            Point::new(80., 35.),
            Point::new(100., 60.),
        ];
        let width = registry
            .paint(&session.snapshot().document.style, &tools.paint)
            .width;
        let commands = brush_commands(
            session.snapshot(),
            0,
            &tools,
            &samples,
            width,
            &session.limits(),
        )
        .unwrap();
        session.execute(commands).unwrap();
        assert_eq!(session.snapshot().document.objects.len(), 1);
        assert!(matches!(
            session.snapshot().document.objects[0].geometry,
            Geometry::Path { closed: true, .. }
        ));
        let saved = session.snapshot().clone();
        let svg = export_svg(&saved, &registry, &session.limits())
            .unwrap()
            .svg;
        if preset != MapPreset::Land {
            assert!(svg.contains("data-ribbon="));
            assert!(svg.contains("patternUnits="));
        }
        session.undo(&mut Standalone).unwrap();
        assert!(session.snapshot().document.objects.is_empty());
        session.redo(&mut Standalone).unwrap();
        assert_eq!(session.snapshot(), &saved);
        let decoded = StoredMap::decode(
            &StoredMap::from_snapshot(&saved)
                .encode(&session.limits())
                .unwrap(),
            &session.limits(),
        )
        .unwrap();
        assert_eq!(
            export_svg(&decoded.into_snapshot(), &registry, &session.limits())
                .unwrap()
                .svg,
            svg
        );
        // A click makes a valid finite dab, even for an area brush.
        assert!(
            brush_commands(&saved, 10, &tools, &samples[..1], width, &session.limits()).is_ok()
        );
        let before = session.snapshot().clone();
        assert!(brush_commands(
            &saved,
            11,
            &tools,
            &[Point::new(f64::NAN, 0.)],
            width,
            &session.limits()
        )
        .is_err());
        let limits = Limits {
            max_object_nodes: 3,
            ..session.limits()
        };
        assert!(brush_commands(&saved, 11, &tools, &samples, width, &limits).is_err());
        assert_eq!(session.snapshot(), &before);
    }
}

#[test]
fn resize_scatter_scales_symbol_geometry_and_spacing_without_mutating_old_objects() {
    let snapshot = MapSnapshot::standalone(MapDocument::new("resize"));
    let tools = EditorTools::default();
    let mut settings = ScatterSettings::default();
    let old = scatter(
        &snapshot,
        0,
        &tools,
        &[Point::new(20., 20.)],
        &settings,
        &Limits::default(),
    )
    .unwrap();
    let previous = settings.clone();
    settings.resize(48.).unwrap();
    assert!(settings.size[0] > previous.size[0]);
    assert!(settings.size[1] > previous.size[1]);
    assert!(settings.spacing > previous.spacing);
    let new = scatter(
        &snapshot,
        0,
        &tools,
        &[Point::new(20., 20.)],
        &settings,
        &Limits::default(),
    )
    .unwrap();
    let scale = |commands: &[Command]| match &commands[0] {
        Command::PutObject { object, .. } => match object.geometry {
            Geometry::Stamp { transform, .. } => transform.scale,
            _ => 0.,
        },
        _ => 0.,
    };
    assert!(scale(&new) > scale(&old));
    let unchanged = settings.clone();
    assert!(settings.resize(f64::NAN).is_err());
    assert_eq!(settings, unchanged);
}

#[test]
fn registered_entities_share_scatter_and_atomic_symbol_replacement() {
    let mut registry = Registry::default();
    let reference = ResourceRef {
        key: "example:new-tree".into(),
        version: 1,
    };
    let mut definition = registry
        .stamp(&ResourceRef {
            key: "builtin:pine".into(),
            version: 1,
        })
        .unwrap()
        .clone();
    definition.resource = reference.clone();
    registry.register_stamp(definition).unwrap();
    let mut session = MapSession::open(
        MapSnapshot::standalone(MapDocument::new("entities")),
        None,
        Limits::default(),
        HistoryBudget::default(),
    )
    .unwrap();
    session.set_registry(registry.clone()).unwrap();
    let mut tools = EditorTools::default();
    dioxus_map_editor::map::apply_symbol(
        &mut tools,
        &registry,
        reference.clone(),
        "vegetation".into(),
    )
    .unwrap();
    let commands = dioxus_map_editor::map::scatter(
        session.snapshot(),
        0,
        &tools,
        &[Point::new(10., 20.), Point::new(100., 20.)],
        &tools.scatter,
        &session.limits(),
    )
    .unwrap();
    session.execute(commands).unwrap();
    let before = session.snapshot().clone();
    let ids = before
        .document
        .objects
        .iter()
        .map(|o| o.id.clone())
        .collect();
    let replacement = ResourceRef {
        key: "builtin:ridge".into(),
        version: 1,
    };
    let commands = dioxus_map_editor::map::replace_symbols(
        session.snapshot(),
        &ids,
        replacement.clone(),
        &registry,
    )
    .unwrap();
    session.execute(commands).unwrap();
    for (old, new) in before
        .document
        .objects
        .iter()
        .zip(&session.snapshot().document.objects)
    {
        assert_eq!(
            scene::transform(&old.geometry),
            scene::transform(&new.geometry)
        );
        assert_eq!(old.extensions, new.extensions);
        assert!(matches!(&new.geometry,Geometry::Stamp{stamp,..}if *stamp==replacement));
    }
    session.undo(&mut Standalone).unwrap();
    assert_eq!(session.snapshot(), &before);
    let mut locked = before;
    locked.document.layers[0].locked = true;
    assert!(
        dioxus_map_editor::map::replace_symbols(&locked, &ids, replacement, &registry).is_err()
    );
    let old = tools.clone();
    assert!(dioxus_map_editor::map::apply_symbol(
        &mut tools,
        &registry,
        ResourceRef {
            key: "missing:x".into(),
            version: 1
        },
        "forest".into()
    )
    .is_err());
    assert_eq!(tools, old);
}
