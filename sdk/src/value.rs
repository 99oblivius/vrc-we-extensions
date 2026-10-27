//! What a value is: the Wex types a field, a parameter or a result carries,
//! their shapes and the limits a shape is held to.

use crate::*;

use serde::{Deserialize, Serialize};

/// A Wex value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    None,
    Bool(bool),
    Number(f64),
    Text(String),
    List(Vec<Value>),
}

/// The Wex type of a field, a parameter or a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Type {
    None,
    Bool,
    Number,
    Text,
    List,
}

impl Value {
    pub fn ty(&self) -> Type {
        match self {
            Value::None => Type::None,
            Value::Bool(_) => Type::Bool,
            Value::Number(_) => Type::Number,
            Value::Text(_) => Type::Text,
            Value::List(_) => Type::List,
        }
    }
}

/// What a field, a parameter or a result holds. The client checks every
/// value against it, nested ones included, and clamps numbers to their
/// ranges. A manifest's shapes stay within [`TEXT_MAX`], [`LIST_MAX`],
/// [`DEPTH_MAX`] and [`VALUES_MAX`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    /// Nothing: a method's result only.
    None,
    Bool,
    Number(Option<Range>),
    /// At most `max` bytes of UTF-8.
    Text {
        max: usize,
    },
    /// At most `max` items, each `item`.
    List {
        item: Box<Shape>,
        max: usize,
    },
    /// Exactly these items, in order: a Wex list of that length.
    Tuple(Vec<Shape>),
}

/// The longest text a shape may hold, in bytes.
pub const TEXT_MAX: usize = 4096;

/// The most items a list or tuple may hold.
pub const LIST_MAX: usize = 1024;

/// The deepest lists may nest: a list of lists.
pub const DEPTH_MAX: usize = 2;

/// The most numbers, bools and texts one call or write may carry.
pub const VALUES_MAX: usize = 16384;

impl Shape {
    pub fn ty(&self) -> Type {
        match self {
            Shape::None => Type::None,
            Shape::Bool => Type::Bool,
            Shape::Number(_) => Type::Number,
            Shape::Text { .. } => Type::Text,
            Shape::List { .. } | Shape::Tuple(_) => Type::List,
        }
    }

    /// Whether a value of it may hold text.
    pub fn holds_text(&self) -> bool {
        match self {
            Shape::Text { .. } => true,
            Shape::List { item, .. } => item.holds_text(),
            Shape::Tuple(items) => items.iter().any(Shape::holds_text),
            Shape::None | Shape::Bool | Shape::Number(_) => false,
        }
    }

    /// The most numbers, bools and texts a value of it carries.
    pub(crate) fn leaves(&self) -> usize {
        match self {
            Shape::None => 0,
            Shape::Bool | Shape::Number(_) | Shape::Text { .. } => 1,
            Shape::List { item, max } => item.leaves().saturating_mul(*max),
            Shape::Tuple(items) => items.iter().map(Shape::leaves).sum(),
        }
    }

    /// Lists nested inside it, itself included.
    pub(crate) fn depth(&self) -> usize {
        match self {
            Shape::List { item, .. } => 1 + item.depth(),
            Shape::Tuple(items) => 1 + items.iter().map(Shape::depth).max().unwrap_or(0),
            _ => 0,
        }
    }

    /// Ranges are sound, texts and lists within the ceilings, tuples not
    /// empty.
    pub(crate) fn sound(&self) -> bool {
        match self {
            Shape::None | Shape::Bool | Shape::Number(None) => true,
            Shape::Number(Some(r)) => r.min.is_finite() && r.max.is_finite() && r.min <= r.max,
            Shape::Text { max } => *max <= TEXT_MAX,
            Shape::List { item, max } => {
                *max <= LIST_MAX && !matches!(**item, Shape::None) && item.sound()
            }
            Shape::Tuple(items) => {
                !items.is_empty()
                    && items.len() <= LIST_MAX
                    && items.iter().all(|i| !matches!(i, Shape::None) && i.sound())
            }
        }
    }

    /// A reply or a published value as it is delivered: none, or `value`
    /// as [`Shape::check`] delivers it; none when it doesn't fit.
    pub fn check_result(&self, name: &str, value: Value) -> Option<Value> {
        match value {
            Value::None => Some(Value::None),
            value => self.check(name, value).ok(),
        }
    }

    /// `value` as it is delivered: of this shape, numbers finite and
    /// clamped. `name` is what an error calls it.
    pub fn check(&self, name: &str, value: Value) -> Result<Value, CheckError> {
        let ty = self.ty();
        if value.ty() != ty {
            return Err(CheckError::Type {
                name: name.to_owned(),
                expected: ty,
                got: value.ty(),
            });
        }
        match (self, value) {
            (Shape::Number(_), Value::Number(n)) if !n.is_finite() => {
                Err(CheckError::NotFinite(name.to_owned()))
            }
            (Shape::Number(range), Value::Number(n)) => {
                Ok(Value::Number(range.map_or(n, |r| n.clamp(r.min, r.max))))
            }
            (Shape::Text { max }, Value::Text(t)) if t.len() > *max => Err(CheckError::TooLong {
                name: name.to_owned(),
                max: *max,
            }),
            (Shape::List { max, .. }, Value::List(items)) if items.len() > *max => {
                Err(CheckError::TooMany {
                    name: name.to_owned(),
                    max: *max,
                })
            }
            (Shape::List { item, .. }, Value::List(items)) => items
                .into_iter()
                .map(|v| item.check(name, v))
                .collect::<Result<_, _>>()
                .map(Value::List),
            (Shape::Tuple(shapes), Value::List(items)) if items.len() != shapes.len() => {
                Err(CheckError::Length {
                    name: name.to_owned(),
                    expected: shapes.len(),
                    got: items.len(),
                })
            }
            (Shape::Tuple(shapes), Value::List(items)) => shapes
                .iter()
                .zip(items)
                .map(|(s, v)| s.check(name, v))
                .collect::<Result<_, _>>()
                .map(Value::List),
            (_, value) => Ok(value),
        }
    }
}

impl Value {
    /// Whether it is or holds text.
    pub fn holds_text(&self) -> bool {
        match self {
            Value::Text(_) => true,
            Value::List(items) => items.iter().any(Value::holds_text),
            Value::None | Value::Bool(_) | Value::Number(_) => false,
        }
    }
}

/// An inclusive range a number is clamped to.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Range {
    pub min: f64,
    pub max: f64,
}

impl Range {
    pub const fn new(min: f64, max: f64) -> Self {
        Self { min, max }
    }
}
