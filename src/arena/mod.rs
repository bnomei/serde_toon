use smol_str::SmolStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Null,
    Bool,
    Number,
    String,
    Array,
    Object,
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

#[derive(Debug)]
pub struct Node {
    pub kind: NodeKind,
    pub first_child: usize,
    pub child_len: usize,
    pub data: NodeData,
}

#[derive(Debug, Default)]
pub struct Arena {
    pub nodes: Vec<Node>,
    pub strings: Vec<String>,
    pub numbers: Vec<String>,
    pub children: Vec<usize>,
    pub pairs: Vec<Pair>,
    pub keys: Vec<SmolStr>,
}

impl Arena {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Debug, Default)]
pub struct ArenaParts {
    pub nodes: Vec<Node>,
    pub strings: Vec<StringRef>,
    pub numbers: Vec<Span>,
    pub children: Vec<usize>,
    pub pairs: Vec<Pair>,
    pub keys: Vec<SmolStr>,
}

impl ArenaParts {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.strings.clear();
        self.numbers.clear();
        self.children.clear();
        self.pairs.clear();
        self.keys.clear();
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
    pub keys: Vec<SmolStr>,
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
            keys: Vec::new(),
        }
    }

    pub fn with_parts(input: &'a str, mut parts: ArenaParts) -> Self {
        parts.clear();
        Self {
            input,
            nodes: parts.nodes,
            strings: parts.strings,
            numbers: parts.numbers,
            children: parts.children,
            pairs: parts.pairs,
            keys: parts.keys,
        }
    }

    pub fn into_parts(self) -> ArenaParts {
        ArenaParts {
            nodes: self.nodes,
            strings: self.strings,
            numbers: self.numbers,
            children: self.children,
            pairs: self.pairs,
            keys: self.keys,
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

    pub fn get_key(&self, index: usize) -> Option<&str> {
        self.keys.get(index).map(|key| key.as_str())
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
