//! Original, editable reference map. All details are normal persisted objects.
use dioxus_map_editor::{document::*, error::MapResult, geometry::*, map};

fn resource(key: &str) -> ResourceRef {
    ResourceRef {
        key: key.into(),
        version: 1,
    }
}
fn point(x: f64, y: f64) -> Point {
    Point::new(x, 740. - y)
}
fn object(
    doc: &mut MapDocument,
    layer: &str,
    role: &str,
    geometry: Geometry,
    fill: Option<&str>,
    width: Option<f64>,
) {
    doc.objects.push(MapObject {
        id: ObjectId(format!("sample-{}", doc.objects.len())),
        layer_id: layer.into(),
        geometry,
        paint: Paint {
            role: role.into(),
            fill: fill.map(str::to_string),
            width,
            ..Paint::default()
        },
        extensions: Extensions::new(),
    });
}
fn path(
    doc: &mut MapDocument,
    layer: &str,
    role: &str,
    points: &[(f64, f64)],
    closed: bool,
    fill: Option<&str>,
    width: Option<f64>,
) {
    object(
        doc,
        layer,
        role,
        Geometry::Path {
            nodes: points
                .iter()
                .map(|&(x, y)| PathNode {
                    position: point(x, y),
                    incoming: None,
                    outgoing: None,
                })
                .collect(),
            closed,
            smooth: true,
            transform: Transform::default(),
        },
        fill,
        width,
    );
}
fn label(doc: &mut MapDocument, text: &str, x: f64, y: f64, size: f64, rotation: f64) {
    object(
        doc,
        "labels",
        "label",
        Geometry::Text {
            text: text.into(),
            transform: Transform {
                translation: point(x, y),
                rotation: rotation.to_radians(),
                scale: 1.,
            },
        },
        None,
        Some(size),
    );
}

pub(crate) fn sample() -> MapResult<MapSnapshot> {
    let mut doc = MapDocument::new("atlas-studio");
    doc.style = resource("builtin:atlas");
    doc.frame = WorldRect {
        min: Point::default(),
        width: 960.,
        height: 740.,
    };
    doc.layers = vec![
        Layer::new("land", "陆地与水域"),
        Layer::new("relief", "山林地貌"),
        Layer::new("routes", "河流与道路"),
        Layer::new("places", "地点"),
        Layer::new("labels", "地图文字"),
    ];
    // Deterministic coastline with several scales of variation and a western inlet.
    let coast = (0..104)
        .map(|i| {
            let a = i as f64 * std::f64::consts::TAU / 104.;
            let r =
                1. + 0.10 * (a * 3. + 1.).sin() + 0.045 * (a * 9.).cos() + 0.022 * (a * 23.).sin();
            let inlet = if (a - 3.1).abs() < 0.45 {
                0.14 * (1. - (a - 3.1).abs() / 0.45)
            } else {
                0.
            };
            (
                474. + (r - inlet) * 238. * a.cos(),
                379. + r * 266. * a.sin(),
            )
        })
        .collect::<Vec<_>>();
    path(&mut doc, "land", "land", &coast, true, None, None);
    path(
        &mut doc,
        "land",
        "forest",
        &[
            (310., 280.),
            (359., 214.),
            (445., 235.),
            (473., 331.),
            (432., 422.),
            (331., 431.),
            (295., 360.),
        ],
        true,
        Some("#d4d3b3"),
        Some(0.05),
    );
    path(
        &mut doc,
        "land",
        "forest",
        &[
            (300., 499.),
            (427., 482.),
            (500., 528.),
            (610., 550.),
            (584., 613.),
            (422., 612.),
            (302., 574.),
        ],
        true,
        Some("#dad7b9"),
        Some(0.05),
    );
    let mut seed = 1982_u64;
    let mut rand = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        (seed >> 11) as f64 / ((1_u64 << 53) as f64)
    };
    let mut trees = Vec::new();
    for _ in 0..172 {
        let x = 303. + rand() * 166.;
        let y = 231. + rand() * 202.;
        let size = 0.37 + rand() * 0.24;
        if ((x - 383.) / 79.).powi(2) + ((y - 332.) / 101.).powi(2) < 1. {
            trees.push((x, y, size));
        }
    }
    trees.sort_by(|a, b| a.1.total_cmp(&b.1));
    for (x, y, scale) in trees {
        object(
            &mut doc,
            "relief",
            "vegetation",
            Geometry::Stamp {
                stamp: resource("builtin:pine"),
                transform: Transform {
                    translation: point(x, y),
                    scale,
                    rotation: 0.,
                },
            },
            None,
            None,
        );
    }
    for i in 0..18 {
        let x = 410. + i as f64 * 7. + (rand() - 0.5) * 49.;
        let y = 182. + i as f64 * 11. + (rand() - 0.5) * 17.;
        let scale = 0.65 + rand() * 0.62;
        object(
            &mut doc,
            "relief",
            "mountain",
            Geometry::Stamp {
                stamp: resource("builtin:ridge"),
                transform: Transform {
                    translation: point(x, y),
                    scale,
                    rotation: 0.,
                },
            },
            None,
            None,
        );
    }
    path(
        &mut doc,
        "routes",
        "river",
        &[
            (501., 278.),
            (493., 310.),
            (511., 339.),
            (492., 374.),
            (497., 412.),
            (515., 443.),
            (521., 479.),
            (555., 509.),
            (583., 532.),
            (607., 555.),
            (626., 605.),
        ],
        false,
        None,
        Some(4.2),
    );
    path(
        &mut doc,
        "routes",
        "river",
        &[(450., 337.), (475., 348.), (499., 362.)],
        false,
        None,
        Some(1.7),
    );
    path(
        &mut doc,
        "routes",
        "trail",
        &[
            (369., 480.),
            (420., 478.),
            (467., 493.),
            (521., 517.),
            (576., 531.),
            (610., 552.),
        ],
        false,
        None,
        None,
    );
    for (name, x, y) in [("灰港", 610., 552.), ("守望塔", 369., 480.)] {
        object(
            &mut doc,
            "places",
            "node",
            Geometry::Stamp {
                stamp: resource("builtin:citadel"),
                transform: Transform {
                    translation: point(x, y),
                    scale: 0.85,
                    rotation: 0.,
                },
            },
            None,
            None,
        );
        if let Some(o) = doc.objects.last_mut() {
            map::set_node_label(o, name.into(), Point::new(23., 0.))?;
        }
    }
    label(&mut doc, "苍 翠 林 地", 289., 449., 15., 12.);
    label(&mut doc, "雾 脊 山 脉", 564., 241., 15., -64.);
    label(&mut doc, "低 地 原 野", 342., 561., 13., 0.);
    label(&mut doc, "西 境 海", 135., 391., 16., 76.);
    label(&mut doc, "静 默 湾", 790., 330., 16., -76.);
    label(&mut doc, "长 河", 551., 424., 12., -60.);
    label(&mut doc, "雾岸群岛", 65., 644., 24., 0.);
    label(&mut doc, "THE MISTWARD ISLES", 65., 669., 10., 0.);
    label(&mut doc, "N", 797., 101., 12., 0.);
    path(
        &mut doc,
        "labels",
        "ink",
        &[
            (800., 118.),
            (795., 143.),
            (800., 163.),
            (805., 143.),
            (800., 118.),
        ],
        false,
        None,
        Some(0.7),
    );
    path(
        &mut doc,
        "labels",
        "ink",
        &[(779., 143.), (821., 143.)],
        false,
        None,
        Some(0.7),
    );
    path(
        &mut doc,
        "labels",
        "ink",
        &[(66., 690.), (156., 690.)],
        false,
        None,
        Some(0.8),
    );
    label(&mut doc, "50 里", 166., 694., 10., 0.);
    let snapshot = MapSnapshot::standalone(doc);
    snapshot.validate(&Limits::default())?;
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_map_is_valid_and_survives_storage() {
        let map = sample().unwrap();
        let registry = dioxus_map_editor::registry::Registry::default();
        let svg =
            dioxus_map_editor::export::export_svg(&map, &registry, &Limits::default()).unwrap();
        assert!(svg.warnings.is_empty());
        assert!(svg.svg.contains("data-coast"));
        let stored = dioxus_map_editor::storage::StoredMap::from_snapshot(&map);
        let json = stored.encode(&Limits::default()).unwrap();
        let reopened =
            dioxus_map_editor::storage::StoredMap::decode(&json, &Limits::default()).unwrap();
        assert_eq!(reopened.into_snapshot().document, map.document);
    }
}
