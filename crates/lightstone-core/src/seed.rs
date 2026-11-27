//! Noise seeds in graph JSON.
//!
//! Seeds are 64-bit integers, but JavaScript numbers hold integers exactly only up to 2⁵³ − 1.
//! A large seed written as a JSON number is rounded when the browser reads the graph, so the
//! same graph gave a different noise realization in the browser than in Python or Rust.
//!
//! Rule: seeds up to [`JS_SAFE_MAX`] are written as numbers, larger ones as decimal strings
//! (`"5933492509060628482"`). Both forms are accepted when reading, numbers of any size too
//! (graphs saved by older versions keep loading; a seed a browser already rounded stays
//! rounded).

use serde::{Deserialize, Deserializer, Serializer};

/// Largest integer a JavaScript number represents exactly (2⁵³ − 1).
pub const JS_SAFE_MAX: u64 = (1 << 53) - 1;

/// A seed as a JSON value: a number if it is safe in JavaScript, else a decimal string.
pub fn to_json(seed: u64) -> serde_json::Value {
    if seed <= JS_SAFE_MAX {
        serde_json::Value::from(seed)
    } else {
        serde_json::Value::String(seed.to_string())
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Raw {
    Num(u64),
    Str(String),
}

fn parse<E: serde::de::Error>(r: Raw) -> Result<u64, E> {
    match r {
        Raw::Num(n) => Ok(n),
        Raw::Str(s) => {
            let t = s.trim();
            let v = match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                Some(h) => u64::from_str_radix(h, 16),
                None => t.parse::<u64>(),
            };
            v.map_err(|_| {
                E::custom(format!(
                    "invalid seed '{s}': expected a non-negative integer"
                ))
            })
        }
    }
}

/// `#[serde(with = "crate::seed::opt")]` for `Option<u64>` seeds.
pub mod opt {
    use super::*;

    pub fn serialize<S: Serializer>(v: &Option<u64>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(x) if *x > JS_SAFE_MAX => s.serialize_str(&x.to_string()),
            Some(x) => s.serialize_u64(*x),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
        Option::<Raw>::deserialize(d)?.map(parse).transpose()
    }
}

/// `#[serde(with = "crate::seed::plain")]` for `u64` seeds.
pub mod plain {
    use super::*;

    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        if *v > JS_SAFE_MAX {
            s.serialize_str(&v.to_string())
        } else {
            s.serialize_u64(*v)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        parse(Raw::deserialize(d)?)
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, PartialEq, Debug, Default)]
    #[serde(default)]
    struct P {
        #[serde(with = "super::opt")]
        seed: Option<u64>,
        #[serde(with = "super::plain")]
        base: u64,
    }

    #[test]
    fn large_seeds_round_trip_as_strings() {
        let p = P {
            seed: Some(5_933_492_509_060_628_482),
            base: 7,
        };
        let j = serde_json::to_string(&p).unwrap();
        assert_eq!(j, r#"{"seed":"5933492509060628482","base":7}"#);
        assert_eq!(serde_json::from_str::<P>(&j).unwrap(), p);
        // A JavaScript round trip of the JSON text keeps strings exact.
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["seed"], "5933492509060628482");
    }

    #[test]
    fn numbers_hex_and_null_are_accepted() {
        let p: P =
            serde_json::from_str(r#"{"seed": 5933492509060628482, "base": "0x10"}"#).unwrap();
        assert_eq!(
            p,
            P {
                seed: Some(5_933_492_509_060_628_482),
                base: 16
            }
        );
        let q: P = serde_json::from_str(r#"{"seed": null}"#).unwrap();
        assert_eq!(q.seed, None);
        assert_eq!(
            serde_json::to_string(&P {
                seed: Some(9),
                base: 0
            })
            .unwrap(),
            r#"{"seed":9,"base":0}"#
        );
        assert!(serde_json::from_str::<P>(r#"{"seed": "-3"}"#).is_err());
        assert!(serde_json::from_str::<P>(r#"{"seed": "abc"}"#).is_err());
        assert_eq!(
            super::to_json(super::JS_SAFE_MAX),
            serde_json::json!(super::JS_SAFE_MAX)
        );
        assert_eq!(
            super::to_json(super::JS_SAFE_MAX + 1),
            serde_json::json!("9007199254740992")
        );
    }
}
