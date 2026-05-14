use std::collections::{HashMap, HashSet};
use tree_sitter::{Node, Parser};

#[derive(Debug, Clone)]
pub struct SymbolRec {
    pub kind: String,
    pub name: String,
    pub fq_name: String,
    pub start_byte: usize,
    pub end_byte: usize,
}

pub fn parse_rust_symbols(source: &str) -> anyhow::Result<(Vec<SymbolRec>, Option<String>)> {
    let src = source.as_bytes();
    let mut parser = Parser::new();
    let lang: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    parser.set_language(&lang)?;
    let Some(tree) = parser.parse(source, None) else {
        anyhow::bail!("parse returned None");
    };
    let root = tree.root_node();
    let mut out = Vec::new();
    visit(root, src, &mut out);
    let err = if root.has_error() {
        Some("tree contains errors".into())
    } else {
        None
    };
    out.sort_by_key(|s| s.start_byte);
    Ok((out, err))
}

fn txt<'a>(n: Node<'a>, src: &'a [u8]) -> &'a str {
    n.utf8_text(src).unwrap_or("")
}

fn visit(node: Node<'_>, src: &[u8], out: &mut Vec<SymbolRec>) {
    match node.kind() {
        "function_item" => {
            if let Some(name_n) = node.child_by_field_name("name") {
                let name = txt(name_n, src).to_string();
                let fq = name.clone();
                out.push(SymbolRec {
                    kind: "fn".into(),
                    name,
                    fq_name: fq,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                });
            }
        }
        "struct_item" => {
            if let Some(name_n) = node.child_by_field_name("name") {
                let name = txt(name_n, src).to_string();
                out.push(SymbolRec {
                    kind: "struct".into(),
                    name: name.clone(),
                    fq_name: name,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                });
            }
        }
        "enum_item" => {
            if let Some(name_n) = node.child_by_field_name("name") {
                let name = txt(name_n, src).to_string();
                out.push(SymbolRec {
                    kind: "enum".into(),
                    name: name.clone(),
                    fq_name: name,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                });
            }
        }
        "trait_item" => {
            if let Some(name_n) = node.child_by_field_name("name") {
                let name = txt(name_n, src).to_string();
                out.push(SymbolRec {
                    kind: "trait".into(),
                    name: name.clone(),
                    fq_name: name,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                });
            }
        }
        "impl_item" => {
            let ty = node
                .child_by_field_name("type")
                .map(|n| txt(n, src))
                .unwrap_or("?");
            let fq = if let Some(tr) = node.child_by_field_name("trait") {
                format!("impl {} for {}", txt(tr, src), ty)
            } else {
                format!("impl {ty}")
            };
            let name = fq.chars().take(64).collect::<String>();
            out.push(SymbolRec {
                kind: "impl".into(),
                name,
                fq_name: fq,
                start_byte: node.start_byte(),
                end_byte: node.end_byte(),
            });
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child, src, out);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymChange {
    Added,
    Removed,
    Modified,
}

pub fn diff_symbol_maps(
    prev: &[crate::db::SymbolRow],
    cur: &[SymbolRec],
) -> Vec<(SymChange, String, String)> {
    let mut pk_prev: HashMap<String, &crate::db::SymbolRow> = HashMap::new();
    for s in prev {
        let k = s
            .fq_name
            .clone()
            .unwrap_or_else(|| format!("{}/{}", s.kind, s.name));
        pk_prev.insert(k, s);
    }
    let mut pk_cur: HashMap<String, &SymbolRec> = HashMap::new();
    for s in cur {
        pk_cur.insert(s.fq_name.clone(), s);
    }
    let keys_prev: HashSet<_> = pk_prev.keys().cloned().collect();
    let keys_cur: HashSet<_> = pk_cur.keys().cloned().collect();
    let mut out = Vec::new();
    for k in keys_cur.difference(&keys_prev) {
        if let Some(s) = pk_cur.get(k) {
            out.push((SymChange::Added, s.name.clone(), s.kind.clone()));
        }
    }
    for k in keys_prev.difference(&keys_cur) {
        if let Some(s) = pk_prev.get(k) {
            out.push((SymChange::Removed, s.name.clone(), s.kind.clone()));
        }
    }
    for k in keys_prev.intersection(&keys_cur) {
        let a = pk_prev.get(k).expect("prev");
        let b = pk_cur.get(k).expect("cur");
        if a.start_byte != b.start_byte || a.end_byte != b.end_byte {
            out.push((SymChange::Modified, b.name.clone(), b.kind.clone()));
        }
    }
    out
}

pub fn summarize_changes(ch: &[(SymChange, String, String)]) -> String {
    let mut a = 0u32;
    let mut r = 0u32;
    let mut m = 0u32;
    for (k, _, _) in ch {
        match k {
            SymChange::Added => a += 1,
            SymChange::Removed => r += 1,
            SymChange::Modified => m += 1,
        }
    }
    format!("symbols: +{a} -{r} ~{m}")
}
