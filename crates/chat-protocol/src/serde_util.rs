//! Serde helpers shared by the protocol modules.

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

/// Optional field that must not be `null`: `#[serde(default, deserialize_with = "non_null",
/// skip_serializing_if = "Option::is_none")]`. Zod `.optional()` rejects `null`.
pub(crate) fn non_null<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(d).map(Some)
}

/// Optional nullable field (`x?: T | null`): `#[serde(default, deserialize_with = "nullable",
/// skip_serializing_if = "Option::is_none")]` on an `Option<Option<T>>`.
pub(crate) fn nullable<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

/// Optional `unknown` value: a present `null` stays `Some(Value::Null)`.
pub(crate) fn any_value<'de, D>(d: D) -> Result<Option<serde_json::Value>, D::Error>
where
    D: Deserializer<'de>,
{
    serde_json::Value::deserialize(d).map(Some)
}

/// The JSON literal `true` (Zod `z.literal(true)`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct True;

impl Serialize for True {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bool(true)
    }
}

impl<'de> Deserialize<'de> for True {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        if bool::deserialize(d)? {
            Ok(True)
        } else {
            Err(D::Error::custom("expected true"))
        }
    }
}

/// The JSON literal `1`, the protocol version in `welcome`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ProtocolV1;

impl Serialize for ProtocolV1 {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u32(crate::PROTOCOL_VERSION)
    }
}

impl<'de> Deserialize<'de> for ProtocolV1 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = u32::deserialize(d)?;
        if v == crate::PROTOCOL_VERSION {
            Ok(ProtocolV1)
        } else {
            Err(D::Error::custom(format!("unsupported protocol {v}")))
        }
    }
}
