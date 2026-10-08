//! Compact, cache-friendly file tree.
//!
//! Nodes live in one arena (`Vec<Node>`), names in one shared string buffer and
//! children in one flat `Vec<u32>` (sorted by size, descending). Parents always
//! have a lower id than their children, which makes bottom-up passes a simple
//! reverse iteration.

pub type NodeId = u32;
pub const NONE: NodeId = u32::MAX;

#[derive(Clone, Copy, Debug)]
pub struct Node {
    name_off: u32,
    name_len: u16,
    pub is_dir: bool,
    pub parent: NodeId,
    /// Allocated (or logical, depending on the scanner) size in bytes; for directories the sum of all descendants.
    pub size: u64,
    child_start: u32,
    child_len: u32,
    /// Number of files in the whole subtree (1 for a file).
    pub files: u32,
    /// Modification time, seconds since the Unix epoch (0 = unknown).
    pub mtime: u32,
}

#[derive(Default)]
pub struct Tree {
    names: String,
    nodes: Vec<Node>,
    children: Vec<NodeId>,
}

impl Tree {
    #[inline]
    pub fn root(&self) -> NodeId {
        0
    }
    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
    #[inline]
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }
    #[inline]
    pub fn name(&self, id: NodeId) -> &str {
        let n = &self.nodes[id as usize];
        &self.names[n.name_off as usize..n.name_off as usize + n.name_len as usize]
    }
    #[inline]
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        let n = &self.nodes[id as usize];
        &self.children[n.child_start as usize..(n.child_start + n.child_len) as usize]
    }
    pub fn path(&self, mut id: NodeId, sep: char) -> String {
        let mut parts = Vec::new();
        while id != NONE {
            parts.push(self.name(id));
            id = self.nodes[id as usize].parent;
        }
        parts.reverse();
        let mut s = String::new();
        for (i, p) in parts.iter().enumerate() {
            if i > 0 && !s.ends_with(sep) {
                s.push(sep);
            }
            s.push_str(p);
        }
        s
    }
    pub fn depth(&self, mut id: NodeId) -> u32 {
        let mut d = 0;
        while self.nodes[id as usize].parent != NONE {
            id = self.nodes[id as usize].parent;
            d += 1;
        }
        d
    }
    pub fn total_size(&self) -> u64 {
        self.nodes.first().map_or(0, |n| n.size)
    }
    /// Directory count in the subtree rooted at `id` (excluding itself).
    pub fn dir_count(&self, id: NodeId) -> u64 {
        let mut stack = vec![id];
        let mut c = 0;
        while let Some(n) = stack.pop() {
            for &ch in self.children(n) {
                if self.nodes[ch as usize].is_dir {
                    c += 1;
                    stack.push(ch);
                }
            }
        }
        c
    }
}

pub struct TreeBuilder {
    names: String,
    nodes: Vec<Node>,
}

impl TreeBuilder {
    pub fn new(root_name: &str) -> Self {
        let mut b = TreeBuilder { names: String::new(), nodes: Vec::new() };
        b.push(NONE, root_name, true, 0, 0);
        b
    }

    fn push(&mut self, parent: NodeId, name: &str, is_dir: bool, size: u64, mtime: u32) -> NodeId {
        let name_off = self.names.len() as u32;
        let name = if name.len() > u16::MAX as usize { &name[..u16::MAX as usize] } else { name };
        // keep UTF-8 valid when truncating
        let mut end = name.len();
        while !name.is_char_boundary(end) {
            end -= 1;
        }
        let name = &name[..end];
        self.names.push_str(name);
        let id = self.nodes.len() as NodeId;
        self.nodes.push(Node {
            name_off,
            name_len: name.len() as u16,
            is_dir,
            parent,
            size,
            child_start: 0,
            child_len: 0,
            files: if is_dir { 0 } else { 1 },
            mtime,
        });
        id
    }

    pub fn add_dir(&mut self, parent: NodeId, name: &str) -> NodeId {
        self.push(parent, name, true, 0, 0)
    }
    pub fn add_file(&mut self, parent: NodeId, name: &str, size: u64, mtime: u32) -> NodeId {
        self.push(parent, name, false, size, mtime)
    }
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Build a tree from `/`- or `\`-separated relative paths (used by the web front-end).
    pub fn add_path(&mut self, dirs: &mut std::collections::HashMap<String, NodeId>, path: &str, size: u64, mtime: u32) {
        let path = path.trim_matches(|c| c == '/' || c == '\\');
        let mut parent = 0;
        let mut acc = String::new();
        let mut it = path.split(|c| c == '/' || c == '\\').peekable();
        while let Some(part) = it.next() {
            if part.is_empty() {
                continue;
            }
            if it.peek().is_some() {
                acc.push('/');
                acc.push_str(part);
                parent = match dirs.get(&acc) {
                    Some(&id) => id,
                    None => {
                        let id = self.add_dir(parent, part);
                        dirs.insert(acc.clone(), id);
                        id
                    }
                };
            } else {
                self.add_file(parent, part, size, mtime);
            }
        }
    }

    /// Computes subtree sizes and sorts children by size (descending, ties by name).
    pub fn finish(mut self) -> Tree {
        let n = self.nodes.len();
        let mut child_count = vec![0u32; n];
        for i in (1..n).rev() {
            let (size, files, parent) = {
                let c = &self.nodes[i];
                (c.size, c.files, c.parent as usize)
            };
            let p = &mut self.nodes[parent];
            p.size += size;
            p.files += files;
            child_count[parent] += 1;
        }
        // prefix sums -> contiguous child ranges
        let mut off = 0u32;
        for i in 0..n {
            self.nodes[i].child_start = off;
            self.nodes[i].child_len = child_count[i];
            off += child_count[i];
        }
        let mut children = vec![NONE; off as usize];
        let mut fill = vec![0u32; n];
        for i in 1..n {
            let p = self.nodes[i].parent as usize;
            children[(self.nodes[p].child_start + fill[p]) as usize] = i as NodeId;
            fill[p] += 1;
        }
        let nodes = &self.nodes;
        let names = &self.names;
        let name_of = |id: NodeId| {
            let n = &nodes[id as usize];
            &names[n.name_off as usize..n.name_off as usize + n.name_len as usize]
        };
        for i in 0..n {
            let (s, l) = (nodes[i].child_start as usize, nodes[i].child_len as usize);
            children[s..s + l].sort_unstable_by(|&a, &b| {
                nodes[b as usize]
                    .size
                    .cmp(&nodes[a as usize].size)
                    .then_with(|| name_of(a).cmp(name_of(b)))
            });
        }
        Tree { names: self.names, nodes: self.nodes, children }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_order() {
        let mut b = TreeBuilder::new("root");
        let d = b.add_dir(0, "d");
        b.add_file(d, "a", 10, 0);
        b.add_file(d, "b", 30, 0);
        b.add_file(0, "c", 20, 0);
        let t = b.finish();
        assert_eq!(t.total_size(), 60);
        assert_eq!(t.node(1).size, 40);
        assert_eq!(t.node(0).files, 3);
        let names: Vec<_> = t.children(0).iter().map(|&c| t.name(c)).collect();
        assert_eq!(names, ["d", "c"]);
        let names: Vec<_> = t.children(d).iter().map(|&c| t.name(c)).collect();
        assert_eq!(names, ["b", "a"]);
        assert_eq!(t.path(t.children(d)[0], '/'), "root/d/b");
    }

    #[test]
    fn paths() {
        let mut b = TreeBuilder::new("r");
        let mut m = Default::default();
        b.add_path(&mut m, "x/y/z.txt", 5, 0);
        b.add_path(&mut m, "x/y/w.txt", 7, 0);
        b.add_path(&mut m, "x/q.txt", 1, 0);
        let t = b.finish();
        assert_eq!(t.total_size(), 13);
        assert_eq!(t.dir_count(0), 2);
    }
}
