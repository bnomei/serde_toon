use std::fmt;

use ::serde::de::{
    self, DeserializeOwned, EnumAccess, IntoDeserializer, MapAccess, SeqAccess, VariantAccess,
    Deserializer, Visitor,
};

use crate::types::{JsonValue, Number};

pub(crate) fn from_value<T: DeserializeOwned>(value: &JsonValue) -> Result<T, DeError> {
    T::deserialize(value)
}

#[derive(Debug)]
pub(crate) struct DeError {
    msg: String,
}

impl de::Error for DeError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        DeError {
            msg: msg.to_string(),
        }
    }
}

impl fmt::Display for DeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl std::error::Error for DeError {}

impl<'de> de::Deserializer<'de> for &'de JsonValue {
    type Error = DeError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            JsonValue::Null => visitor.visit_unit(),
            JsonValue::Bool(b) => visitor.visit_bool(*b),
            JsonValue::Number(n) => visit_number(visitor, n),
            JsonValue::String(s) => visitor.visit_str(s),
            JsonValue::Array(arr) => visitor.visit_seq(SeqDeserializer::new(arr)),
            JsonValue::Object(obj) => visitor.visit_map(MapDeserializer::new(obj)),
        }
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            JsonValue::Bool(b) => visitor.visit_bool(*b),
            _ => Err(de::Error::custom("expected bool")),
        }
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            JsonValue::Number(n) => {
                if let Some(i) = n.as_i64() {
                    visitor.visit_i64(i)
                } else {
                    Err(de::Error::custom("expected i64"))
                }
            }
            _ => Err(de::Error::custom("expected i64")),
        }
    }

    fn deserialize_i128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            JsonValue::Number(n) => {
                if let Some(i) = n.as_i64() {
                    visitor.visit_i128(i as i128)
                } else {
                    Err(de::Error::custom("expected i128"))
                }
            }
            _ => Err(de::Error::custom("expected i128")),
        }
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            JsonValue::Number(n) => {
                if let Some(u) = n.as_u64() {
                    visitor.visit_u64(u)
                } else {
                    Err(de::Error::custom("expected u64"))
                }
            }
            _ => Err(de::Error::custom("expected u64")),
        }
    }

    fn deserialize_u128<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            JsonValue::Number(n) => {
                if let Some(u) = n.as_u64() {
                    visitor.visit_u128(u as u128)
                } else {
                    Err(de::Error::custom("expected u128"))
                }
            }
            _ => Err(de::Error::custom("expected u128")),
        }
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_f64(visitor)
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            JsonValue::Number(n) => {
                if let Some(f) = n.as_f64() {
                    visitor.visit_f64(f)
                } else {
                    Err(de::Error::custom("expected f64"))
                }
            }
            _ => Err(de::Error::custom("expected f64")),
        }
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            JsonValue::String(s) => {
                let mut chars = s.chars();
                let first = chars.next().ok_or_else(|| de::Error::custom("empty char"))?;
                if chars.next().is_some() {
                    return Err(de::Error::custom("expected single character"));
                }
                visitor.visit_char(first)
            }
            _ => Err(de::Error::custom("expected char")),
        }
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            JsonValue::String(s) => visitor.visit_str(s),
            _ => Err(de::Error::custom("expected string")),
        }
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            JsonValue::String(s) => visitor.visit_string(s.clone()),
            _ => Err(de::Error::custom("expected string")),
        }
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            JsonValue::String(s) => visitor.visit_bytes(s.as_bytes()),
            JsonValue::Array(arr) => visitor.visit_byte_buf(values_to_bytes(arr)?),
            _ => Err(de::Error::custom("expected bytes")),
        }
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            JsonValue::String(s) => visitor.visit_byte_buf(s.as_bytes().to_vec()),
            JsonValue::Array(arr) => visitor.visit_byte_buf(values_to_bytes(arr)?),
            _ => Err(de::Error::custom("expected bytes")),
        }
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            JsonValue::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            JsonValue::Null => visitor.visit_unit(),
            _ => Err(de::Error::custom("expected unit")),
        }
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_unit(visitor)
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

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            JsonValue::Array(arr) => visitor.visit_seq(SeqDeserializer::new(arr)),
            _ => Err(de::Error::custom("expected sequence")),
        }
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            JsonValue::Object(obj) => visitor.visit_map(MapDeserializer::new(obj)),
            _ => Err(de::Error::custom("expected map")),
        }
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            JsonValue::String(s) => visitor.visit_enum(s.as_str().into_deserializer()),
            JsonValue::Object(map) => {
                if map.len() != 1 {
                    return Err(de::Error::custom("expected single-key enum map"));
                }
                let (variant, value) = map.iter().next().expect("len checked");
                visitor.visit_enum(EnumDeserializer::new(variant, Some(value)))
            }
            _ => Err(de::Error::custom("expected enum")),
        }
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }
}

fn visit_number<'de, V>(visitor: V, n: &Number) -> Result<V::Value, DeError>
where
    V: Visitor<'de>,
{
    if let Some(i) = n.as_i64() {
        visitor.visit_i64(i)
    } else if let Some(u) = n.as_u64() {
        visitor.visit_u64(u)
    } else if let Some(f) = n.as_f64() {
        visitor.visit_f64(f)
    } else {
        Err(de::Error::custom("invalid number"))
    }
}

fn values_to_bytes(values: &[JsonValue]) -> Result<Vec<u8>, DeError> {
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        match value {
            JsonValue::Number(n) => {
                if let Some(u) = n.as_u64() {
                    if u <= u8::MAX as u64 {
                        out.push(u as u8);
                        continue;
                    }
                }
                return Err(de::Error::custom("byte value out of range"));
            }
            _ => return Err(de::Error::custom("expected byte array")),
        }
    }
    Ok(out)
}

struct SeqDeserializer<'de> {
    iter: std::slice::Iter<'de, JsonValue>,
}

impl<'de> SeqDeserializer<'de> {
    fn new(values: &'de [JsonValue]) -> Self {
        SeqDeserializer {
            iter: values.iter(),
        }
    }
}

impl<'de> SeqAccess<'de> for SeqDeserializer<'de> {
    type Error = DeError;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some(value) => seed.deserialize(value).map(Some),
            None => Ok(None),
        }
    }
}

struct MapDeserializer<'de> {
    iter: indexmap::map::Iter<'de, String, JsonValue>,
    value: Option<&'de JsonValue>,
}

impl<'de> MapDeserializer<'de> {
    fn new(map: &'de indexmap::IndexMap<String, JsonValue>) -> Self {
        MapDeserializer {
            iter: map.iter(),
            value: None,
        }
    }
}

impl<'de> MapAccess<'de> for MapDeserializer<'de> {
    type Error = DeError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: de::DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some((key, value)) => {
                self.value = Some(value);
                seed.deserialize(key.as_str().into_deserializer()).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        match self.value.take() {
            Some(value) => seed.deserialize(value),
            None => Err(de::Error::custom("value is missing for key")),
        }
    }
}

struct EnumDeserializer<'de> {
    variant: &'de str,
    value: Option<&'de JsonValue>,
}

impl<'de> EnumDeserializer<'de> {
    fn new(variant: &'de str, value: Option<&'de JsonValue>) -> Self {
        EnumDeserializer { variant, value }
    }
}

impl<'de> EnumAccess<'de> for EnumDeserializer<'de> {
    type Error = DeError;
    type Variant = VariantDeserializer<'de>;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        let val = seed.deserialize(self.variant.into_deserializer())?;
        Ok((val, VariantDeserializer { value: self.value }))
    }
}

struct VariantDeserializer<'de> {
    value: Option<&'de JsonValue>,
}

impl<'de> VariantAccess<'de> for VariantDeserializer<'de> {
    type Error = DeError;

    fn unit_variant(self) -> Result<(), Self::Error> {
        match self.value {
            None => Ok(()),
            Some(JsonValue::Null) => Ok(()),
            _ => Err(de::Error::custom("expected unit variant")),
        }
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        match self.value {
            Some(value) => seed.deserialize(value),
            None => Err(de::Error::custom("expected newtype variant")),
        }
    }

    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Some(value) => value.deserialize_seq(visitor),
            None => Err(de::Error::custom("expected tuple variant")),
        }
    }

    fn struct_variant<V>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.value {
            Some(value) => value.deserialize_map(visitor),
            None => Err(de::Error::custom("expected struct variant")),
        }
    }
}
