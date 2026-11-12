//! Optional editable compass rose, with versioned payload and safe vector output.
use crate::{
    command::Command,
    document::*,
    error::{MapError, MapResult},
    geometry::*,
    plugin::*,
    registry::{Registry, SymbolPath},
};
use serde::{Deserialize, Serialize};
/// Payload v1. Scale/rotation/placement live in the ordinary object transform.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compass {
    /// Outer radius in local world units (4..=200).
    pub radius: f64,
    /// Ratio of inner points to outer points (0.1..=0.8).
    pub inset: f64,
}
impl Default for Compass {
    fn default() -> Self {
        Self {
            radius: 40.,
            inset: 0.3,
        }
    }
}
/// ID `official:objects`, v1, with a creation action and host-owned property slot.
#[derive(Debug)]
pub struct ObjectsPlugin;
impl Plugin for ObjectsPlugin {
    fn info(&self) -> PluginInfo {
        super::info("official:objects", "Compass rose")
    }
    fn register(&self, r: &mut Registry) -> MapResult<()> {
        r.register_object(CompassCodec)?;
        r.register_action(AddCompass)?;
        r.contribute(Contribution {
            key: "official:add-compass".into(),
            label: "Compass rose".into(),
            slot: Slot::Menu,
            shortcut: None,
        })?;
        r.contribute(Contribution {
            key: "official:compass-properties".into(),
            label: "Compass radius / inset".into(),
            slot: Slot::Properties,
            shortcut: None,
        })?;
        r.contribute(Contribution {
            key: "official:compass-guide".into(),
            label: "Compass guide".into(),
            slot: Slot::Overlay,
            shortcut: None,
        })
    }
}
/// Validates and renders the compass with no raw SVG or remote assets.
#[derive(Debug)]
pub struct CompassCodec;
impl ObjectCodec for CompassCodec {
    fn resource(&self) -> ResourceRef {
        resource("official:compass")
    }
    fn drawing(&self, payload: &serde_json::Value) -> MapResult<ObjectDrawing> {
        let c: Compass = serde_json::from_value(payload.clone())?;
        if !c.radius.is_finite()
            || !(4.0..=200.).contains(&c.radius)
            || !c.inset.is_finite()
            || !(0.1..=0.8).contains(&c.inset)
        {
            return Err(MapError::Invalid("compass parameters".into()));
        }
        let nodes = (0..16)
            .map(|i| {
                let a = std::f64::consts::FRAC_PI_2 + i as f64 * std::f64::consts::PI / 8.;
                let radius = if i % 2 == 0 {
                    c.radius
                } else {
                    c.radius * c.inset
                };
                PathNode {
                    position: Point::new(radius * a.cos(), radius * a.sin()),
                    incoming: None,
                    outgoing: None,
                }
            })
            .collect();
        Ok(ObjectDrawing {
            bounds: WorldRect {
                min: Point::new(-c.radius, -c.radius),
                width: c.radius * 2.,
                height: c.radius * 2.,
            },
            paths: vec![SymbolPath {
                nodes,
                closed: true,
                filled: true,
            }],
        })
    }
    fn hit(&self, payload: &serde_json::Value, point: Point, tolerance: f64) -> bool {
        serde_json::from_value::<Compass>(payload.clone())
            .is_ok_and(|c| point.distance(Point::default()) <= c.radius + tolerance)
    }
}
/// Build a custom compass, using the exact same codec as validation and export.
pub fn compass_object(
    id: ObjectId,
    layer_id: LayerId,
    position: Point,
    compass: Compass,
) -> MapResult<MapObject> {
    let payload = serde_json::to_value(compass)?;
    let drawing = CompassCodec.drawing(&payload)?;
    Ok(MapObject {
        id,
        layer_id,
        geometry: Geometry::Custom {
            object_type: resource("official:compass"),
            payload: Box::new(payload),
            bounds: drawing.bounds,
            transform: Transform {
                translation: position,
                ..Transform::default()
            },
        },
        paint: Paint {
            fill: Some("#c7b485".into()),
            ..Paint::default()
        },
        extensions: Extensions::new(),
    })
}
/// Update payload and derived bounds together; ID, paint and placement are retained.
pub fn edit_compass(object: &MapObject, compass: Compass) -> MapResult<MapObject> {
    let mut next = object.clone();
    let Geometry::Custom {
        object_type,
        payload,
        bounds,
        ..
    } = &mut next.geometry
    else {
        return Err(MapError::Invalid("compass required".into()));
    };
    if *object_type != resource("official:compass") {
        return Err(MapError::Invalid("compass version".into()));
    }
    **payload = serde_json::to_value(compass)?;
    *bounds = CompassCodec.drawing(payload)?.bounds;
    Ok(next)
}
/// Public creation action; JSON parameters are a world-space Point.
#[derive(Debug)]
pub struct AddCompass;
impl Action for AddCompass {
    fn key(&self) -> &str {
        "official:add-compass"
    }
    fn commands(&self, s: &MapSnapshot, parameters: &serde_json::Value) -> MapResult<Vec<Command>> {
        let position: Point = serde_json::from_value(parameters.clone())?;
        let layer = s
            .document
            .layers
            .iter()
            .find(|l| l.visible && !l.locked)
            .ok_or_else(|| MapError::Invalid("no editable layer".into()))?;
        let object = compass_object(
            crate::editor::next_object_id(s, s.document.objects.len() as u64)?,
            layer.id.clone(),
            position,
            Compass::default(),
        )?;
        Ok(vec![Command::PutObject {
            object,
            index: s.document.objects.len(),
        }])
    }
}
