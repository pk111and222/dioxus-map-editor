//! Content/session invariants including atomic failures and async interleavings.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use dioxus_map_editor::{
    command::*, document::*, error::*, geometry::*, history::*, session::*, storage::*,
};
use serde_json::json;
use std::collections::BTreeMap;

fn empty() -> MapSession {
    MapSession::open(
        MapSnapshot::standalone(MapDocument::new("map-a")),
        None,
        Limits::default(),
        HistoryBudget::default(),
    )
    .unwrap()
}
fn object(id: &str) -> MapObject {
    MapObject {
        id: id.into(),
        layer_id: "base".into(),
        geometry: Geometry::Stroke {
            points: vec![Point::new(1.0, 2.0), Point::new(3.0, 4.0)],
            transform: Transform::default(),
        },
        paint: Paint::default(),
        extensions: Extensions::new(),
    }
}
fn put(id: &str) -> Command {
    Command::PutObject {
        object: object(id),
        index: 0,
    }
}
fn add(session: &mut MapSession, id: &str) {
    session.execute(vec![put(id)]).unwrap();
}
fn resource() -> ResourceRef {
    ResourceRef {
        key: "other:paper".into(),
        version: 2,
    }
}
fn editable(raw: &str) -> MapDocument {
    let LoadedDocument::Editable(doc) = decode_document(raw, &Limits::default()).unwrap() else {
        panic!("protected unexpectedly")
    };
    doc
}
fn bound_snapshot() -> MapSnapshot {
    let document = MapDocument::new("map-a");
    let id = AnchorId::from("place");
    MapSnapshot {
        document,
        anchors: BTreeMap::from([(
            id.clone(),
            ExternalAnchor {
                id,
                name: "旧名字".into(),
                parent_id: Some("map-a".into()),
                position: Some(AnchorPosition {
                    point: Point::new(10.0, 20.0),
                    z: Some(99.0),
                    canvas: "map-a".into(),
                }),
                metadata: BTreeMap::from([("host:extra".into(), json!({"kept":true}))]),
            },
        )]),
    }
}
struct Host {
    anchors: BTreeMap<AnchorId, ExternalAnchor>,
    fail: bool,
}
impl AnchorTransaction for Host {
    fn accept(&mut self, patches: &[AnchorPatch]) -> MapResult<()> {
        if self.fail {
            return Err(MapError::HostRejected("test".into()));
        }
        let mut next = self.anchors.clone();
        for p in patches {
            let anchor = next
                .get_mut(&p.id)
                .ok_or_else(|| MapError::Missing(p.id.to_string()))?;
            if anchor.parent_id != p.parent_id || anchor.position != p.before {
                return Err(MapError::Stale);
            }
            anchor.position = p.after.clone();
        }
        self.anchors = next;
        Ok(())
    }
}

#[test]
fn document_roundtrip_preserves_unknown_extensions_and_resource_refs() {
    let mut doc = MapDocument::new("map-a");
    doc.extensions.insert(
        "future:payload".into(),
        json!({"version":31,"nested":[1,"literal < & text",null]}),
    );
    let mut obj = object("stroke");
    obj.extensions
        .insert("plugin:unknown".into(), json!([4, 5]));
    doc.objects.push(obj);
    doc.style = resource();
    let raw = encode_document(&doc, &Limits::default()).unwrap();
    assert_eq!(editable(&raw), doc);
    assert!(!raw.contains("viewport"));
    assert!(!raw.contains("history"));
}
#[test]
fn all_geometry_variants_validate_and_roundtrip() {
    let mut doc = MapDocument::new("map-a");
    let geometries = vec![
        Geometry::Path {
            nodes: vec![
                PathNode {
                    position: Point::new(0.0, 0.0),
                    incoming: None,
                    outgoing: Some(Point::new(2.0, 3.0)),
                },
                PathNode {
                    position: Point::new(4.0, 5.0),
                    incoming: None,
                    outgoing: None,
                },
            ],
            closed: false,
            smooth: true,
            transform: Transform::default(),
        },
        Geometry::Text {
            text: "地图 < & \"".into(),
            transform: Transform::default(),
        },
        Geometry::Stamp {
            stamp: resource(),
            transform: Transform::default(),
        },
        Geometry::BoundMarker {
            anchor_id: "absent".into(),
            appearance: MarkerAppearance {
                icon: resource(),
                scale: 1.0,
                label_offset: Point::default(),
            },
        },
    ];
    for (i, g) in geometries.into_iter().enumerate() {
        let mut obj = object(&format!("o-{i}"));
        obj.geometry = g;
        doc.objects.push(obj);
    }
    assert_eq!(
        editable(&encode_document(&doc, &Limits::default()).unwrap()),
        doc
    );
}
#[test]
fn schema_protection_retains_exact_original_and_blocks_writes() {
    let raw = " { \"schema_version\": 999, \"id\":\"map-a\", \"new\": [1,2] } ";
    let LoadedDocument::Protected(protected) = decode_document(raw, &Limits::default()).unwrap()
    else {
        panic!()
    };
    assert_eq!(protected.raw, raw);
    assert_eq!(protected.reason, ProtectionReason::FutureSchema(999));
    let mut s =
        MapSession::open_protected(protected, Limits::default(), HistoryBudget::default()).unwrap();
    assert_eq!(s.execute(vec![put("x")]), Err(MapError::ReadOnly));
    assert_eq!(s.begin_save().unwrap_err(), MapError::ReadOnly);
    assert!(s.set_read_only(false).is_err());
    assert_eq!(s.protected_document().unwrap().raw, raw);
}
#[test]
fn unknown_structure_and_legacy_without_migration_are_preserved() {
    let mut value = serde_json::to_value(MapDocument::new("map-a")).unwrap();
    value["unknown_top_level"] = json!({"new":true});
    assert!(matches!(
        decode_document(&value.to_string(), &Limits::default()).unwrap(),
        LoadedDocument::Protected(_)
    ));
    value.as_object_mut().unwrap().remove("unknown_top_level");
    value["schema_version"] = json!(0);
    assert!(matches!(
        decode_document(&value.to_string(), &Limits::default()).unwrap(),
        LoadedDocument::Protected(ProtectedDocument {
            reason: ProtectionReason::MissingMigration(0),
            ..
        })
    ));
}
struct V0;
impl Migration for V0 {
    fn source_version(&self) -> u32 {
        0
    }
    fn migrate(&self, mut value: serde_json::Value) -> MapResult<serde_json::Value> {
        value["schema_version"] = json!(1);
        Ok(value)
    }
}
#[test]
fn migration_boundary_validates_final_content_and_preserves_extensions() {
    let mut value = serde_json::to_value(MapDocument::new("map-a")).unwrap();
    value["schema_version"] = json!(0);
    value["extensions"] = json!({"x:data":{"v":8}});
    let LoadedDocument::Editable(doc) =
        decode_with_migrations(&value.to_string(), &Limits::default(), &[&V0]).unwrap()
    else {
        panic!()
    };
    assert_eq!(doc.schema_version, 1);
    assert_eq!(doc.extensions["x:data"], json!({"v":8}));
}
#[test]
fn malformed_json_numbers_and_budgets_are_rejected_before_mutation() {
    assert!(decode_document("{", &Limits::default()).is_err());
    let limits = Limits {
        max_bytes: 2,
        ..Limits::default()
    };
    assert!(matches!(
        decode_document("[1,2,3]", &limits),
        Err(MapError::Budget(_))
    ));
    let mut s = empty();
    let before = s.snapshot().clone();
    let mut bad = object("bad");
    if let Geometry::Stroke { points, .. } = &mut bad.geometry {
        points[0].x = f64::NAN;
    }
    assert!(s
        .execute(vec![
            put("good"),
            Command::PutObject {
                object: bad,
                index: 0
            }
        ])
        .is_err());
    assert_eq!(s.snapshot(), &before);
    assert_eq!(s.stamp().revision, 0);
    assert_eq!(s.history().undo, 0);
}
#[test]
fn common_validation_rejects_ids_references_styles_transforms_and_duplicate_bindings() {
    let base = MapDocument::new("map-a");
    for mutate in [0, 1, 2, 3, 4, 5] {
        let mut d = base.clone();
        let mut o = object("a");
        match mutate {
            0 => o.id = "".into(),
            1 => o.layer_id = "missing".into(),
            2 => o.paint.opacity = Some(2.0),
            3 => {
                if let Geometry::Stroke { transform, .. } = &mut o.geometry {
                    transform.scale = -1.0;
                }
            }
            4 => {
                o.extensions.insert("no-namespace".into(), json!(1));
            }
            _ => o.paint.color = Some("url(https://bad)".into()),
        }
        d.objects.push(o);
        assert!(d.validate(&Limits::default()).is_err());
    }
    let mut d = base;
    d.objects = vec![object("same"), object("same")];
    assert!(d.validate(&Limits::default()).is_err());
    let mut o = object("bound-1");
    o.geometry = Geometry::BoundMarker {
        anchor_id: "place".into(),
        appearance: MarkerAppearance {
            icon: resource(),
            scale: 1.0,
            label_offset: Point::default(),
        },
    };
    let mut o2 = o.clone();
    o2.id = "bound-2".into();
    d.objects = vec![o, o2];
    assert!(d.validate(&Limits::default()).is_err());
}
#[test]
fn batches_and_undo_redo_are_atomic_with_monotonic_revisions() {
    let mut s = empty();
    let original = s.snapshot().clone();
    assert_eq!(
        s.execute(vec![put("a"), put("b"), Command::SetStyle(resource())])
            .unwrap(),
        CommitOutcome::Committed(1)
    );
    let changed = s.snapshot().clone();
    assert_eq!(s.history().undo, 1);
    assert_eq!(
        s.undo(&mut Standalone).unwrap(),
        CommitOutcome::Committed(2)
    );
    assert_eq!(s.snapshot(), &original);
    assert_eq!(
        s.redo(&mut Standalone).unwrap(),
        CommitOutcome::Committed(3)
    );
    assert_eq!(s.snapshot(), &changed);
}
#[test]
fn net_noop_does_not_increment_revision_or_clear_redo_or_schedule_io() {
    let mut s = empty();
    add(&mut s, "a");
    s.undo(&mut Standalone).unwrap();
    let stamp = s.stamp();
    assert_eq!(
        s.execute(vec![put("x"), Command::RemoveObject("x".into())])
            .unwrap(),
        CommitOutcome::NoOp
    );
    assert_eq!(s.stamp(), stamp);
    assert_eq!(s.history().redo, 1);
    assert!(s.begin_save().unwrap().is_none());
}
#[test]
fn layer_delete_restores_order_and_references_in_one_undo() {
    let mut s = empty();
    s.execute(vec![
        put("a"),
        put("b"),
        Command::PutLayer {
            layer: Layer::new("top", "Top"),
            index: 1,
        },
    ])
    .unwrap();
    let before = s.snapshot().clone();
    assert!(s
        .execute(vec![Command::RemoveLayer {
            id: "base".into(),
            delete_objects: false
        }])
        .is_err());
    s.execute(vec![Command::RemoveLayer {
        id: "base".into(),
        delete_objects: true,
    }])
    .unwrap();
    assert!(s.snapshot().document.objects.is_empty());
    s.undo(&mut Standalone).unwrap();
    assert_eq!(s.snapshot(), &before);
}
#[test]
fn layer_lock_blocks_object_and_bound_anchor_edits() {
    let mut s = empty();
    let mut l = Layer::new("base", "Locked");
    l.locked = true;
    s.execute(vec![Command::PutLayer { layer: l, index: 0 }])
        .unwrap();
    assert_eq!(s.execute(vec![put("a")]), Err(MapError::ReadOnly));
}
#[test]
fn history_count_and_byte_budgets_evict_old_entries() {
    let mut s = MapSession::open(
        MapSnapshot::standalone(MapDocument::new("map-a")),
        None,
        Limits::default(),
        HistoryBudget {
            entries: 2,
            bytes: usize::MAX,
        },
    )
    .unwrap();
    for id in ["a", "b", "c"] {
        add(&mut s, id);
    }
    assert_eq!(s.history().undo, 2);
    assert_eq!(s.history().evicted, 1);
    s.undo(&mut Standalone).unwrap();
    s.undo(&mut Standalone).unwrap();
    assert_eq!(s.snapshot().document.objects.len(), 1);
    assert_eq!(s.undo(&mut Standalone).unwrap(), CommitOutcome::NoOp);
    let mut s = MapSession::open(
        MapSnapshot::standalone(MapDocument::new("map-a")),
        None,
        Limits::default(),
        HistoryBudget {
            entries: 2,
            bytes: 1,
        },
    )
    .unwrap();
    add(&mut s, "a");
    assert_eq!(s.history().undo, 0);
    assert_eq!(s.history().bytes, 0);
    assert_eq!(s.history().evicted, 1);
}
#[test]
fn gesture_preview_is_not_persistent_and_commit_is_one_history_action() {
    let mut s = empty();
    let ticket = s.begin_gesture().unwrap();
    for id in ["preview-a", "preview-b", "preview-c"] {
        s.update_gesture(&ticket, vec![put(id)]).unwrap();
    }
    assert!(s.snapshot().document.objects.is_empty());
    assert_eq!(s.stamp().revision, 0);
    assert!(s.begin_save().unwrap().is_none());
    assert!(s.execute(vec![put("other")]).is_err());
    s.commit_gesture(&ticket, &mut Standalone).unwrap();
    assert_eq!(
        s.snapshot().document.objects[0].id,
        ObjectId::from("preview-c")
    );
    assert_eq!(s.history().undo, 1);
    let ticket = s.begin_gesture().unwrap();
    s.update_gesture(&ticket, vec![put("cancelled")]).unwrap();
    s.cancel_gesture(&ticket).unwrap();
    assert_eq!(s.stamp().revision, 1);
}
#[test]
fn invalid_gesture_can_be_retried_or_cancelled_without_partial_commit() {
    let mut s = empty();
    let t = s.begin_gesture().unwrap();
    s.update_gesture(&t, vec![Command::RemoveObject("absent".into())])
        .unwrap();
    assert!(s.commit_gesture(&t, &mut Standalone).is_err());
    assert_eq!(s.preview().len(), 1);
    assert_eq!(s.history().undo, 0);
    s.update_gesture(&t, vec![put("ok")]).unwrap();
    s.commit_gesture(&t, &mut Standalone).unwrap();
    assert_eq!(s.history().undo, 1);
}
#[test]
fn host_rejection_keeps_both_sides_and_history_untouched() {
    let snapshot = bound_snapshot();
    let mut host = Host {
        anchors: snapshot.anchors.clone(),
        fail: true,
    };
    let mut s = MapSession::open(
        snapshot.clone(),
        None,
        Limits::default(),
        HistoryBudget::default(),
    )
    .unwrap();
    let commands = vec![
        put("mixed"),
        Command::MoveAnchor {
            id: "place".into(),
            point: Some(Point::new(50.0, 60.0)),
        },
    ];
    assert!(s.execute_with_host(commands.clone(), &mut host).is_err());
    assert_eq!(s.snapshot(), &snapshot);
    assert_eq!(host.anchors, snapshot.anchors);
    assert_eq!(s.history().undo, 0);
    host.fail = false;
    s.execute_with_host(commands, &mut host).unwrap();
    assert_eq!(s.history().undo, 1);
    let pos = host.anchors[&AnchorId::from("place")]
        .position
        .as_ref()
        .unwrap();
    assert_eq!(pos.z, Some(99.0));
    assert_eq!(pos.point, Point::new(50.0, 60.0));
    assert_eq!(
        host.anchors[&AnchorId::from("place")].metadata,
        snapshot.anchors[&AnchorId::from("place")].metadata
    );
    s.undo(&mut host).unwrap();
    assert_eq!(s.snapshot(), &snapshot);
    assert_eq!(host.anchors, snapshot.anchors);
}
#[test]
fn rename_refresh_preserves_local_position_history_and_undo_never_reverts_name() {
    let snapshot = bound_snapshot();
    let persisted = StoredMap::from_snapshot(&snapshot);
    let mut host = Host {
        anchors: snapshot.anchors.clone(),
        fail: false,
    };
    let mut s = MapSession::open(
        snapshot.clone(),
        Some(persisted),
        Limits::default(),
        HistoryBudget::default(),
    )
    .unwrap();
    s.execute_with_host(
        vec![Command::MoveAnchor {
            id: "place".into(),
            point: Some(Point::new(50.0, 60.0)),
        }],
        &mut host,
    )
    .unwrap();
    let mut fresh = snapshot;
    fresh
        .anchors
        .get_mut(&AnchorId::from("place"))
        .unwrap()
        .name = "新名字".into();
    s.observe_external(&s.stamp(), fresh).unwrap();
    assert_eq!(s.stamp().revision, 1);
    assert_eq!(s.history().undo, 1);
    host.anchors.get_mut(&AnchorId::from("place")).unwrap().name = "新名字".into();
    s.undo(&mut host).unwrap();
    assert_eq!(
        s.snapshot().anchors[&AnchorId::from("place")].name,
        "新名字"
    );
    assert_eq!(host.anchors[&AnchorId::from("place")].name, "新名字");
}
#[test]
fn stale_prepared_edit_and_old_session_tasks_cannot_commit() {
    let mut s = empty();
    let edit = s.prepare(&s.stamp(), vec![put("stale")]).unwrap();
    add(&mut s, "newer");
    assert_eq!(s.accept(edit, &mut Standalone), Err(MapError::Stale));
    let stamp = s.stamp();
    let resource = StoredMap::from_snapshot(s.snapshot());
    s.reload(s.snapshot().clone(), Some(resource)).unwrap();
    assert_ne!(s.stamp().token, stamp.token);
    assert_eq!(s.check_stamp(&stamp), Err(MapError::Stale));
}
#[test]
fn opening_empty_does_not_write_and_save_does_not_clear_history() {
    let mut s = empty();
    let mut store = MemoryStore::default();
    s.flush(&mut store).unwrap();
    assert!(store
        .load(&"map-a".into(), &Limits::default())
        .unwrap()
        .is_none());
    add(&mut s, "a");
    s.flush(&mut store).unwrap();
    assert_eq!(s.history().undo, 1);
    assert_eq!(s.saved_revision(), 1);
    assert_eq!(s.save_state(), &SaveState::Saved);
    let raw = store
        .load(&"map-a".into(), &Limits::default())
        .unwrap()
        .unwrap()
        .encode(&Limits::default())
        .unwrap();
    let loaded = StoredMap::decode(&raw, &Limits::default()).unwrap();
    let reopened = MapSession::open(
        loaded.clone().into_snapshot(),
        Some(loaded),
        Limits::default(),
        HistoryBudget::default(),
    )
    .unwrap();
    assert_eq!(reopened.snapshot(), s.snapshot());
    assert_eq!(reopened.history().undo, 0);
}
#[test]
fn editing_r_plus_one_during_save_r_advances_only_acknowledged_revision() {
    let mut s = empty();
    let mut store = MemoryStore::default();
    add(&mut s, "r1");
    let first = s.begin_save().unwrap().unwrap();
    add(&mut s, "r2");
    assert_eq!(s.begin_save().unwrap_err(), MapError::SaveInFlight);
    store.save(&first, &s.limits()).unwrap();
    assert!(s.complete_save(first.receipt(Ok(()))));
    assert_eq!(s.saved_revision(), 1);
    assert_eq!(s.save_state(), &SaveState::Pending);
    assert!(s.is_dirty());
    let second = s.begin_save().unwrap().unwrap();
    assert_eq!(second.stamp.revision, 2);
    assert_eq!(second.baseline.as_ref(), Some(&first.snapshot));
    store.save(&second, &s.limits()).unwrap();
    s.complete_save(second.receipt(Ok(())));
    assert_eq!(s.saved_revision(), 2);
    assert!(!s.is_dirty());
    assert!(!s.complete_save(first.receipt(Ok(()))));
}
#[test]
fn save_failure_retry_and_old_retry_receipts_are_isolated() {
    let mut s = empty();
    add(&mut s, "draft");
    let first = s.begin_save().unwrap().unwrap();
    assert!(s.complete_save(first.receipt(Err(MapError::Storage("disk full".into())))));
    assert!(matches!(s.save_state(), SaveState::Failed(_)));
    assert!(s.is_dirty());
    assert_eq!(s.history().undo, 1);
    let retry = s.begin_save().unwrap().unwrap();
    assert_ne!(first.request_id, retry.request_id);
    assert!(!s.complete_save(first.receipt(Ok(()))));
    assert!(s.is_saving());
    s.complete_save(retry.receipt(Ok(())));
    assert!(!s.is_dirty());
}
#[test]
fn undo_to_saved_content_needs_no_write_but_pending_write_must_settle() {
    let mut s = empty();
    add(&mut s, "a");
    s.undo(&mut Standalone).unwrap();
    assert!(!s.is_dirty());
    assert_eq!(s.saved_revision(), 2);
    assert!(s.begin_save().unwrap().is_none());
    s.redo(&mut Standalone).unwrap();
    let r = s.begin_save().unwrap().unwrap();
    s.undo(&mut Standalone).unwrap();
    assert!(s.is_saving());
    s.complete_save(r.receipt(Ok(())));
    assert!(s.is_dirty());
    assert_eq!(s.save_state(), &SaveState::Pending);
    assert!(s.begin_save().unwrap().is_some());
}
#[test]
fn switch_and_close_failures_retain_draft_history_and_allow_retry() {
    let mut s = empty();
    add(&mut s, "draft");
    let before = s.snapshot().clone();
    assert_eq!(
        s.switch(MapSnapshot::standalone(MapDocument::new("b")), None),
        Err(MapError::Unsaved)
    );
    assert_eq!(s.snapshot(), &before);
    s.begin_close().unwrap();
    assert_eq!(s.execute(vec![put("no")]), Err(MapError::Closed));
    let r = s.begin_save().unwrap().unwrap();
    s.complete_save(r.receipt(Err(MapError::Storage("fail".into()))));
    assert_eq!(s.finish_close(), Err(MapError::Unsaved));
    assert_eq!(s.snapshot(), &before);
    assert_eq!(s.history().undo, 1);
    s.abort_close().unwrap();
    let mut store = MemoryStore::default();
    s.flush(&mut store).unwrap();
    let old = s.stamp();
    s.switch(MapSnapshot::standalone(MapDocument::new("b")), None)
        .unwrap();
    assert_eq!(s.history().undo, 0);
    assert_ne!(old.token, s.stamp().token);
    assert!(!s.complete_save(r.receipt(Ok(()))));
    s.begin_close().unwrap();
    s.finish_close().unwrap();
    assert_eq!(s.mode(), SessionMode::Closed);
}
#[test]
fn conflicts_preserve_draft_and_block_undo_until_explicit_reload() {
    let mut s = empty();
    add(&mut s, "draft");
    let before = s.snapshot().clone();
    let fresh = MapSnapshot::standalone(MapDocument::new("map-a"));
    let mut changed = fresh.clone();
    changed.document.style = resource();
    assert!(s.observe_external(&s.stamp(), changed.clone()).is_err());
    assert_eq!(s.snapshot(), &before);
    assert_eq!(s.history().undo, 1);
    assert!(s.undo(&mut Standalone).is_err());
    assert!(s.begin_save().is_err());
    let persisted = StoredMap::from_snapshot(&changed);
    s.reload(changed, Some(persisted)).unwrap();
    assert!(!s.is_dirty());
    assert_eq!(s.history().undo, 0);
}
#[test]
fn baseline_conflict_is_detected_by_store_without_replacing_external_content() {
    let mut s = empty();
    add(&mut s, "draft");
    let r = s.begin_save().unwrap().unwrap();
    let mut store = MemoryStore::default();
    let external = StoredMap::from_snapshot(&MapSnapshot::standalone(MapDocument::new("map-a")));
    store.replace(external.clone(), &s.limits()).unwrap();
    let err = store.save(&r, &s.limits()).unwrap_err();
    s.complete_save(r.receipt(Err(err)));
    assert!(matches!(s.save_state(), SaveState::Conflict(_)));
    assert_eq!(
        store.load(&"map-a".into(), &s.limits()).unwrap(),
        Some(external)
    );
    assert_eq!(s.snapshot().document.objects.len(), 1);
}
#[test]
fn mixed_save_contains_one_resource_and_no_copied_host_names() {
    let snapshot = bound_snapshot();
    let mut host = Host {
        anchors: snapshot.anchors.clone(),
        fail: false,
    };
    let mut s =
        MapSession::open(snapshot, None, Limits::default(), HistoryBudget::default()).unwrap();
    s.execute_with_host(
        vec![
            put("mixed"),
            Command::MoveAnchor {
                id: "place".into(),
                point: Some(Point::new(8.0, 9.0)),
            },
        ],
        &mut host,
    )
    .unwrap();
    let r = s.begin_save().unwrap().unwrap();
    assert_eq!(r.anchor_patches.len(), 1);
    let raw = r.snapshot.encode(&s.limits()).unwrap();
    assert!(!raw.contains("旧名字"));
    assert!(!raw.contains("host:extra"));
    let mut store = MemoryStore::default();
    store.save(&r, &s.limits()).unwrap();
    s.complete_save(r.receipt(Ok(())));
    assert!(!s.is_dirty());
}
#[test]
fn readonly_blocks_content_but_can_be_closed_and_reopened_explicitly() {
    let mut s = empty();
    s.set_read_only(true).unwrap();
    assert_eq!(s.execute(vec![put("a")]), Err(MapError::ReadOnly));
    s.set_read_only(false).unwrap();
    add(&mut s, "a");
    assert_eq!(s.set_read_only(true), Err(MapError::Unsaved));
}
#[test]
fn transform_bounds_and_serialized_point_contract_agree() {
    let t = Transform {
        translation: Point::new(5.0, 6.0),
        scale: 2.0,
        rotation: std::f64::consts::FRAC_PI_2,
    };
    let p = t.apply(Point::new(1.0, 0.0)).unwrap();
    assert!((p.x - 5.0).abs() < 1e-8);
    assert!((p.y - 8.0).abs() < 1e-8);
    assert!(WorldRect::default().contains(Point::new(5.0, 8.0)));
    assert!(Transform { scale: 0.0, ..t }
        .apply(Point::default())
        .is_err());
}

#[test]
fn async_prepared_edits_preserve_metadata_refresh_and_noop_stays_noop() {
    let snapshot = bound_snapshot();
    let mut s = MapSession::open(
        snapshot.clone(),
        None,
        Limits::default(),
        HistoryBudget::default(),
    )
    .unwrap();
    let edit = s.prepare(&s.stamp(), vec![put("async")]).unwrap();
    let mut fresh = snapshot.clone();
    fresh
        .anchors
        .get_mut(&AnchorId::from("place"))
        .unwrap()
        .name = "refreshed".into();
    s.observe_external(&s.stamp(), fresh.clone()).unwrap();
    s.accept(edit, &mut Standalone).unwrap();
    assert_eq!(
        s.snapshot().anchors[&AnchorId::from("place")].name,
        "refreshed"
    );
    let edit = s.prepare(&s.stamp(), vec![]).unwrap();
    fresh
        .anchors
        .get_mut(&AnchorId::from("place"))
        .unwrap()
        .name = "renamed again".into();
    s.observe_external(&s.stamp(), fresh).unwrap();
    assert_eq!(
        s.accept(edit, &mut Standalone).unwrap(),
        CommitOutcome::NoOp
    );
    assert_eq!(s.stamp().revision, 1);
}
#[test]
fn external_change_during_save_does_not_clear_conflict_when_receipt_arrives() {
    let mut s = empty();
    add(&mut s, "draft");
    let request = s.begin_save().unwrap().unwrap();
    let mut fresh = MapSnapshot::standalone(MapDocument::new("map-a"));
    fresh.document.style = resource();
    assert!(s.observe_external(&s.stamp(), fresh.clone()).is_err());
    assert!(s
        .reload(fresh.clone(), Some(StoredMap::from_snapshot(&fresh)))
        .is_err());
    s.complete_save(request.receipt(Ok(())));
    assert!(matches!(s.save_state(), SaveState::Conflict(_)));
    assert!(s.begin_save().is_err());
    assert_eq!(s.snapshot().document.objects.len(), 1);
}
#[test]
fn command_node_and_envelope_budgets_apply_without_partial_state() {
    let limits = Limits {
        max_commands: 1,
        max_nodes: 1,
        ..Limits::default()
    };
    let mut s = MapSession::open(
        MapSnapshot::standalone(MapDocument::new("map-a")),
        None,
        limits,
        HistoryBudget::default(),
    )
    .unwrap();
    assert!(s
        .execute(vec![
            Command::SetStyle(resource()),
            Command::SetFrame(WorldRect::default())
        ])
        .is_err());
    assert!(s.execute(vec![put("too-many-nodes")]).is_err());
    assert_eq!(s.stamp().revision, 0);
    let ticket = s.begin_gesture().unwrap();
    assert!(s.update_gesture(&ticket, vec![put("a"), put("b")]).is_err());
    s.cancel_gesture(&ticket).unwrap();
    let mut snapshot = bound_snapshot();
    snapshot
        .anchors
        .get_mut(&AnchorId::from("place"))
        .unwrap()
        .position
        .as_mut()
        .unwrap()
        .z = Some(f64::INFINITY);
    assert!(snapshot.validate(&Limits::default()).is_err());
}

#[test]
fn clean_external_change_clears_invalid_history_and_advances_revision() {
    let mut s = empty();
    let mut store = MemoryStore::default();
    add(&mut s, "saved");
    s.flush(&mut store).unwrap();
    let mut fresh = s.snapshot().clone();
    fresh.document.style = resource();
    s.observe_external(&s.stamp(), fresh.clone()).unwrap();
    assert_eq!(s.stamp().revision, 2);
    assert_eq!(s.saved_revision(), 2);
    assert_eq!(s.history().undo, 0);
    assert_eq!(s.snapshot(), &fresh);
}
#[test]
fn reparented_host_anchor_rejects_prepared_transaction_atomically() {
    let snapshot = bound_snapshot();
    let mut s = MapSession::open(
        snapshot.clone(),
        None,
        Limits::default(),
        HistoryBudget::default(),
    )
    .unwrap();
    let edit = s
        .prepare(
            &s.stamp(),
            vec![
                put("mixed"),
                Command::MoveAnchor {
                    id: "place".into(),
                    point: Some(Point::new(9.0, 10.0)),
                },
            ],
        )
        .unwrap();
    let mut host = Host {
        anchors: snapshot.anchors.clone(),
        fail: false,
    };
    host.anchors
        .get_mut(&AnchorId::from("place"))
        .unwrap()
        .parent_id = Some("other".into());
    assert!(s.accept(edit, &mut host).is_err());
    assert_eq!(s.snapshot(), &snapshot);
    assert_eq!(s.history().undo, 0);
}
#[test]
fn invalid_switch_target_keeps_clean_source_and_token() {
    let mut s = empty();
    let stamp = s.stamp();
    let mut target = MapDocument::new("map-b");
    target.layers.clear();
    assert!(s.switch(MapSnapshot::standalone(target), None).is_err());
    assert_eq!(s.stamp(), stamp);
}

#[test]
fn external_resource_over_envelope_budget_leaves_session_untouched() {
    let snapshot = MapSnapshot::standalone(MapDocument::new("map-a"));
    let limits = Limits {
        max_bytes: StoredMap::from_snapshot(&snapshot)
            .encode(&Limits::default())
            .unwrap()
            .len(),
        ..Limits::default()
    };
    let mut session =
        MapSession::open(snapshot.clone(), None, limits, HistoryBudget::default()).unwrap();
    let fresh = bound_snapshot();
    // The document still fits; only the complete storage envelope exceeds it.
    fresh.validate(&limits).unwrap();
    let stamp = session.stamp();
    assert!(matches!(
        session.observe_external(&stamp, fresh),
        Err(MapError::Budget(_))
    ));
    assert_eq!(session.stamp(), stamp);
    assert_eq!(session.snapshot(), &snapshot);
    assert_eq!(session.save_state(), &SaveState::Unmodified);
}
