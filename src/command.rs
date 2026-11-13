//! Atomic command preparation and reversible, field-specific patches.
use crate::document::*;
use crate::error::{MapError, MapResult};
use crate::geometry::{Point, WorldRect};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A user's intent. A batch is one history entry and one revision.
#[derive(Clone, Debug)]
pub enum Command {
    /// Explicit standalone import, retaining the destination map identity.
    /// Host-bound sessions reject this operation; the caller reviews dirty state.
    ReplaceDocument(Box<MapDocument>),
    /// Insert/replace an object. An insertion uses `index`, replacement keeps order.
    PutObject {
        /// Complete object data.
        object: MapObject,
        /// Global object sequence insertion index for a new ID.
        index: usize,
    },
    /// Delete one existing object.
    RemoveObject(ObjectId),
    /// Insert/replace a layer. Replacement retains its order.
    PutLayer {
        /// Complete layer settings.
        layer: Layer,
        /// Insertion index for a new layer.
        index: usize,
    },
    /// Delete an empty layer, or explicitly request atomic deletion of its objects.
    RemoveLayer {
        /// Layer to remove.
        id: LayerId,
        /// Host confirmation policy is separate; true makes the data operation explicit.
        delete_objects: bool,
    },
    /// Exact bottom-to-top sequence, with every existing ID once.
    ReorderLayers(Vec<LayerId>),
    /// Change the finite export frame.
    SetFrame(WorldRect),
    /// Change a semantic style reference without rewriting explicit paints.
    SetStyle(ResourceRef),
    /// Set/remove one namespaced document extension, preserving other providers.
    SetExtension {
        /// Provider namespace.
        key: String,
        /// None removes the key.
        value: Option<serde_json::Value>,
    },
    /// Move/locate/unlocate a host anchor. Host acceptance is required.
    MoveAnchor {
        /// Host identity.
        id: AnchorId,
        /// World position, or None to clear location; existing Z is preserved on moves.
        point: Option<Point>,
    },
}

/// A field-limited host patch. Neither name nor metadata is replaced.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorPatch {
    /// Host resource to change.
    pub id: AnchorId,
    /// Expected parent, preventing writes after reparenting.
    pub parent_id: Option<String>,
    /// Exact coordinate baseline, including optional Z and canvas.
    pub before: Option<AnchorPosition>,
    /// Accepted coordinate result.
    pub after: Option<AnchorPosition>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Indexed<T> {
    pub index: usize,
    pub value: T,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) enum Patch {
    Document {
        before: Box<MapDocument>,
        after: Box<MapDocument>,
    },
    Object {
        before: Option<Box<Indexed<MapObject>>>,
        after: Option<Box<Indexed<MapObject>>>,
    },
    Layers {
        before: Vec<Layer>,
        after: Vec<Layer>,
    },
    Frame {
        before: WorldRect,
        after: WorldRect,
    },
    Style {
        before: ResourceRef,
        after: ResourceRef,
    },
    Extensions {
        before: Extensions,
        after: Extensions,
    },
    Anchor(AnchorPatch),
}

impl Patch {
    pub(crate) fn reversed(&self) -> Self {
        match self {
            Self::Document { before, after } => Self::Document {
                before: after.clone(),
                after: before.clone(),
            },
            Self::Object { before, after } => Self::Object {
                before: after.clone(),
                after: before.clone(),
            },
            Self::Layers { before, after } => Self::Layers {
                before: after.clone(),
                after: before.clone(),
            },
            Self::Frame { before, after } => Self::Frame {
                before: *after,
                after: *before,
            },
            Self::Style { before, after } => Self::Style {
                before: after.clone(),
                after: before.clone(),
            },
            Self::Extensions { before, after } => Self::Extensions {
                before: after.clone(),
                after: before.clone(),
            },
            Self::Anchor(a) => Self::Anchor(AnchorPatch {
                before: a.after.clone(),
                after: a.before.clone(),
                ..a.clone()
            }),
        }
    }

    pub(crate) fn apply(&self, snapshot: &mut MapSnapshot) -> MapResult<()> {
        let doc = &mut snapshot.document;
        match self {
            Self::Document { before, after } => {
                if doc != before.as_ref() {
                    return Err(MapError::Stale);
                }
                *doc = *after.clone();
            }
            Self::Object { before, after } => {
                if let Some(before) = before {
                    if doc.objects.get(before.index) != Some(&before.value) {
                        return Err(MapError::Stale);
                    }
                    doc.objects.remove(before.index);
                }
                if let Some(after) = after {
                    if after.index > doc.objects.len()
                        || doc.objects.iter().any(|o| o.id == after.value.id)
                    {
                        return Err(MapError::Stale);
                    }
                    doc.objects.insert(after.index, after.value.clone());
                }
            }
            Self::Layers { before, after } => {
                if &doc.layers != before {
                    return Err(MapError::Stale);
                }
                doc.layers = after.clone();
            }
            Self::Frame { before, after } => {
                if doc.frame != *before {
                    return Err(MapError::Stale);
                }
                doc.frame = *after;
            }
            Self::Style { before, after } => {
                if &doc.style != before {
                    return Err(MapError::Stale);
                }
                doc.style = after.clone();
            }
            Self::Extensions { before, after } => {
                if &doc.extensions != before {
                    return Err(MapError::Stale);
                }
                doc.extensions = after.clone();
            }
            Self::Anchor(patch) => {
                let anchor = snapshot
                    .anchors
                    .get_mut(&patch.id)
                    .ok_or_else(|| MapError::Missing(patch.id.to_string()))?;
                if anchor.parent_id != patch.parent_id || anchor.position != patch.before {
                    return Err(MapError::Stale);
                }
                anchor.position = patch.after.clone();
            }
        }
        Ok(())
    }
}

pub(crate) fn prepare(
    snapshot: &MapSnapshot,
    commands: Vec<Command>,
    limits: &Limits,
) -> MapResult<(MapSnapshot, Vec<Patch>)> {
    if commands.len() > limits.max_commands {
        return Err(MapError::Budget("transaction commands".into()));
    }
    let mut next = snapshot.clone();
    let mut patches = Vec::new();
    for command in commands {
        let doc = &next.document;
        let patch = match command {
            Command::ReplaceDocument(mut after) => {
                if !next.anchors.is_empty() {
                    return Err(MapError::HostRejected(
                        "standalone import cannot replace host anchors".into(),
                    ));
                }
                after.id = doc.id.clone();
                Patch::Document {
                    before: Box::new(doc.clone()),
                    after,
                }
            }
            Command::PutObject { object, index } => {
                unlocked(doc, &object.layer_id)?;
                let before = doc.objects.iter().position(|o| o.id == object.id).map(|i| {
                    Box::new(Indexed {
                        index: i,
                        value: doc.objects[i].clone(),
                    })
                });
                if let Some(existing) = &before {
                    unlocked(doc, &existing.value.layer_id)?;
                }
                let index = before.as_ref().map_or(index, |v| v.index);
                if index > doc.objects.len() {
                    return invalid("object index");
                }
                Patch::Object {
                    before,
                    after: Some(Box::new(Indexed {
                        index,
                        value: object,
                    })),
                }
            }
            Command::RemoveObject(id) => {
                let index = doc
                    .objects
                    .iter()
                    .position(|o| o.id == id)
                    .ok_or_else(|| MapError::Missing(id.to_string()))?;
                unlocked(doc, &doc.objects[index].layer_id)?;
                Patch::Object {
                    before: Some(Box::new(Indexed {
                        index,
                        value: doc.objects[index].clone(),
                    })),
                    after: None,
                }
            }
            Command::PutLayer { layer, index } => {
                let before = doc.layers.clone();
                let mut after = before.clone();
                if let Some(i) = after.iter().position(|l| l.id == layer.id) {
                    after[i] = layer;
                } else {
                    if index > after.len() {
                        return invalid("layer index");
                    }
                    after.insert(index, layer);
                }
                Patch::Layers { before, after }
            }
            Command::RemoveLayer { id, delete_objects } => {
                unlocked(doc, &id)?;
                let occupied = doc.objects.iter().any(|o| o.layer_id == id);
                if occupied && !delete_objects {
                    return invalid("nonempty layer requires explicit deletion");
                }
                if delete_objects {
                    // Remove in descending index order so inverse insertion restores exact order.
                    for index in (0..next.document.objects.len()).rev() {
                        if next.document.objects[index].layer_id == id {
                            let p = Patch::Object {
                                before: Some(Box::new(Indexed {
                                    index,
                                    value: next.document.objects[index].clone(),
                                })),
                                after: None,
                            };
                            p.apply(&mut next)?;
                            patches.push(p);
                        }
                    }
                }
                let before = next.document.layers.clone();
                let after = before.iter().filter(|l| l.id != id).cloned().collect();
                Patch::Layers { before, after }
            }
            Command::ReorderLayers(ids) => {
                let before = doc.layers.clone();
                if ids.len() != before.len() {
                    return invalid("layer order");
                }
                let mut by_id: BTreeMap<_, _> =
                    before.iter().map(|l| (l.id.clone(), l.clone())).collect();
                let mut after = Vec::new();
                for id in ids {
                    after.push(
                        by_id
                            .remove(&id)
                            .ok_or_else(|| MapError::Missing(id.to_string()))?,
                    );
                }
                Patch::Layers { before, after }
            }
            Command::SetFrame(after) => Patch::Frame {
                before: doc.frame,
                after,
            },
            Command::SetStyle(after) => Patch::Style {
                before: doc.style.clone(),
                after,
            },
            Command::SetExtension { key, value } => {
                let before = doc.extensions.clone();
                let mut after = before.clone();
                if let Some(value) = value {
                    after.insert(key, value);
                } else {
                    after.remove(&key);
                }
                Patch::Extensions { before, after }
            }
            Command::MoveAnchor { id, point } => {
                // A default projection belongs to the first layer. Check the
                // transaction's original state too: adding an appearance on an
                // unlocked layer must not bypass that projection's lock.
                // Placing an unlocated anchor creates that projection too.
                if snapshot
                    .anchors
                    .get(&id)
                    .is_some_and(|a| a.position.is_some() || point.is_some())
                    && !snapshot.document.objects.iter().any(|o| matches!(&o.geometry, Geometry::BoundMarker { anchor_id, .. } if anchor_id == &id))
                {
                    if let Some(layer) = snapshot.document.layers.first() {
                        unlocked(&snapshot.document, &layer.id)?;
                    }
                }
                let anchor = next
                    .anchors
                    .get(&id)
                    .ok_or_else(|| MapError::Missing(id.to_string()))?;
                for object in &doc.objects {
                    if matches!(&object.geometry, Geometry::BoundMarker { anchor_id, .. } if anchor_id == &id)
                    {
                        unlocked(doc, &object.layer_id)?;
                    }
                }
                if anchor.parent_id.as_deref() != Some(doc.id.0.as_str())
                    || anchor.position.as_ref().is_some_and(|p| p.canvas != doc.id)
                {
                    return invalid("anchor does not belong to this map");
                }
                let after = point.map(|point| AnchorPosition {
                    point,
                    z: anchor.position.as_ref().and_then(|p| p.z),
                    canvas: doc.id.clone(),
                });
                if let Some(position) = &after {
                    validate_position(position)?;
                }
                Patch::Anchor(AnchorPatch {
                    id,
                    parent_id: anchor.parent_id.clone(),
                    before: anchor.position.clone(),
                    after,
                })
            }
        };
        // Skip individual no-ops; a net-empty batch is filtered by the session too.
        if patch == patch.reversed() {
            continue;
        }
        patch.apply(&mut next)?;
        patches.push(patch);
    }
    next.validate(limits)?;
    crate::storage::StoredMap::from_snapshot(&next).validate(limits)?;
    Ok((next, patches))
}

fn unlocked(doc: &MapDocument, id: &LayerId) -> MapResult<()> {
    let layer = doc
        .layers
        .iter()
        .find(|l| &l.id == id)
        .ok_or_else(|| MapError::Missing(id.to_string()))?;
    if layer.locked {
        return Err(MapError::ReadOnly);
    }
    Ok(())
}

pub(crate) fn anchor_changes(before: &MapSnapshot, after: &MapSnapshot) -> Vec<AnchorPatch> {
    before
        .anchors
        .iter()
        .filter_map(|(id, a)| {
            let b = after.anchors.get(id)?;
            (a.position != b.position).then(|| AnchorPatch {
                id: id.clone(),
                parent_id: a.parent_id.clone(),
                before: a.position.clone(),
                after: b.position.clone(),
            })
        })
        .collect()
}
