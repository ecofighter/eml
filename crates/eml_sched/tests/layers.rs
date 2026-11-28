//! `eml_sched`のmoduleのimportのうち、意味のある辺 (docs/spec/runtime.mdの「範囲」)。
use std::collections::{BTreeMap, BTreeSet};

use crate::scan::{NEW, Source, closing, sources};

/// ファイルが`crate::`を通して名指すcrateのmodule。自分は除く。
fn imports(source: &Source) -> BTreeSet<String> {
    let module = |path: &str| {
        path.trim_end_matches(".rs")
            .split('/')
            .next()
            .unwrap()
            .to_string()
    };
    let own = module(&source.path);
    let code = &source.code;
    let mut out = BTreeSet::new();
    for (at, pattern) in code.match_indices("crate::") {
        let rest = &code[at + pattern.len()..];
        if rest.starts_with('{') {
            let group = &rest[1..closing(rest) - 1];
            let mut depth = 0;
            let mut start = 0;
            for (k, c) in group.char_indices().chain([(group.len(), ',')]) {
                match c {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    ',' if depth == 0 => {
                        let name: String = group[start..k]
                            .chars()
                            .take_while(|&c| crate::scan::is_ident(c))
                            .collect();
                        if !name.is_empty() {
                            out.insert(name);
                        }
                        start = k + 1;
                    }
                    _ => {}
                }
            }
        } else {
            let name: String = rest
                .chars()
                .take_while(|&c| crate::scan::is_ident(c))
                .collect();
            out.insert(name);
        }
    }
    out.remove(&own);
    out
}

#[test]
fn the_modules_import_downwards() {
    let sources = sources("eml_sched");
    let module = |path: &str| {
        path.trim_end_matches(".rs")
            .split('/')
            .next()
            .unwrap()
            .to_string()
    };
    let new: BTreeSet<String> = NEW.iter().map(|path| module(path)).collect();
    let graph: BTreeMap<String, BTreeSet<String>> = sources
        .iter()
        .filter(|source| source.path != "lib.rs")
        .map(|source| (module(&source.path), imports(source)))
        .fold(BTreeMap::new(), |mut graph, (name, edges)| {
            graph.entry(name).or_default().extend(edges);
            graph
        });
    assert!(graph["sequence"].contains("drive"));
    for old in [
        "cells", "config", "error", "park", "doorbell", "driver", "counters",
    ] {
        let named: Vec<&String> = graph
            .get(old)
            .into_iter()
            .flatten()
            .filter(|m| new.contains(*m))
            .collect();
        assert!(named.is_empty(), "{old} imports the new files {named:?}");
    }
    let rank: Vec<&String> = graph
        .get("rank")
        .into_iter()
        .flatten()
        .filter(|m| *m != "cells")
        .collect();
    assert!(rank.is_empty(), "rank imports {rank:?}");
    for (name, edges) in &graph {
        for top in ["sequence", "script"] {
            assert!(!edges.contains(top), "{name} imports {top}");
        }
    }
    assert!(
        !graph
            .get("exec")
            .is_some_and(|edges| edges.contains("drive")),
        "exec imports drive"
    );
    // スケジューラのファイルのimportの図に輪はない。残りのファイルを何もimportしないファイルを、なくなるまで除いてい
    // く
    let mut left: BTreeMap<&String, BTreeSet<&String>> = graph
        .iter()
        .filter(|(name, _)| new.contains(*name))
        .map(|(name, edges)| {
            (
                name,
                edges
                    .iter()
                    .filter(|m| new.contains(*m) && graph.contains_key(*m))
                    .collect(),
            )
        })
        .collect();
    while !left.is_empty() {
        let leaves: Vec<&String> = left
            .iter()
            .filter(|(_, edges)| edges.is_empty())
            .map(|(name, _)| *name)
            .collect();
        assert!(
            !leaves.is_empty(),
            "the new files import in a cycle: {left:?}"
        );
        for leaf in leaves {
            left.remove(leaf);
            for edges in left.values_mut() {
                edges.remove(leaf);
            }
        }
    }
}
