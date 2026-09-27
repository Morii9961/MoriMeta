// SPDX-License-Identifier: GPL-3.0-or-later
//! Raw tag snapshot of one file: `Group1:Tag` → value, as read with
//! `-json -G1 -api StructFormat=JSONQ` (every value is a string or a list of strings).

use std::collections::BTreeMap;

use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot(BTreeMap<String, Value>);

impl Snapshot {
    /// From one object of ExifTool's JSON array (keys other than `SourceFile` are kept).
    pub fn from_json(obj: &Value) -> Snapshot {
        let mut m = BTreeMap::new();
        if let Some(o) = obj.as_object() {
            for (k, v) in o {
                if k != "SourceFile" {
                    m.insert(k.clone(), v.clone());
                }
            }
        }
        Snapshot(m)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.0.keys()
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    /// Single text value; lists are not joined (returns None for lists).
    pub fn text(&self, key: &str) -> Option<String> {
        match self.0.get(key)? {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    }

    /// List value; a single string is a one-item list (ExifTool prints one-item lists as scalars).
    pub fn list(&self, key: &str) -> Option<Vec<String>> {
        match self.0.get(key)? {
            Value::Array(a) => Some(a.iter().map(value_text).collect()),
            v => Some(vec![value_text(v)]),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.0.iter()
    }
}

pub fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
