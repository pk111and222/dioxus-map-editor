//! Versioned vector symbols and semantic styles shared by the canvas and export.
use crate::plugin::{Action, Brush, Contribution, Format, ObjectCodec, ObjectDrawing};
use crate::{
    document::{Paint, ResourceRef},
    error::{MapError, MapResult},
    geometry::{PathNode, WorldRect},
};
use std::{collections::BTreeMap, rc::Rc};

/// A fully resolved, validated paint (no arbitrary CSS).
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedPaint {
    /// Stroke color.
    pub color: String,
    /// Optional fill color.
    pub fill: Option<String>,
    /// World-space width.
    pub width: f64,
    /// Opacity in [0, 1].
    pub opacity: f64,
}
/// A semantic theme, independent of the UI's light/dark preference.
#[derive(Clone, Debug, PartialEq)]
pub struct StyleDefinition {
    /// Stable key and version.
    pub resource: ResourceRef,
    /// Display name.
    pub name: String,
    /// Export background color.
    pub background: String,
    /// Role-specific defaults. Unknown roles resolve through `ink`.
    pub roles: BTreeMap<String, ResolvedPaint>,
    /// Optional derived map treatment, shared by canvas and standalone export.
    pub artwork: MapArtwork,
}
/// Presentation supplied by a registered style; never changes saved geometry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapArtwork {
    /// Width of translucent contour bands around closed land paths; zero disables.
    pub coast_width: f64,
    /// Add a fine, deterministic paper pattern using the style's ink color.
    pub paper_grain: bool,
    /// Use a serif stack for map labels instead of the system UI font.
    pub serif_labels: bool,
}
/// Safe code-native symbol path; never raw SVG or external URLs.
#[derive(Clone, Debug, PartialEq)]
pub struct SymbolPath {
    /// Cubic endpoints and controls.
    pub nodes: Vec<PathNode>,
    /// Close the outline.
    pub closed: bool,
    /// Whether to fill using resolved fill (or stroke color).
    pub filled: bool,
}
/// Vector symbol with declared local bounds.
#[derive(Clone, Debug, PartialEq)]
pub struct StampDefinition {
    /// Stable key and version.
    pub resource: ResourceRef,
    /// User-visible name.
    pub name: String,
    /// Local bounds for selection and placement.
    pub bounds: WorldRect,
    /// Safe vector paths.
    pub paths: Vec<SymbolPath>,
}
/// Resource catalogue. Registering a missing definition restores existing objects.
#[derive(Clone, Debug)]
pub struct Registry {
    styles: Vec<StyleDefinition>,
    stamps: Vec<StampDefinition>,
    objects: Vec<Rc<dyn ObjectCodec>>,
    brushes: Vec<Rc<dyn Brush>>,
    formats: Vec<Rc<dyn Format>>,
    actions: Vec<Rc<dyn Action>>,
    contributions: Vec<Contribution>,
}
fn reference(key: &str) -> ResourceRef {
    ResourceRef {
        key: key.into(),
        version: 1,
    }
}
fn valid_color(c: &str) -> bool {
    c.starts_with('#') && [7, 9].contains(&c.len()) && c[1..].bytes().all(|b| b.is_ascii_hexdigit())
}
fn valid_ref(r: &ResourceRef) -> bool {
    r.version > 0
        && r.key.len() <= 256
        && !r.key.chars().any(char::is_control)
        && r.key
            .split_once(':')
            .is_some_and(|(a, b)| !a.is_empty() && !b.is_empty())
}
impl Default for Registry {
    fn default() -> Self {
        let mut result = Self {
            styles: Vec::new(),
            stamps: Vec::new(),
            objects: Vec::new(),
            brushes: Vec::new(),
            formats: Vec::new(),
            actions: Vec::new(),
            contributions: Vec::new(),
        };
        for (key, name, bg, ink, fill) in [
            ("builtin:simple", "Simple", "#ffffff", "#294c47", "#d7e6dd"),
            ("builtin:paper", "Paper", "#f5eedb", "#594c3c", "#d8cbae"),
        ] {
            let paint = ResolvedPaint {
                color: ink.into(),
                fill: None,
                width: 2.5,
                opacity: 1.0,
            };
            let mut roles = BTreeMap::from([
                ("ink".into(), paint.clone()),
                (
                    "label".into(),
                    ResolvedPaint {
                        width: 18.0,
                        ..paint.clone()
                    },
                ),
            ]);
            roles.insert(
                "area".into(),
                ResolvedPaint {
                    fill: Some(fill.into()),
                    ..paint
                },
            );
            for (role, color, fill, width) in [
                ("land", "#827450", Some("#e5dcb9"), 1.5),
                ("water", "#488ba4", Some("#b5dce6"), 1.5),
                ("lake", "#488ba4", Some("#b5dce6"), 1.5),
                ("mountain", "#796b60", Some("#c9beb0"), 1.5),
                ("forest", "#42725b", Some("#b5ccb0"), 1.5),
                ("boundary", ink, None, 2.),
                ("river", "#468fab", None, 6.),
                ("road", "#a67b49", None, 4.),
                ("trail", "#947d63", None, 2.5),
                ("node", ink, None, 2.),
                ("vegetation", "#416c4b", None, 1.2),
            ] {
                roles.insert(
                    role.into(),
                    ResolvedPaint {
                        color: color.into(),
                        fill: fill.map(str::to_string),
                        width,
                        opacity: 1.,
                    },
                );
            }
            result.styles.push(StyleDefinition {
                resource: reference(key),
                name: name.into(),
                background: bg.into(),
                roles,
                artwork: MapArtwork::default(),
            });
        }
        for (key, name, points) in [
            (
                "builtin:diamond",
                "Diamond",
                vec![(0., 14.), (11., 0.), (0., -14.), (-11., 0.)],
            ),
            (
                "builtin:flag",
                "Flag",
                vec![(-10., -14.), (-10., 14.), (13., 14.), (7., 5.), (-10., 5.)],
            ),
            (
                "builtin:star",
                "Star",
                (0..10)
                    .map(|i| {
                        let a = std::f64::consts::FRAC_PI_2 + i as f64 * std::f64::consts::PI / 5.;
                        let r = if i % 2 == 0 { 16. } else { 7. };
                        (r * a.cos(), r * a.sin())
                    })
                    .collect(),
            ),
        ] {
            result.stamps.push(StampDefinition {
                resource: reference(key),
                name: name.into(),
                bounds: WorldRect {
                    min: crate::geometry::Point::new(-17., -17.),
                    width: 34.,
                    height: 34.,
                },
                paths: vec![SymbolPath {
                    nodes: points
                        .into_iter()
                        .map(|(x, y)| PathNode {
                            position: crate::geometry::Point::new(x, y),
                            incoming: None,
                            outgoing: None,
                        })
                        .collect(),
                    closed: true,
                    filled: true,
                }],
            });
        }
        for (key, name, paths) in [
            (
                "builtin:tree",
                "Tree",
                vec![
                    (vec![(0., 18.), (-12., -4.), (12., -4.)], true, true),
                    (vec![(0., -4.), (0., -15.)], false, false),
                ],
            ),
            (
                "builtin:forest",
                "Forest",
                vec![
                    (vec![(-7., 16.), (-18., -6.), (4., -6.)], true, true),
                    (vec![(8., 19.), (-3., -9.), (19., -9.)], true, true),
                ],
            ),
            (
                "builtin:mountain",
                "Mountain",
                vec![
                    (vec![(-19., -12.), (0., 17.), (19., -12.)], true, false),
                    (vec![(-6., 8.), (0., 4.), (5., 9.)], false, false),
                ],
            ),
            (
                "builtin:town",
                "Town",
                vec![
                    (
                        vec![(-12., -12.), (-12., 7.), (0., 17.), (12., 7.), (12., -12.)],
                        true,
                        true,
                    ),
                    (
                        vec![(-4., -12.), (-4., 0.), (4., 0.), (4., -12.)],
                        false,
                        false,
                    ),
                ],
            ),
        ] {
            result.stamps.push(StampDefinition {
                resource: reference(key),
                name: name.into(),
                bounds: WorldRect {
                    min: crate::geometry::Point::new(-20., -20.),
                    width: 40.,
                    height: 40.,
                },
                paths: paths
                    .into_iter()
                    .map(|(points, closed, filled)| SymbolPath {
                        nodes: points
                            .into_iter()
                            .map(|(x, y)| PathNode {
                                position: crate::geometry::Point::new(x, y),
                                incoming: None,
                                outgoing: None,
                            })
                            .collect(),
                        closed,
                        filled,
                    })
                    .collect(),
            });
        }
        // A separate resource preserves the appearance of existing paper documents.
        let mut atlas = result.style(&reference("builtin:paper")).clone();
        atlas.resource = reference("builtin:atlas");
        atlas.name = "Paper atlas".into();
        atlas.background = "#bdcbc6".into();
        atlas.artwork = MapArtwork {
            coast_width: 18.,
            paper_grain: true,
            serif_labels: true,
        };
        for (role, color, fill, width) in [
            ("ink", "#555541", None, 1.2),
            ("label", "#555541", None, 18.),
            ("land", "#7e856c", Some("#e3dcc0"), 1.1),
            ("water", "#769397", Some("#b6cdca"), 1.),
            ("lake", "#769397", Some("#b6cdca"), 1.),
            ("forest", "#9ca985", Some("#b8bd99"), 0.7),
            ("vegetation", "#53684e", Some("#a8b18d"), 0.75),
            ("mountain", "#696550", Some("#d4cfb7"), 0.85),
            ("river", "#82a8aa", None, 4.),
            ("road", "#a38b64", None, 2.),
            ("trail", "#9a886b", None, 1.5),
            ("node", "#555541", Some("#e9e2c9"), 1.),
        ] {
            atlas.roles.insert(
                role.into(),
                ResolvedPaint {
                    color: color.into(),
                    fill: fill.map(str::to_string),
                    width,
                    opacity: 1.,
                },
            );
        }
        result.styles.push(atlas);
        for (key, name, paths) in [
            (
                "builtin:pine",
                "Pine",
                vec![
                    (
                        vec![
                            (0., 19.),
                            (-4., 8.),
                            (-2., 9.),
                            (-8., 0.),
                            (-5., 1.),
                            (-11., -10.),
                            (-6., -9.),
                            (-13., -16.),
                            (12., -16.),
                            (6., -9.),
                            (10., -10.),
                            (5., 1.),
                            (8., 0.),
                            (2., 9.),
                            (4., 8.),
                        ],
                        true,
                        true,
                    ),
                    (vec![(0., 17.), (0., -20.)], false, false),
                    (vec![(0., 5.), (-4., 0.)], false, false),
                    (vec![(0., -3.), (5., -9.)], false, false),
                    (vec![(-4., -7.), (-8., -12.)], false, false),
                ],
            ),
            (
                "builtin:ridge",
                "Ridge",
                vec![
                    (
                        vec![
                            (-20., -16.),
                            (-9., 4.),
                            (-4., 12.),
                            (1., 20.),
                            (7., 6.),
                            (12., 1.),
                            (20., -16.),
                            (9., -14.),
                            (0., -17.),
                            (-8., -14.),
                        ],
                        true,
                        true,
                    ),
                    (
                        vec![(1., 18.), (2., 3.), (7., 0.), (11., -13.)],
                        false,
                        false,
                    ),
                    (vec![(2., 3.), (-3., -4.), (-7., -14.)], false, false),
                    (vec![(-4., 8.), (-9., -1.)], false, false),
                    (vec![(-5., 3.), (-13., -10.)], false, false),
                    (vec![(6., -3.), (9., -8.)], false, false),
                    (vec![(7., -7.), (9., -12.)], false, false),
                ],
            ),
            (
                "builtin:citadel",
                "Citadel",
                vec![
                    (
                        vec![
                            (-16., -13.),
                            (-16., 4.),
                            (-12., 4.),
                            (-12., 0.),
                            (-8., 0.),
                            (-8., 8.),
                            (-4., 8.),
                            (-4., 3.),
                            (0., 3.),
                            (0., 13.),
                            (5., 13.),
                            (5., 3.),
                            (9., 3.),
                            (9., 8.),
                            (14., 8.),
                            (14., -13.),
                        ],
                        true,
                        true,
                    ),
                    (
                        vec![(-4., -13.), (-4., -5.), (2., -5.), (2., -13.)],
                        false,
                        false,
                    ),
                    (vec![(-12., -3.), (-12., -7.)], false, false),
                    (vec![(9., -1.), (9., -6.)], false, false),
                ],
            ),
        ] {
            result.stamps.push(StampDefinition {
                resource: reference(key),
                name: name.into(),
                bounds: WorldRect {
                    min: crate::geometry::Point::new(-22., -22.),
                    width: 44.,
                    height: 44.,
                },
                paths: paths
                    .into_iter()
                    .map(|(points, closed, filled)| SymbolPath {
                        nodes: points
                            .into_iter()
                            .map(|(x, y)| PathNode {
                                position: crate::geometry::Point::new(x, y),
                                incoming: None,
                                outgoing: None,
                            })
                            .collect(),
                        closed,
                        filled,
                    })
                    .collect(),
            });
        }
        result
    }
}
impl Registry {
    /// All registered themes.
    pub fn styles(&self) -> &[StyleDefinition] {
        &self.styles
    }
    /// All registered symbols.
    pub fn stamps(&self) -> &[StampDefinition] {
        &self.stamps
    }
    /// Resolve an exact symbol version.
    pub fn stamp(&self, r: &ResourceRef) -> Option<&StampDefinition> {
        self.stamps.iter().find(|s| s.resource == *r)
    }
    /// Resolve an exact style version; missing themes use the built-in simple theme.
    pub fn style(&self, r: &ResourceRef) -> &StyleDefinition {
        self.styles
            .iter()
            .find(|s| s.resource == *r)
            .unwrap_or(&self.styles[0])
    }
    /// Whether a style is currently available.
    pub fn has_style(&self, r: &ResourceRef) -> bool {
        self.styles.iter().any(|s| s.resource == *r)
    }
    /// Explicit overrides survive theme changes.
    pub fn paint(&self, style: &ResourceRef, p: &Paint) -> ResolvedPaint {
        let s = self.style(style);
        let base = s.roles.get(&p.role).unwrap_or(&s.roles["ink"]);
        ResolvedPaint {
            color: p.color.clone().unwrap_or_else(|| base.color.clone()),
            fill: p.fill.clone().or_else(|| base.fill.clone()),
            width: p.width.unwrap_or(base.width),
            opacity: p.opacity.unwrap_or(base.opacity),
        }
    }
    /// Register a safe style. Duplicate keys/versions are rejected.
    pub fn register_style(&mut self, s: StyleDefinition) -> MapResult<()> {
        if !valid_ref(&s.resource)
            || !s.artwork.coast_width.is_finite()
            || !(0.0..=1000.).contains(&s.artwork.coast_width)
            || !valid_color(&s.background)
            || !s.roles.contains_key("ink")
            || s.roles.values().any(|p| {
                !valid_color(&p.color)
                    || p.fill.as_ref().is_some_and(|c| !valid_color(c))
                    || !p.width.is_finite()
                    || p.width <= 0.
                    || !p.opacity.is_finite()
                    || !(0.0..=1.0).contains(&p.opacity)
            })
        {
            return Err(MapError::Invalid("style definition".into()));
        }
        if self.styles.iter().any(|v| v.resource == s.resource) {
            return Err(MapError::Duplicate(s.resource.key));
        }
        self.styles.push(s);
        Ok(())
    }
    /// Register bounded vector data, rejecting invalid geometry and duplicates.
    pub fn register_stamp(&mut self, s: StampDefinition) -> MapResult<()> {
        validate_stamp(&s)?;
        if self.stamp(&s.resource).is_some() {
            return Err(MapError::Duplicate(s.resource.key));
        }
        self.stamps.push(s);
        Ok(())
    }
}

impl PartialEq for Registry {
    fn eq(&self, other: &Self) -> bool {
        self.styles == other.styles
            && self.stamps == other.stamps
            && self.contributions == other.contributions
            && self.objects.len() == other.objects.len()
            && self
                .objects
                .iter()
                .zip(&other.objects)
                .all(|(a, b)| Rc::ptr_eq(a, b))
            && self.brushes.len() == other.brushes.len()
            && self
                .brushes
                .iter()
                .zip(&other.brushes)
                .all(|(a, b)| Rc::ptr_eq(a, b))
            && self.formats.len() == other.formats.len()
            && self
                .formats
                .iter()
                .zip(&other.formats)
                .all(|(a, b)| Rc::ptr_eq(a, b))
            && self.actions.len() == other.actions.len()
            && self
                .actions
                .iter()
                .zip(&other.actions)
                .all(|(a, b)| Rc::ptr_eq(a, b))
    }
}
impl Registry {
    /// Register a custom payload codec. Multiple explicit versions can coexist.
    pub fn register_object(&mut self, codec: impl ObjectCodec + 'static) -> MapResult<()> {
        let key = codec.resource();
        check_resource(&key)?;
        if self.object_codec(&key).is_some() {
            return Err(MapError::Duplicate(key.key));
        }
        self.objects.push(Rc::new(codec));
        Ok(())
    }
    /// Resolve an exact payload version; unsupported versions remain opaque.
    pub fn object_codec(&self, resource: &ResourceRef) -> Option<&dyn ObjectCodec> {
        self.objects
            .iter()
            .find(|c| c.resource() == *resource)
            .map(|c| c.as_ref())
    }
    /// Register a sampled command-producing brush.
    pub fn register_brush(&mut self, brush: impl Brush + 'static) -> MapResult<()> {
        let key = brush.resource();
        check_resource(&key)?;
        if self.brush(&key).is_some() {
            return Err(MapError::Duplicate(key.key));
        }
        self.brushes.push(Rc::new(brush));
        Ok(())
    }
    /// Exact enabled brush implementation.
    pub fn brush(&self, resource: &ResourceRef) -> Option<&dyn Brush> {
        self.brushes
            .iter()
            .find(|b| b.resource() == *resource)
            .map(|b| b.as_ref())
    }
    /// Available brush identities for a host-owned toolbar.
    pub fn brushes(&self) -> Vec<ResourceRef> {
        self.brushes.iter().map(|b| b.resource()).collect()
    }
    /// Register an IO-free file format handler.
    pub fn register_format(&mut self, format: impl Format + 'static) -> MapResult<()> {
        let key = format.resource();
        check_resource(&key)?;
        if self.format(&key).is_some() {
            return Err(MapError::Duplicate(key.key));
        }
        self.formats.push(Rc::new(format));
        Ok(())
    }
    /// Resolve a format for a host-owned file workflow.
    pub fn format(&self, resource: &ResourceRef) -> Option<&dyn Format> {
        self.formats
            .iter()
            .find(|f| f.resource() == *resource)
            .map(|f| f.as_ref())
    }
    /// Available format keys, without adding file pickers to the library.
    pub fn formats(&self) -> Vec<ResourceRef> {
        self.formats.iter().map(|f| f.resource()).collect()
    }
    /// Register an atomic command action.
    pub fn register_action(&mut self, action: impl Action + 'static) -> MapResult<()> {
        crate::plugin::validate_key(action.key())?;
        if self.action(action.key()).is_some() {
            return Err(MapError::Duplicate(action.key().into()));
        }
        self.actions.push(Rc::new(action));
        Ok(())
    }
    /// Resolve a menu/property action.
    pub fn action(&self, key: &str) -> Option<&dyn Action> {
        self.actions
            .iter()
            .find(|a| a.key() == key)
            .map(|a| a.as_ref())
    }
    /// Declare a host-rendered slot. Shortcut strings are canonical and case insensitive.
    pub fn contribute(&mut self, mut entry: Contribution) -> MapResult<()> {
        crate::plugin::validate_key(&entry.key)?;
        if let Some(shortcut) = &mut entry.shortcut {
            *shortcut = shortcut
                .split('+')
                .map(|part| part.trim().to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join("+");
            if ["escape", "enter", "delete", "backspace"].contains(&shortcut.as_str()) {
                return Err(MapError::Duplicate(format!(
                    "reserved canvas shortcut {shortcut}"
                )));
            }
            if shortcut.is_empty() {
                return Err(MapError::Invalid("empty shortcut".into()));
            }
        }
        if self.contributions.iter().any(|c| {
            c.key == entry.key || (entry.shortcut.is_some() && c.shortcut == entry.shortcut)
        }) {
            return Err(MapError::Duplicate(entry.key));
        }
        self.contributions.push(entry);
        Ok(())
    }
    /// Descriptors for custom divs, toolbars, property panels and passive overlays.
    pub fn contributions(&self) -> &[Contribution] {
        &self.contributions
    }
    /// Validate known payloads. Missing codecs are preserved and never guessed.
    pub fn validate_snapshot(&self, s: &crate::document::MapSnapshot) -> MapResult<()> {
        self.validate_snapshot_with_limits(s, &crate::document::Limits::default())
    }
    /// Check generated vector budgets as well as encoded payload budgets.
    pub fn validate_snapshot_with_limits(
        &self,
        s: &crate::document::MapSnapshot,
        limits: &crate::document::Limits,
    ) -> MapResult<()> {
        let mut generated = 0usize;
        for object in &s.document.objects {
            let base = match &object.geometry {
                crate::document::Geometry::Stroke { points, .. } => points.len(),
                crate::document::Geometry::Path { nodes, .. } => nodes
                    .iter()
                    .map(|n| {
                        1 + usize::from(n.incoming.is_some()) + usize::from(n.outgoing.is_some())
                    })
                    .sum(),
                _ => 0,
            };
            generated = generated.checked_add(base).ok_or(MapError::Exhausted)?;
            if let crate::document::Geometry::Custom {
                object_type,
                payload,
                bounds,
                ..
            } = &object.geometry
            {
                if let Some(codec) = self.object_codec(object_type) {
                    let drawing = checked_drawing(codec, payload)?;
                    let nodes: usize = drawing
                        .paths
                        .iter()
                        .flat_map(|p| &p.nodes)
                        .map(|n| {
                            1 + usize::from(n.incoming.is_some())
                                + usize::from(n.outgoing.is_some())
                        })
                        .sum();
                    if nodes > limits.max_object_nodes {
                        return Err(MapError::Budget("custom object generated nodes".into()));
                    }
                    generated = generated.checked_add(nodes).ok_or(MapError::Exhausted)?;
                    if drawing.bounds != *bounds {
                        return Err(MapError::Invalid(
                            "custom object bounds disagree with codec".into(),
                        ));
                    }
                }
            }
        }
        if generated > limits.max_nodes {
            return Err(MapError::Budget("generated document nodes".into()));
        }
        Ok(())
    }
}
fn check_resource(key: &ResourceRef) -> MapResult<()> {
    if !valid_ref(key) {
        return Err(MapError::Invalid("plugin resource".into()));
    }
    Ok(())
}
/// Bound codec output through the same safe vector rules used by symbols.
pub(crate) fn checked_drawing(
    codec: &dyn ObjectCodec,
    payload: &serde_json::Value,
) -> MapResult<ObjectDrawing> {
    let drawing = codec.drawing(payload)?;
    validate_paths(drawing.bounds, &drawing.paths)?;
    Ok(drawing)
}

fn validate_stamp(s: &StampDefinition) -> MapResult<()> {
    check_resource(&s.resource)?;
    validate_paths(s.bounds, &s.paths)
}
fn validate_paths(bounds: WorldRect, paths: &[SymbolPath]) -> MapResult<()> {
    if !bounds.is_valid()
        || paths.is_empty()
        || paths.len() > 128
        || paths.iter().map(|p| p.nodes.len()).sum::<usize>() > 8192
        || paths.iter().any(|p| {
            p.nodes.len() < 2
                || p.nodes
                    .iter()
                    .flat_map(|n| {
                        [Some(n.position), n.incoming, n.outgoing]
                            .into_iter()
                            .flatten()
                    })
                    .any(|v| !bounds.contains(v))
        })
    {
        return Err(MapError::Invalid("bounded vector definition".into()));
    }
    Ok(())
}
