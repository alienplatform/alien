//! The document as a tree whose every value and key knows where it came from.
//!
//! Sugar rewrites this tree into the shape of alien-core's types, so a node it makes up
//! carries the span of the node it was made from, and an error about it still points at
//! something the author wrote.

use std::ops::Range;

use toml::de::{DeTable, DeValue};
use toml::Spanned;

/// A value of the document with its position.
#[derive(Debug, Clone)]
pub struct Node {
    pub span: Range<usize>,
    pub value: Value,
}

/// What a node holds.
#[derive(Debug, Clone)]
pub enum Value {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Datetime,
    Array(Vec<Node>),
    Table(Table),
}

/// A table: entries in the order the file declares them.
#[derive(Debug, Clone, Default)]
pub struct Table {
    pub entries: Vec<Entry>,
}

/// A key of a table and its value.
#[derive(Debug, Clone)]
pub struct Entry {
    pub key: String,
    pub key_span: Range<usize>,
    pub value: Node,
}

impl Node {
    pub fn new(span: Range<usize>, value: Value) -> Self {
        Node { span, value }
    }

    pub fn string(span: Range<usize>, text: impl Into<String>) -> Self {
        Node::new(span, Value::String(text.into()))
    }

    pub fn integer(span: Range<usize>, number: i64) -> Self {
        Node::new(span, Value::Integer(number))
    }

    pub fn boolean(span: Range<usize>, flag: bool) -> Self {
        Node::new(span, Value::Boolean(flag))
    }

    pub fn array(span: Range<usize>, items: Vec<Node>) -> Self {
        Node::new(span, Value::Array(items))
    }

    pub fn table(span: Range<usize>, table: Table) -> Self {
        Node::new(span, Value::Table(table))
    }

    pub fn as_str(&self) -> Option<&str> {
        match &self.value {
            Value::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_table(&self) -> Option<&Table> {
        match &self.value {
            Value::Table(table) => Some(table),
            _ => None,
        }
    }

    pub fn as_table_mut(&mut self) -> Option<&mut Table> {
        match &mut self.value {
            Value::Table(table) => Some(table),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Node]> {
        match &self.value {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    /// What the value is, for messages such as "expected a table, found a string".
    pub fn kind(&self) -> &'static str {
        match &self.value {
            Value::String(_) => "a string",
            Value::Integer(_) => "an integer",
            Value::Float(_) => "a number",
            Value::Boolean(_) => "a boolean",
            Value::Datetime => "a date",
            Value::Array(_) => "a list",
            Value::Table(_) => "a table",
        }
    }

    /// A number as JavaScript prints it: `2`, `0.5`.
    pub fn number_text(&self) -> Option<String> {
        match &self.value {
            Value::Integer(number) => Some(number.to_string()),
            Value::Float(number) => Some(number.to_string()),
            _ => None,
        }
    }
}

impl Table {
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.entries
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| &entry.value)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Node> {
        self.entries
            .iter_mut()
            .find(|entry| entry.key == key)
            .map(|entry| &mut entry.value)
    }

    pub fn entry(&self, key: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.key == key)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.entry(key).is_some()
    }

    pub fn remove(&mut self, key: &str) -> Option<Entry> {
        let index = self.entries.iter().position(|entry| entry.key == key)?;
        Some(self.entries.remove(index))
    }

    /// Adds an entry; the key's span is that of the node it is made from.
    pub fn insert(&mut self, key: &str, key_span: Range<usize>, value: Node) {
        self.entries.push(Entry {
            key: key.to_string(),
            key_span,
            value,
        });
    }

    /// Sets `key` unless the table has it.
    pub fn insert_missing(&mut self, key: &str, key_span: Range<usize>, value: Node) {
        if !self.contains(key) {
            self.insert(key, key_span, value);
        }
    }
}

/// Converts a parsed document. The parser keeps tables sorted by key, so the entries are put
/// back in the order the file declares them.
pub fn from_document(document: Spanned<DeTable<'_>>) -> Node {
    let span = document.span();
    table_node(span, document.into_inner())
}

fn table_node(span: Range<usize>, table: DeTable<'_>) -> Node {
    let mut entries: Vec<Entry> = table
        .into_iter()
        .map(|(key, value)| Entry {
            key: key.get_ref().to_string(),
            key_span: key.span(),
            value: value_node(value),
        })
        .collect();
    entries.sort_by_key(|entry| entry.key_span.start);
    Node::table(span, Table { entries })
}

fn value_node(value: Spanned<DeValue<'_>>) -> Node {
    let span = value.span();
    match value.into_inner() {
        DeValue::String(text) => Node::string(span, text),
        DeValue::Integer(number) => match i64::from_str_radix(number.as_str(), number.radix()) {
            Ok(number) => Node::integer(span, number),
            Err(_) => Node::new(span, Value::Float(f64::INFINITY)),
        },
        DeValue::Float(number) => {
            let number = number.as_str().parse().unwrap_or(f64::NAN);
            Node::new(span, Value::Float(number))
        }
        DeValue::Boolean(flag) => Node::boolean(span, flag),
        DeValue::Datetime(_) => Node::new(span, Value::Datetime),
        DeValue::Array(items) => Node::array(span, items.into_iter().map(value_node).collect()),
        DeValue::Table(table) => table_node(span, table),
    }
}
