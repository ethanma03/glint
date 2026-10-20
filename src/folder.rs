//! Sibling-file listing in Explorer-like natural order.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use crate::decode::is_image_path;

/// Images in the same folder as `path`, naturally sorted. `path` itself is
/// always included, even with an unusual extension.
pub fn siblings(path: &Path) -> Vec<PathBuf> {
    let Some(dir) = path.parent() else { return vec![path.to_path_buf()] };
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .map(|e| e.path())
                .filter(|p| is_image_path(p) || p == path)
                .collect()
        })
        .unwrap_or_default();
    if !files.iter().any(|p| p == path) {
        files.push(path.to_path_buf());
    }
    files.sort_by(|a, b| natural_cmp(&file_name(a), &file_name(b)));
    files
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// "frame2" < "frame10", case-insensitive.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    loop {
        match (a.first(), b.first()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let da = a.iter().take_while(|c| c.is_ascii_digit()).count();
                let db = b.iter().take_while(|c| c.is_ascii_digit()).count();
                let (na, nb) = (trim_zeros(&a[..da]), trim_zeros(&b[..db]));
                let ord = na.len().cmp(&nb.len()).then_with(|| na.cmp(nb)).then_with(|| da.cmp(&db));
                if ord != Ordering::Equal {
                    return ord;
                }
                a = &a[da..];
                b = &b[db..];
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(y);
                }
                a = &a[1..];
                b = &b[1..];
            }
        }
    }
}

fn trim_zeros(d: &[u8]) -> &[u8] {
    let n = d.iter().take_while(|&&c| c == b'0').count();
    &d[n.min(d.len().saturating_sub(1))..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut v = vec!["frame10.png", "frame2.png", "Frame1.png", "frame02b.png", "a.png"];
        v.sort_by(|a, b| natural_cmp(&a.to_lowercase(), &b.to_lowercase()));
        assert_eq!(v, ["a.png", "Frame1.png", "frame2.png", "frame02b.png", "frame10.png"]);
    }
}
