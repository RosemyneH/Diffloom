use diffloom::db::SymbolRow;
use diffloom::rust_parse::{
    diff_symbol_maps, parse_rust_symbols, summarize_changes, SymChange, SymbolRec,
};

#[test]
fn parse_rust_symbols_extracts_items() {
    let src = r#"
pub struct Alpha { x: u32 }
impl Alpha {
    pub fn bump(&mut self) { self.x += 1; }
}
pub trait Marker {}
enum E { A, B }
"#;
    let (syms, err) = parse_rust_symbols(src).unwrap();
    assert!(err.is_none(), "unexpected parse error: {err:?}");
    let kinds: Vec<_> = syms.iter().map(|s| s.kind.as_str()).collect();
    assert!(kinds.contains(&"struct"));
    assert!(kinds.contains(&"impl"));
    assert!(kinds.contains(&"trait"));
    assert!(kinds.contains(&"enum"));
    assert!(syms.iter().any(|s| s.name == "Alpha" && s.kind == "struct"));
    assert!(syms.iter().any(|s| s.kind == "fn" && s.name == "bump"));
}

#[test]
fn parse_rust_symbols_marks_error_nodes() {
    let src = "fn broken( {";
    let (_syms, err) = parse_rust_symbols(src).unwrap();
    assert_eq!(err.as_deref(), Some("tree contains errors"));
}

#[test]
fn diff_symbol_maps_added_removed_modified() {
    let prev = vec![
        SymbolRow {
            kind: "fn".into(),
            name: "old".into(),
            fq_name: Some("old".into()),
            start_byte: 0,
            end_byte: 2,
        },
        SymbolRow {
            kind: "fn".into(),
            name: "gone".into(),
            fq_name: Some("gone".into()),
            start_byte: 5,
            end_byte: 6,
        },
    ];
    let cur = vec![
        SymbolRec {
            kind: "fn".into(),
            name: "old".into(),
            fq_name: "old".into(),
            start_byte: 10,
            end_byte: 12,
        },
        SymbolRec {
            kind: "fn".into(),
            name: "new_fn".into(),
            fq_name: "new_fn".into(),
            start_byte: 20,
            end_byte: 25,
        },
    ];
    let d = diff_symbol_maps(&prev, &cur);
    assert!(d.contains(&(SymChange::Added, "new_fn".into(), "fn".into())));
    assert!(d.contains(&(SymChange::Removed, "gone".into(), "fn".into())));
    assert!(d.contains(&(SymChange::Modified, "old".into(), "fn".into())));
}

#[test]
fn diff_symbol_maps_fq_name_fallback_on_prev() {
    let prev = vec![SymbolRow {
        kind: "struct".into(),
        name: "S".into(),
        fq_name: None,
        start_byte: 0,
        end_byte: 1,
    }];
    let cur = vec![SymbolRec {
        kind: "struct".into(),
        name: "S".into(),
        fq_name: "S".into(),
        start_byte: 0,
        end_byte: 1,
    }];
    let d = diff_symbol_maps(&prev, &cur);
    assert!(d
        .iter()
        .any(|(c, n, _)| *c == SymChange::Removed && n == "S"));
    assert!(d.iter().any(|(c, n, _)| *c == SymChange::Added && n == "S"));
}

#[test]
fn summarize_changes_counts() {
    let v = vec![
        (SymChange::Added, "a".into(), "fn".into()),
        (SymChange::Added, "b".into(), "fn".into()),
        (SymChange::Removed, "c".into(), "fn".into()),
        (SymChange::Modified, "d".into(), "struct".into()),
    ];
    assert_eq!(summarize_changes(&v), "symbols: +2 -1 ~1");
}
