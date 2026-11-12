#![allow(clippy::unwrap_used)]
//! Public API integration coverage; official providers use no private core hooks.
#[cfg(any(feature = "plugin-brushes", feature = "plugin-objects"))]
use dioxus_map_editor::geometry::*;
#[cfg(any(
    feature = "plugin-brushes",
    feature = "plugin-objects",
    feature = "plugin-formats"
))]
use dioxus_map_editor::{command::Command, storage::*};
use dioxus_map_editor::{
    document::*,
    editor::EditorTools,
    error::{MapError, MapResult},
    history::HistoryBudget,
    plugin::*,
    registry::Registry,
    session::{MapSession, Standalone},
};
use std::{cell::RefCell, rc::Rc};
fn session() -> MapSession {
    MapSession::open(
        MapSnapshot::standalone(MapDocument::new("plugins")),
        None,
        Limits::default(),
        HistoryBudget::default(),
    )
    .unwrap()
}
#[derive(Debug)]
struct TestPlugin {
    key: &'static str,
    dep: Vec<Dependency>,
    log: Rc<RefCell<Vec<String>>>,
    conflict: bool,
}
impl Plugin for TestPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: PluginId(self.key.into()),
            name: self.key.into(),
            version: 1,
            api_version: PLUGIN_API_VERSION,
            dependencies: self.dep.clone(),
        }
    }
    fn register(&self, r: &mut Registry) -> MapResult<()> {
        r.contribute(Contribution {
            key: format!("{}-menu", self.key),
            label: "Example".into(),
            slot: Slot::Menu,
            shortcut: Some(if self.conflict { "shift+f" } else { "shift+x" }.into()),
        })
    }
    fn attach(&self, s: &dioxus_map_editor::session::SessionStamp) {
        self.log
            .borrow_mut()
            .push(format!("attach:{}:{}", self.key, s.token));
    }
    fn detach(&self, s: &dioxus_map_editor::session::SessionStamp) {
        self.log
            .borrow_mut()
            .push(format!("detach:{}:{}", self.key, s.token));
    }
}
#[test]
fn lifecycle_dependencies_conflicts_and_async_tickets_are_atomic() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut host = PluginHost::default();
    let mut s = session();
    let mut t = EditorTools::default();
    let id = PluginId("test:one".into());
    host.register(TestPlugin {
        key: "test:one",
        dep: vec![],
        log: log.clone(),
        conflict: false,
    })
    .unwrap();
    assert!(matches!(
        host.register(TestPlugin {
            key: "test:one",
            dep: vec![],
            log: log.clone(),
            conflict: false
        }),
        Err(MapError::Duplicate(_))
    ));
    host.set_enabled(&id, true, &mut s, &mut t).unwrap();
    assert_eq!(log.borrow().len(), 1);
    let ticket = host.ticket(&id, &s).unwrap();
    let gesture = s.begin_gesture().unwrap();
    host.set_enabled(&id, false, &mut s, &mut t).unwrap();
    assert!(!s.is_gesture_current(&gesture));
    assert_eq!(s.stamp().revision, 0);
    assert!(matches!(
        host.commit(&ticket, &mut s, vec![], &mut Standalone),
        Err(MapError::Stale)
    ));
    host.set_enabled(&id, true, &mut s, &mut t).unwrap();
    assert!(host
        .commit(&ticket, &mut s, vec![], &mut Standalone)
        .is_err());
    let dep = PluginId("test:dep".into());
    host.register(TestPlugin {
        key: "test:dep",
        dep: vec![Dependency {
            id: id.clone(),
            version: 1,
        }],
        log: log.clone(),
        conflict: false,
    })
    .unwrap();
    // Same canonical shortcut: failed enable changes neither lifecycle nor catalogue.
    let before = host.registry().clone();
    let count = log.borrow().len();
    assert!(host.set_enabled(&dep, true, &mut s, &mut t).is_err());
    assert_eq!(before, *host.registry());
    assert_eq!(count, log.borrow().len());
    let old = host.ticket(&id, &s).unwrap();
    s.switch(MapSnapshot::standalone(MapDocument::new("next")), None)
        .unwrap();
    host.sync(&s);
    assert!(host.commit(&old, &mut s, vec![], &mut Standalone).is_err());
    let count = log.borrow().len();
    drop(host);
    assert_eq!(log.borrow().len(), count + 1);
}
#[test]
fn missing_dependency_and_wrong_api_leave_provider_disabled() {
    let mut host = PluginHost::default();
    let mut s = session();
    let mut t = EditorTools::default();
    host.register(TestPlugin {
        key: "test:dep",
        dep: vec![Dependency {
            id: PluginId("missing:one".into()),
            version: 1,
        }],
        log: Rc::default(),
        conflict: false,
    })
    .unwrap();
    assert!(host
        .set_enabled(&PluginId("test:dep".into()), true, &mut s, &mut t)
        .is_err());
    assert!(!host.plugins()[0].1);
    #[derive(Debug)]
    struct Future;
    impl Plugin for Future {
        fn info(&self) -> PluginInfo {
            PluginInfo {
                id: PluginId("test:future".into()),
                name: "Future".into(),
                version: 1,
                api_version: 2,
                dependencies: vec![],
            }
        }
        fn register(&self, _: &mut Registry) -> MapResult<()> {
            Ok(())
        }
    }
    assert!(host.register(Future).is_err());
}
#[cfg(feature = "plugin-objects")]
mod objects {
    use super::*;
    use dioxus_map_editor::{editor, export::export_svg, plugins::objects::*, scene};
    fn enabled() -> (PluginHost, MapSession, EditorTools) {
        let mut h = PluginHost::default();
        h.register(ObjectsPlugin).unwrap();
        let mut s = session();
        let mut t = EditorTools::default();
        h.set_enabled(&PluginId("official:objects".into()), true, &mut s, &mut t)
            .unwrap();
        (h, s, t)
    }
    #[test]
    fn custom_edit_transform_hit_save_export_and_missing_restore() {
        let (mut h, mut s, mut t) = enabled();
        let original = compass_object(
            "compass".into(),
            "base".into(),
            Point::new(60., 80.),
            Compass::default(),
        )
        .unwrap();
        s.execute(vec![Command::PutObject {
            object: original,
            index: 0,
        }])
        .unwrap();
        assert_eq!(
            scene::hit_test(s.snapshot(), h.registry(), Point::new(60., 80.), 1.),
            Some("compass".into())
        );
        assert_eq!(
            scene::hit_test(s.snapshot(), h.registry(), Point::new(98., 118.), 10.),
            None
        );
        let selected = ["compass".into()].into();
        let commands = editor::transform_selection(
            s.snapshot(),
            &selected,
            Point::new(60., 80.),
            Point::new(10., 0.),
            2.,
            0.4,
        )
        .unwrap();
        s.execute(commands).unwrap();
        let o = &s.snapshot().document.objects[0];
        let edited = edit_compass(
            o,
            Compass {
                radius: 55.,
                inset: 0.4,
            },
        )
        .unwrap();
        s.execute(vec![Command::PutObject {
            object: edited,
            index: 0,
        }])
        .unwrap();
        let saved = StoredMap::from_snapshot(s.snapshot());
        let decoded = StoredMap::decode(&saved.encode(&s.limits()).unwrap(), &s.limits()).unwrap();
        assert_eq!(saved, decoded);
        let exported = export_svg(s.snapshot(), h.registry(), &s.limits()).unwrap();
        assert!(exported.svg.contains("data-custom"));
        assert!(exported.warnings.is_empty());
        let before = s.snapshot().clone();
        h.set_enabled(&PluginId("official:objects".into()), false, &mut s, &mut t)
            .unwrap();
        assert_eq!(before, *s.snapshot());
        let fallback = export_svg(s.snapshot(), h.registry(), &s.limits()).unwrap();
        assert!(fallback.svg.contains("data-missing-object"));
        assert_eq!(fallback.warnings.len(), 1);
        assert!(s
            .execute(
                editor::transform_selection(
                    s.snapshot(),
                    &selected,
                    Point::default(),
                    Point::new(1., 0.),
                    1.,
                    0.
                )
                .unwrap()
            )
            .is_err());
        h.set_enabled(&PluginId("official:objects".into()), true, &mut s, &mut t)
            .unwrap();
        assert_eq!(
            exported.svg,
            export_svg(s.snapshot(), h.registry(), &s.limits())
                .unwrap()
                .svg
        );
        s.undo(&mut Standalone).unwrap();
        s.redo(&mut Standalone).unwrap();
        assert_eq!(before, *s.snapshot());
        // Reopening with missing plugin keeps bytes until activation.
        let mut reopened = MapSession::open(
            decoded.into_snapshot(),
            None,
            s.limits(),
            HistoryBudget::default(),
        )
        .unwrap();
        let mut fresh = PluginHost::default();
        fresh.register(ObjectsPlugin).unwrap();
        fresh
            .set_enabled(
                &PluginId("official:objects".into()),
                true,
                &mut reopened,
                &mut t,
            )
            .unwrap();
        assert_eq!(*reopened.snapshot(), before);
    }
    #[test]
    fn invalid_payload_bounds_prepared_edit_and_budget_cannot_bypass_session() {
        let (mut h, mut s, mut t) = enabled();
        let o = compass_object(
            "c".into(),
            "base".into(),
            Point::default(),
            Compass::default(),
        )
        .unwrap();
        let edit = s
            .prepare(
                &s.stamp(),
                vec![Command::PutObject {
                    object: o.clone(),
                    index: 0,
                }],
            )
            .unwrap();
        h.set_enabled(&PluginId("official:objects".into()), false, &mut s, &mut t)
            .unwrap();
        assert!(matches!(
            s.accept(edit, &mut Standalone),
            Err(MapError::Stale)
        ));
        h.set_enabled(&PluginId("official:objects".into()), true, &mut s, &mut t)
            .unwrap();
        let before = s.snapshot().clone();
        let mut invalid = o.clone();
        if let Geometry::Custom { payload, .. } = &mut invalid.geometry {
            **payload = serde_json::json!({"radius":-2,"inset":0.3});
        }
        assert!(s
            .execute(vec![Command::PutObject {
                object: invalid,
                index: 0
            }])
            .is_err());
        assert_eq!(before, *s.snapshot());
        assert_eq!(s.history().undo, 0);
        let mut invalid = o.clone();
        if let Geometry::Custom { bounds, .. } = &mut invalid.geometry {
            bounds.width += 1.;
        }
        assert!(s
            .execute(vec![Command::PutObject {
                object: invalid,
                index: 0
            }])
            .is_err());
        let mut future = o;
        if let Geometry::Custom { object_type, .. } = &mut future.geometry {
            object_type.version = 2;
        }
        let mut d = MapDocument::new("future");
        d.objects.push(future);
        let raw = encode_document(&d, &s.limits()).unwrap();
        assert!(matches!(
            decode_document(&raw, &s.limits()).unwrap(),
            LoadedDocument::Editable(_)
        ));
        assert!(
            export_svg(&MapSnapshot::standalone(d), h.registry(), &s.limits())
                .unwrap()
                .warnings
                .len()
                == 1
        );
    }
}
#[cfg(feature = "plugin-brushes")]
#[test]
fn custom_brush_preview_cancel_commit_history_and_reload() {
    use dioxus_map_editor::plugins::brushes::*;
    let mut h = PluginHost::default();
    h.register(BrushesPlugin).unwrap();
    let mut s = session();
    let mut t = EditorTools::default();
    h.set_enabled(&PluginId("official:brushes".into()), true, &mut s, &mut t)
        .unwrap();
    let points = [Point::new(10., 10.), Point::new(200., 10.)];
    let brush = h.registry().brush(&resource("official:foliage")).unwrap();
    let generate = || {
        brush
            .commands(BrushInput {
                snapshot: s.snapshot(),
                revision: 0,
                tools: &t,
                points: &points,
                limits: &s.limits(),
            })
            .unwrap()
    };
    let preview = generate();
    assert_eq!(format!("{:?}", preview), format!("{:?}", generate()));
    assert_eq!(s.snapshot().document.objects.len(), 0);
    let ticket = s.begin_gesture().unwrap();
    s.update_gesture(&ticket, preview.clone()).unwrap();
    s.cancel_gesture(&ticket).unwrap();
    assert_eq!(s.history().undo, 0);
    let ticket = s.begin_gesture().unwrap();
    s.update_gesture(&ticket, preview).unwrap();
    s.commit_gesture(&ticket, &mut Standalone).unwrap();
    let saved = s.snapshot().clone();
    assert!(saved.document.objects.len() > 2);
    assert_eq!(s.history().undo, 1);
    s.undo(&mut Standalone).unwrap();
    assert!(s.snapshot().document.objects.is_empty());
    s.redo(&mut Standalone).unwrap();
    assert_eq!(saved, *s.snapshot());
    let raw = StoredMap::from_snapshot(s.snapshot())
        .encode(&s.limits())
        .unwrap();
    assert_eq!(
        StoredMap::decode(&raw, &s.limits()).unwrap().document,
        saved.document
    );
    let tiny = Limits {
        max_objects: 1,
        ..Limits::default()
    };
    assert!(brush
        .commands(BrushInput {
            snapshot: &MapSnapshot::standalone(MapDocument::new("tiny")),
            revision: 0,
            tools: &t,
            points: &points,
            limits: &tiny
        })
        .is_err());
}
#[cfg(feature = "plugin-formats")]
mod formats {
    use super::*;
    use dioxus_map_editor::plugins::formats::*;
    #[test]
    fn import_validate_review_dirty_stale_cancel_undo_and_future_protection() {
        let mut h = PluginHost::default();
        h.register(FormatsPlugin).unwrap();
        let mut s = session();
        let mut t = EditorTools::default();
        h.set_enabled(&PluginId("official:formats".into()), true, &mut s, &mut t)
            .unwrap();
        let mut document = MapDocument::new("source");
        document
            .extensions
            .insert("host:opaque".into(), serde_json::json!({"v":1}));
        let raw = encode_document(&document, &s.limits()).unwrap();
        let original = s.snapshot().clone();
        assert!(ImportCandidate::parse("{bad", &s, h.registry()).is_err());
        assert_eq!(original, *s.snapshot());
        let cancel = ImportCandidate::parse(&raw, &s, h.registry()).unwrap();
        drop(cancel);
        assert_eq!(s.history().undo, 0);
        let candidate = ImportCandidate::parse(&raw, &s, h.registry()).unwrap();
        assert_eq!(candidate.document().id, document.id);
        candidate.apply(&mut s).unwrap();
        assert_eq!(s.snapshot().document.id, original.document.id);
        assert!(s.is_dirty());
        assert_eq!(s.history().undo, 1);
        assert!(matches!(
            ImportCandidate::parse(&raw, &s, h.registry())
                .unwrap()
                .apply(&mut s),
            Err(MapError::Unsaved)
        ));
        s.undo(&mut Standalone).unwrap();
        assert_eq!(original, *s.snapshot());
        s.redo(&mut Standalone).unwrap();
        assert!(s.is_dirty());
        let stale = ImportCandidate::parse(&raw, &s, h.registry()).unwrap();
        s.undo(&mut Standalone).unwrap();
        assert!(matches!(stale.apply(&mut s), Err(MapError::Stale)));
        let mut future_value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        future_value["schema_version"] = serde_json::json!(99);
        let future = serde_json::to_string(&future_value).unwrap();
        assert!(ImportCandidate::parse(&future, &s, h.registry()).is_err());
        assert!(Svg.decode(&raw, &s.limits()).is_err());
        assert!(NativeJson
            .decode(
                &raw,
                &Limits {
                    max_bytes: 2,
                    ..s.limits()
                }
            )
            .is_err());
        assert_eq!(h.registry().formats().len(), 2);
        assert_eq!(
            NativeJson
                .encode(&s.snapshot().clone(), h.registry(), &s.limits())
                .unwrap()
                .text,
            encode_document(&s.snapshot().document, &s.limits()).unwrap()
        );
    }
}
#[cfg(feature = "official-plugins")]
#[test]
fn optional_providers_combine_and_isolate_two_canvases() {
    use dioxus_map_editor::plugins::{
        brushes::BrushesPlugin, formats::FormatsPlugin, map::MapPackPlugin, objects::ObjectsPlugin,
    };
    let mut h = PluginHost::default();
    h.register(BrushesPlugin).unwrap();
    h.register(ObjectsPlugin).unwrap();
    h.register(FormatsPlugin).unwrap();
    h.register(MapPackPlugin).unwrap();
    let mut s = session();
    let mut t = EditorTools::default();
    for key in [
        "official:brushes",
        "official:objects",
        "official:formats",
        "official:map",
    ] {
        h.set_enabled(&PluginId(key.into()), true, &mut s, &mut t)
            .unwrap();
    }
    let other = PluginHost::default();
    assert!(other.registry().brushes().is_empty());
    assert!(h.registry().has_style(&resource("official:atlas")));
    assert!(h.registry().action("official:add-compass").is_some());
    assert!(h
        .registry()
        .contributions()
        .iter()
        .any(|c| c.slot == Slot::Overlay));
    let ticket = h.ticket(&PluginId("official:objects".into()), &s).unwrap();
    let commands = h
        .registry()
        .action("official:add-compass")
        .unwrap()
        .commands(s.snapshot(), &serde_json::json!({"x":50,"y":50}))
        .unwrap();
    h.commit(&ticket, &mut s, commands, &mut Standalone)
        .unwrap();
    assert_eq!(s.history().undo, 1);
}

#[cfg(feature = "plugin-objects")]
#[test]
fn generated_custom_geometry_obeys_session_budgets_and_rejected_enable_is_atomic() {
    use dioxus_map_editor::plugins::objects::*;
    let mut host = PluginHost::default();
    host.register(ObjectsPlugin).unwrap();
    let mut session = MapSession::open(
        MapSnapshot::standalone(MapDocument::new("tiny")),
        None,
        Limits {
            max_object_nodes: 8,
            ..Limits::default()
        },
        HistoryBudget::default(),
    )
    .unwrap();
    let mut tools = EditorTools::default();
    host.set_enabled(
        &PluginId("official:objects".into()),
        true,
        &mut session,
        &mut tools,
    )
    .unwrap();
    let object = compass_object(
        "c".into(),
        "base".into(),
        Point::default(),
        Compass::default(),
    )
    .unwrap();
    assert!(matches!(
        session.execute(vec![Command::PutObject {
            object: object.clone(),
            index: 0
        }]),
        Err(MapError::Budget(_))
    ));
    assert_eq!(session.history().undo, 0);
    let mut doc = MapDocument::new("invalid");
    let mut malformed = object;
    if let Geometry::Custom { payload, .. } = &mut malformed.geometry {
        **payload = serde_json::json!({"radius":-1.,"inset":0.3});
    }
    doc.objects.push(malformed);
    let mut unknown = MapSession::open(
        MapSnapshot::standalone(doc),
        None,
        Limits::default(),
        HistoryBudget::default(),
    )
    .unwrap();
    let mut fresh = PluginHost::default();
    fresh.register(ObjectsPlugin).unwrap();
    let before = unknown.snapshot().clone();
    assert!(fresh
        .set_enabled(
            &PluginId("official:objects".into()),
            true,
            &mut unknown,
            &mut tools
        )
        .is_err());
    assert!(!fresh.plugins()[0].1);
    assert_eq!(before, *unknown.snapshot());
}

#[test]
fn release_invalidates_tasks_even_after_reattaching_same_session() {
    let mut host = PluginHost::default();
    let mut session = session();
    let mut tools = EditorTools::default();
    let id = PluginId("test:release".into());
    host.register(TestPlugin {
        key: "test:release",
        dep: vec![],
        log: Rc::default(),
        conflict: false,
    })
    .unwrap();
    host.set_enabled(&id, true, &mut session, &mut tools)
        .unwrap();
    let ticket = host.ticket(&id, &session).unwrap();
    host.release();
    assert!(host.ticket(&id, &session).is_err());
    host.sync(&session);
    assert!(matches!(
        host.commit(&ticket, &mut session, vec![], &mut Standalone),
        Err(MapError::Stale)
    ));
}

#[test]
fn dependency_versions_cycles_and_disabling_required_provider_are_checked() {
    #[derive(Debug)]
    struct Provider(&'static str, Vec<Dependency>);
    impl Plugin for Provider {
        fn info(&self) -> PluginInfo {
            PluginInfo {
                id: PluginId(self.0.into()),
                name: self.0.into(),
                version: 1,
                api_version: 1,
                dependencies: self.1.clone(),
            }
        }
        fn register(&self, _: &mut Registry) -> MapResult<()> {
            Ok(())
        }
    }
    let mut host = PluginHost::default();
    let mut s = session();
    let mut t = EditorTools::default();
    let base = PluginId("z:base".into());
    let dependent = PluginId("a:dependent".into());
    host.register(Provider("z:base", vec![])).unwrap();
    host.register(Provider(
        "a:dependent",
        vec![Dependency {
            id: base.clone(),
            version: 1,
        }],
    ))
    .unwrap();
    host.set_enabled(&base, true, &mut s, &mut t).unwrap();
    host.set_enabled(&dependent, true, &mut s, &mut t).unwrap();
    assert!(host.set_enabled(&base, false, &mut s, &mut t).is_err());
    host.register(Provider(
        "a:wrong",
        vec![Dependency {
            id: base.clone(),
            version: 2,
        }],
    ))
    .unwrap();
    assert!(host
        .set_enabled(&PluginId("a:wrong".into()), true, &mut s, &mut t)
        .is_err());
    host.register(Provider(
        "cycle:a",
        vec![Dependency {
            id: PluginId("cycle:a".into()),
            version: 1,
        }],
    ))
    .unwrap();
    assert!(host
        .set_enabled(&PluginId("cycle:a".into()), true, &mut s, &mut t)
        .is_err());
    assert!(host.registry().contributions().is_empty());
}
