//! Node arena for canonical parse output.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Null,
    Bool,
    Number,
    String,
    Array,
    Object,
}

#[derive(Debug)]
pub struct Node {
    pub kind: NodeKind,
    pub first_child: usize,
    pub child_len: usize,
    pub data: NodeData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeData {
    None,
    Bool(bool),
    String(usize),
    Number(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug)]
pub struct Arena {
    pub nodes: Vec<Node>,
    pub strings: Vec<String>,
}

impl Arena {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            strings: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct ArenaView<'a> {
    pub input: &'a str,
    pub nodes: Vec<Node>,
    pub strings: Vec<StringRef>,
    pub numbers: Vec<Span>,
    pub children: Vec<usize>,
    pub pairs: Vec<Pair>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StringRef {
    Span(Span),
    Owned(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pair {
    pub key: usize,
    pub value: usize,
}

impl<'a> ArenaView<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            nodes: Vec::new(),
            strings: Vec::new(),
            numbers: Vec::new(),
            children: Vec::new(),
            pairs: Vec::new(),
        }
    }

    pub fn get_str(&self, index: usize) -> Option<&str> {
        match self.strings.get(index)? {
            StringRef::Span(span) => self.input.get(span.start..span.end),
            StringRef::Owned(value) => Some(value.as_str()),
        }
    }

    pub fn get_num_str(&self, index: usize) -> Option<&'a str> {
        let span = self.numbers.get(index)?;
        self.input.get(span.start..span.end)
    }

    pub fn children(&self, node: &Node) -> &[usize] {
        let start = node.first_child;
        let end = start.saturating_add(node.child_len);
        self.children.get(start..end).unwrap_or(&[])
    }

    pub fn pairs(&self, node: &Node) -> &[Pair] {
        let start = node.first_child;
        let end = start.saturating_add(node.child_len);
        self.pairs.get(start..end).unwrap_or(&[])
    }
}
