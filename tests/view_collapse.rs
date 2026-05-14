use diffloom::view::{self, SbsRow};

#[test]
fn focused_diff_folds_long_equal_runs() {
    let old: String = (0..40).map(|i| format!("line {i}\n")).collect();
    let new = old.clone();

    let (full, _) = view::side_by_side_rows(&old, &new);
    let (folded, _) = view::side_by_side_rows_focused(&old, &new, 2);
    assert!(
        full.len() > folded.len(),
        "expected fewer rows when folding, full={} folded={}",
        full.len(),
        folded.len()
    );
    assert!(
        folded.iter().any(|r| matches!(r, SbsRow::Skipped { .. })),
        "expected a Skipped row"
    );
}

#[test]
fn focused_diff_folds_between_two_hunks() {
    let old: String = (0..25).map(|i| format!("a{i}\n")).collect();
    let mut new = String::new();
    for i in 0..25 {
        if i == 5 {
            new.push_str("X\n");
        } else if i == 18 {
            new.push_str("Y\n");
        } else {
            new.push_str(&format!("a{i}\n"));
        }
    }
    let (folded, _) = view::side_by_side_rows_focused(&old, &new, 2);
    assert!(
        folded.iter().any(|r| matches!(r, SbsRow::Skipped { .. })),
        "expected fold between hunks"
    );
}
