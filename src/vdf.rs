//! Lossless text VDF scalar edits. Unrelated bytes are preserved.
//! Map keys and lookup paths are ASCII case-insensitive, as in Steam KeyValues.
use crate::{Result, fail};
use std::collections::BTreeMap;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Map(BTreeMap<String, Value>),
}
#[derive(Clone)]
struct Token {
    text: String,
    start: usize,
    end: usize,
    quoted: bool,
}
pub struct Vdf {
    pub data: Value,
    text: String,
    sections: BTreeMap<Vec<String>, usize>,
    scalars: BTreeMap<Vec<String>, (usize, usize)>,
}
fn tokens(text: &str) -> Result<Vec<Token>> {
    let b = text.as_bytes();
    let mut i = 0;
    let mut result = Vec::new();
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b[i..].starts_with(b"//") {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let start = i;
        if b[i] == b'{' || b[i] == b'}' {
            result.push(Token {
                text: (b[i] as char).to_string(),
                start,
                end: i + 1,
                quoted: false,
            });
            i += 1;
            continue;
        }
        if b[i] != b'"' {
            return fail("A Steam file uses a layout this version does not understand.");
        }
        i += 1;
        let mut bytes = Vec::new();
        let mut closed = false;
        while i < b.len() {
            if b[i] == b'"' {
                i += 1;
                closed = true;
                break;
            }
            if b[i] == b'\\' {
                i += 1;
                if i >= b.len() {
                    return fail("A Steam file ends in the middle of an escaped text value.");
                }
                match b[i] {
                    b'n' => bytes.push(b'\n'),
                    b't' => bytes.push(b'\t'),
                    b'"' | b'\\' => bytes.push(b[i]),
                    _ => {
                        bytes.push(b'\\');
                        bytes.push(b[i]);
                    }
                }
            } else {
                bytes.push(b[i]);
            }
            i += 1;
        }
        if !closed {
            return fail("A Steam file has an unfinished text value. It may be incomplete.");
        }
        result.push(Token {
            text: String::from_utf8(bytes)?,
            start,
            end: i,
            quoted: true,
        });
    }
    Ok(result)
}
impl Vdf {
    pub fn parse(text: String) -> Result<Self> {
        let items = tokens(&text)?;
        let mut this = Self {
            data: Value::Map(BTreeMap::new()),
            text,
            sections: BTreeMap::new(),
            scalars: BTreeMap::new(),
        };
        let mut position = 0;
        this.data = this.section(&items, &mut position, vec![])?;
        if position != items.len() {
            return fail("A Steam file has unexpected data after its settings.");
        }
        Ok(this)
    }
    fn section(&mut self, t: &[Token], i: &mut usize, path: Vec<String>) -> Result<Value> {
        if path.len() > 64 {
            return fail(
                "A Steam file has more nested settings than this version can safely read.",
            );
        }
        let mut out = BTreeMap::new();
        while *i < t.len() {
            let key = &t[*i];
            if !key.quoted && key.text == "}" {
                if path.is_empty() {
                    return fail("A Steam file has an unexpected closing '}' character.");
                }
                self.sections.insert(path, key.start);
                *i += 1;
                return Ok(Value::Map(out));
            }
            if !key.quoted || *i + 1 >= t.len() {
                return fail("A Steam file has a setting with a missing name or value.");
            }
            let name = key.text.to_ascii_lowercase();
            if out.contains_key(&name) {
                return fail(
                    "A Steam file contains duplicate setting names. The tool cannot choose one safely.",
                );
            }
            let value = &t[*i + 1];
            *i += 2;
            let mut child = path.clone();
            child.push(name.clone());
            let parsed = if !value.quoted && value.text == "{" {
                self.section(t, i, child)?
            } else if value.quoted {
                self.scalars.insert(child, (key.start, value.end));
                Value::Text(value.text.clone())
            } else {
                return fail("A Steam setting has a value this version cannot safely read.");
            };
            out.insert(name, parsed);
        }
        if !path.is_empty() {
            return fail("A Steam file is missing a closing '}' character. It may be incomplete.");
        }
        self.sections.insert(path, self.text.len());
        Ok(Value::Map(out))
    }
    pub fn get(&self, path: &[&str]) -> Option<&Value> {
        let mut v = &self.data;
        for part in path {
            if let Value::Map(m) = v {
                v = m.get(&part.to_ascii_lowercase())?;
            } else {
                return None;
            }
        }
        Some(v)
    }
    pub fn text(&self, path: &[&str]) -> Option<&str> {
        if let Some(Value::Text(t)) = self.get(path) {
            Some(t)
        } else {
            None
        }
    }
    pub fn set(&self, path: &[&str], value: Option<&str>) -> Result<String> {
        if path.is_empty() {
            return fail("The tool could not identify the Steam setting to edit.");
        }
        if self.get(path).is_none() && value.is_none() {
            return Ok(self.text.clone());
        }
        let mut expected = self.data.clone();
        let mut node = &mut expected;
        for part in &path[..path.len() - 1] {
            if let Value::Map(m) = node {
                node = m
                    .entry(part.to_ascii_lowercase())
                    .or_insert_with(|| Value::Map(BTreeMap::new()));
            } else {
                return fail(
                    "A Steam setting contains text where a group of settings was expected.",
                );
            }
        }
        if let Value::Map(m) = node {
            if let Some(value) = value {
                m.insert(
                    path[path.len() - 1].to_ascii_lowercase(),
                    Value::Text(value.into()),
                );
            } else {
                m.remove(&path[path.len() - 1].to_ascii_lowercase());
            }
        } else {
            return fail("The Steam setting to edit is inside an unexpected text value.");
        }
        let key = path
            .iter()
            .map(|s| s.to_ascii_lowercase())
            .collect::<Vec<_>>();
        let quote = |v: &str| {
            format!(
                "\"{}\"",
                v.replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "\\n")
                    .replace('\t', "\\t")
            )
        };
        let mut replacement = value
            .map(|v| format!("{}\t\t{}", quote(path[path.len() - 1]), quote(v)))
            .unwrap_or_default();
        let (start, end) = if let Some(pair) = self.scalars.get(&key) {
            *pair
        } else {
            let mut parent = key[..key.len() - 1].to_vec();
            while !self.sections.contains_key(&parent) {
                let name = parent
                    .pop()
                    .ok_or("The tool could not find the main group of Steam settings.")?;
                replacement = format!("{}\n{{\n{}\n}}", quote(&name), replacement);
            }
            replacement = format!("\n{replacement}\n");
            let p = self.sections[&parent];
            (p, p)
        };
        let new = format!(
            "{}{}{}",
            &self.text[..start],
            replacement,
            &self.text[end..]
        );
        if Self::parse(new.clone())?.data != expected {
            return fail(
                "Editing this game setting would change other Steam settings. The update was refused.",
            );
        }
        Ok(new)
    }
}
