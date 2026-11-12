//! CSS-pixel / world coordinate conversion. World Y points upwards.

use std::error::Error;
use std::fmt;

/// A two-dimensional coordinate. Units are determined by the containing API.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    /// Horizontal coordinate.
    pub x: f64,
    /// Vertical coordinate.
    pub y: f64,
}

impl Point {
    /// Construct a point.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Whether both coordinates are finite.
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    pub(crate) fn distance(self, other: Self) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

/// Invalid geometry supplied by a host or input device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryError {
    /// Coordinates, scale, or a zoom factor are invalid.
    InvalidInput,
}

impl fmt::Display for GeometryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("coordinates must be finite and zoom factors must be positive")
    }
}

impl Error for GeometryError {}

/// A world-to-local-screen transform. Screen Y points downwards.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    origin: Point,
    scale: f64,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            origin: Point::new(48.0, 280.0),
            scale: 1.0,
        }
    }
}

impl Viewport {
    /// Smallest supported scale.
    pub const MIN_SCALE: f64 = 0.125;
    /// Largest supported scale.
    pub const MAX_SCALE: f64 = 8.0;

    /// Screen position of world origin.
    pub fn origin(self) -> Point {
        self.origin
    }

    /// Number of CSS pixels per world unit.
    pub fn scale(self) -> f64 {
        self.scale
    }

    /// Convert a canvas-local screen point to world coordinates.
    pub fn screen_to_world(self, point: Point) -> Point {
        Point::new(
            (point.x - self.origin.x) / self.scale,
            (self.origin.y - point.y) / self.scale,
        )
    }

    /// Convert world coordinates to canvas-local screen coordinates.
    pub fn world_to_screen(self, point: Point) -> Point {
        Point::new(
            self.origin.x + point.x * self.scale,
            self.origin.y - point.y * self.scale,
        )
    }

    /// Translate in CSS pixels. Invalid input leaves the transform unchanged.
    pub fn pan(&mut self, delta: Point) -> Result<(), GeometryError> {
        let next = Point::new(self.origin.x + delta.x, self.origin.y + delta.y);
        if !delta.is_finite() || !next.is_finite() {
            return Err(GeometryError::InvalidInput);
        }
        self.origin = next;
        Ok(())
    }

    /// Zoom while preserving the world point beneath `screen`.
    pub fn zoom_at(&mut self, screen: Point, factor: f64) -> Result<(), GeometryError> {
        if !screen.is_finite() || !factor.is_finite() || factor <= 0.0 {
            return Err(GeometryError::InvalidInput);
        }
        let world = self.screen_to_world(screen);
        let scale = (self.scale * factor).clamp(Self::MIN_SCALE, Self::MAX_SCALE);
        let origin = Point::new(screen.x - world.x * scale, screen.y + world.y * scale);
        if !origin.is_finite() {
            return Err(GeometryError::InvalidInput);
        }
        self.scale = scale;
        self.origin = origin;
        Ok(())
    }
}

/// Finite, positive world-space export frame / axis-aligned bounds.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldRect {
    /// Lower-left corner, world Y upwards.
    pub min: Point,
    /// Horizontal extent.
    pub width: f64,
    /// Vertical extent.
    pub height: f64,
}

impl Default for WorldRect {
    fn default() -> Self {
        Self {
            min: Point::default(),
            width: 1000.0,
            height: 800.0,
        }
    }
}

impl WorldRect {
    /// Validate positive finite dimensions and finite upper-right corner.
    pub fn is_valid(self) -> bool {
        self.min.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width > 0.0
            && self.height > 0.0
            && (self.min.x + self.width).is_finite()
            && (self.min.y + self.height).is_finite()
    }

    /// Whether a finite world point is inside these inclusive bounds.
    pub fn contains(self, point: Point) -> bool {
        self.is_valid()
            && point.is_finite()
            && point.x >= self.min.x
            && point.y >= self.min.y
            && point.x <= self.min.x + self.width
            && point.y <= self.min.y + self.height
    }
}

/// Translation, positive uniform scale and rotation in radians; no skew/mirror.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transform {
    /// World translation.
    pub translation: Point,
    /// Positive uniform scale.
    pub scale: f64,
    /// Counterclockwise rotation in radians.
    pub rotation: f64,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: Point::default(),
            scale: 1.0,
            rotation: 0.0,
        }
    }
}

impl Transform {
    /// Validate finite components and a positive scale.
    pub fn is_valid(self) -> bool {
        self.translation.is_finite()
            && self.scale.is_finite()
            && self.scale > 0.0
            && self.rotation.is_finite()
    }

    /// Transform a local point, rejecting overflow instead of persisting NaN/inf.
    pub fn apply(self, point: Point) -> Result<Point, GeometryError> {
        if !self.is_valid() || !point.is_finite() {
            return Err(GeometryError::InvalidInput);
        }
        let (sin, cos) = self.rotation.sin_cos();
        let result = Point::new(
            self.translation.x + (point.x * cos - point.y * sin) * self.scale,
            self.translation.y + (point.x * sin + point.y * cos) * self.scale,
        );
        if !result.is_finite() {
            return Err(GeometryError::InvalidInput);
        }
        Ok(result)
    }
}

/// A path point with optional cubic handles, in world-local coordinates.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathNode {
    /// Main path position.
    pub position: Point,
    /// Incoming absolute control point, if present.
    pub incoming: Option<Point>,
    /// Outgoing absolute control point, if present.
    pub outgoing: Option<Point>,
}
