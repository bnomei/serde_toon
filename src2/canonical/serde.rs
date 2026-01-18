//! Serde bridge for arena-based decoding and streaming encoding.

use std::fmt;

use serde::de::{self, Error as _, IntoDeserializer, MapAccess, SeqAccess, Visitor};

use super::arena::{Arena, ArenaView, NodeData, NodeKind};

pub fn decode_from_arena<T: serde::de::DeserializeOwned>(_arena: &Arena) -> crate::ToonResult<T> {
    Err(crate::ToonError::DeserializationError(
        "canonical serde bridge not implemented".to_string(),
    ))
}

pub fn decode_from_arena_view<T: serde::de::DeserializeOwned>(
    arena: &ArenaView<'_>,
) -> crate::ToonResult<T> {
    let mut de = ArenaDeserializer::new(arena, 0);
    T::deserialize(&mut de).map_err(|err| {
        crate::ToonError::DeserializationError(format!("canonical decode failed: {err}"))
    })
}

pub struct ArenaDeserializer<'a> {
    arena: &'a ArenaView<'a>,
    node_index: usize,
}

impl<'a> ArenaDeserializer<'a> {
    pub fn new(arena: &'a ArenaView<'a>, node_index: usize) -> Self {
        Self { arena, node_index }
    }

    fn node(&self) -> &super::arena::Node {
        &self.arena.nodes[self.node_index]
    }

    fn error<T: fmt::Display>(message: T) -> ArenaDeError {
        ArenaDeError {
            message: message.to_string(),
        }
    }
}

#[derive(Debug)]
pub struct ArenaDeError {
    message: String,
}

impl fmt::Display for ArenaDeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ArenaDeError {}

impl de::Error for ArenaDeError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        ArenaDeError {
            message: msg.to_string(),
        }
    }
}

impl<'de, 'a> de::Deserializer<'de> for &'a mut ArenaDeserializer<'de> {
    type Error = ArenaDeError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let node = self.node();
        match node.kind {
            NodeKind::Null => visitor.visit_unit(),
            NodeKind::Bool => match node.data {
                NodeData::Bool(value) => visitor.visit_bool(value),
                _ => Err(Self::Error::custom("invalid bool payload")),
            },
            NodeKind::String => match node.data {
                NodeData::String(index) => self
                    .arena
                    .get_str(index)
                    .map(|s| visitor.visit_str(s))
                    .unwrap_or_else(|| Err(Self::Error::custom("invalid string span"))),
                _ => Err(Self::Error::custom("invalid string payload")),
            },
            NodeKind::Number => {
                if let Ok(value) = parse_i64(self.arena, node) {
                    return visitor.visit_i64(value);
                }
                if let Ok(value) = parse_u64(self.arena, node) {
                    return visitor.visit_u64(value);
                }
                let value = parse_f64(self.arena, node)?;
                visitor.visit_f64(value)
            }
            NodeKind::Array => {
                let iter = ArrayAccess::new(self.arena, node.first_child, node.child_len);
                visitor.visit_seq(iter)
            }
            NodeKind::Object => {
                let iter = ObjectAccess::new(self.arena, node.first_child, node.child_len);
                visitor.visit_map(iter)
            }
        }
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let node = self.node();
        if node.kind != NodeKind::Bool {
            return Err(Self::Error::custom("expected bool"));
        }
        match node.data {
            NodeData::Bool(value) => visitor.visit_bool(value),
            _ => Err(Self::Error::custom("invalid bool payload")),
        }
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let node = self.node();
        if node.kind != NodeKind::Number {
            return Err(Self::Error::custom("expected number"));
        }
        let value = parse_i64(self.arena, node)?;
        visitor.visit_i64(value)
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let node = self.node();
        if node.kind != NodeKind::Number {
            return Err(Self::Error::custom("expected number"));
        }
        let value = parse_u64(self.arena, node)?;
        visitor.visit_u64(value)
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let node = self.node();
        if node.kind != NodeKind::Number {
            return Err(Self::Error::custom("expected number"));
        }
        let value = parse_f64(self.arena, node)?;
        visitor.visit_f64(value)
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let value = parse_i64_checked::<i8>(self.arena, self.node())?;
        visitor.visit_i8(value)
    }

    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let value = parse_i64_checked::<i16>(self.arena, self.node())?;
        visitor.visit_i16(value)
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let value = parse_i64_checked::<i32>(self.arena, self.node())?;
        visitor.visit_i32(value)
    }

    fn deserialize_i128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let value = parse_i128(self.arena, self.node())?;
        visitor.visit_i128(value)
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let value = parse_u64_checked::<u8>(self.arena, self.node())?;
        visitor.visit_u8(value)
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let value = parse_u64_checked::<u16>(self.arena, self.node())?;
        visitor.visit_u16(value)
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let value = parse_u64_checked::<u32>(self.arena, self.node())?;
        visitor.visit_u32(value)
    }

    fn deserialize_u128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let value = parse_u128(self.arena, self.node())?;
        visitor.visit_u128(value)
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let value = parse_f64(self.arena, self.node())? as f32;
        visitor.visit_f32(value)
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let node = self.node();
        if node.kind != NodeKind::String {
            return Err(Self::Error::custom("expected string"));
        }
        match node.data {
            NodeData::String(index) => self
                .arena
                .get_str(index)
                .map(|s| visitor.visit_str(s))
                .unwrap_or_else(|| Err(Self::Error::custom("invalid string span"))),
            _ => Err(Self::Error::custom("invalid string payload")),
        }
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let node = self.node();
        if node.kind != NodeKind::Array {
            return Err(Self::Error::custom("expected array"));
        }
        let iter = ArrayAccess::new(self.arena, node.first_child, node.child_len);
        visitor.visit_seq(iter)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let node = self.node();
        if node.kind != NodeKind::Object {
            return Err(Self::Error::custom("expected object"));
        }
        let iter = ObjectAccess::new(self.arena, node.first_child, node.child_len);
        visitor.visit_map(iter)
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let node = self.node();
        if node.kind != NodeKind::Null {
            return Err(Self::Error::custom("expected null"));
        }
        visitor.visit_unit()
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let node = self.node();
        if node.kind == NodeKind::Null {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }

    serde::forward_to_deserialize_any! {
        char bytes byte_buf unit_struct tuple tuple_struct struct enum identifier
    }
}

fn parse_number_str<'a>(
    arena: &'a ArenaView<'a>,
    node: &super::arena::Node,
) -> Result<&'a str, ArenaDeError> {
    match node.data {
        NodeData::Number(index) => arena
            .get_num_str(index)
            .ok_or_else(|| ArenaDeError::custom("invalid number span")),
        _ => Err(ArenaDeError::custom("invalid number payload")),
    }
}

fn parse_i64(arena: &ArenaView<'_>, node: &super::arena::Node) -> Result<i64, ArenaDeError> {
    let s = parse_number_str(arena, node)?;
    s.parse::<i64>()
        .map_err(|_| ArenaDeError::custom("invalid i64"))
}

fn parse_u64(arena: &ArenaView<'_>, node: &super::arena::Node) -> Result<u64, ArenaDeError> {
    let s = parse_number_str(arena, node)?;
    s.parse::<u64>()
        .map_err(|_| ArenaDeError::custom("invalid u64"))
}

fn parse_f64(arena: &ArenaView<'_>, node: &super::arena::Node) -> Result<f64, ArenaDeError> {
    let s = parse_number_str(arena, node)?;
    s.parse::<f64>()
        .map_err(|_| ArenaDeError::custom("invalid f64"))
}

fn parse_i128(arena: &ArenaView<'_>, node: &super::arena::Node) -> Result<i128, ArenaDeError> {
    let s = parse_number_str(arena, node)?;
    s.parse::<i128>()
        .map_err(|_| ArenaDeError::custom("invalid i128"))
}

fn parse_u128(arena: &ArenaView<'_>, node: &super::arena::Node) -> Result<u128, ArenaDeError> {
    let s = parse_number_str(arena, node)?;
    s.parse::<u128>()
        .map_err(|_| ArenaDeError::custom("invalid u128"))
}

fn parse_i64_checked<T>(arena: &ArenaView<'_>, node: &super::arena::Node) -> Result<T, ArenaDeError>
where
    T: TryFrom<i64>,
{
    let value = parse_i64(arena, node)?;
    T::try_from(value).map_err(|_| ArenaDeError::custom("out of range"))
}

fn parse_u64_checked<T>(arena: &ArenaView<'_>, node: &super::arena::Node) -> Result<T, ArenaDeError>
where
    T: TryFrom<u64>,
{
    let value = parse_u64(arena, node)?;
    T::try_from(value).map_err(|_| ArenaDeError::custom("out of range"))
}

pub struct ArrayAccess<'a> {
    arena: &'a ArenaView<'a>,
    index: usize,
    end: usize,
}

impl<'a> ArrayAccess<'a> {
    pub fn new(arena: &'a ArenaView<'a>, start: usize, len: usize) -> Self {
        Self {
            arena,
            index: start,
            end: start.saturating_add(len),
        }
    }
}

impl<'de, 'a> SeqAccess<'de> for ArrayAccess<'a>
where
    'a: 'de,
{
    type Error = ArenaDeError;

    fn size_hint(&self) -> Option<usize> {
        Some(self.end.saturating_sub(self.index))
    }

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        if self.index >= self.end {
            return Ok(None);
        }
        let node_index = self
            .arena
            .children
            .get(self.index)
            .copied()
            .ok_or_else(|| Self::Error::custom("array child index out of bounds"))?;
        self.index += 1;
        let mut de = ArenaDeserializer::new(self.arena, node_index);
        seed.deserialize(&mut de).map(Some)
    }
}

pub struct ObjectAccess<'a> {
    arena: &'a ArenaView<'a>,
    index: usize,
    end: usize,
    pending_value: Option<usize>,
}

impl<'a> ObjectAccess<'a> {
    pub fn new(arena: &'a ArenaView<'a>, start: usize, len: usize) -> Self {
        Self {
            arena,
            index: start,
            end: start.saturating_add(len),
            pending_value: None,
        }
    }
}

impl<'de, 'a> MapAccess<'de> for ObjectAccess<'a>
where
    'a: 'de,
{
    type Error = ArenaDeError;

    fn size_hint(&self) -> Option<usize> {
        Some(self.end.saturating_sub(self.index))
    }

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: de::DeserializeSeed<'de>,
    {
        if self.index >= self.end {
            return Ok(None);
        }

        let pair = self
            .arena
            .pairs
            .get(self.index)
            .copied()
            .ok_or_else(|| Self::Error::custom("object pair index out of bounds"))?;
        let key_str = self
            .arena
            .get_str(pair.key)
            .ok_or_else(|| Self::Error::custom("invalid key span"))?;
        self.pending_value = Some(pair.value);
        let key_de = key_str.into_deserializer();
        let key = seed.deserialize(key_de)?;
        Ok(Some(key))
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        let value_index = self
            .pending_value
            .take()
            .ok_or_else(|| Self::Error::custom("value requested before key"))?;
        self.index += 1;
        let mut de = ArenaDeserializer::new(self.arena, value_index);
        seed.deserialize(&mut de)
    }
}
