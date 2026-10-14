pub mod flat_tree;
pub mod fuzzy;
pub mod icons;
pub mod walker;

pub use flat_tree::{FlatNode, FlatTree};
pub use fuzzy::{FuzzyMatch, FuzzySearcher};
pub use icons::icon_for_path;
pub use walker::walk_project_files;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Instant;

    #[test]
    fn test_icons() {
        assert_eq!(
            icon_for_path(&PathBuf::from("main.rs"), false, false),
            "\u{ebe6}"
        );
        assert_eq!(
            icon_for_path(&PathBuf::from("app.tsx"), false, false),
            "\u{ebbc}"
        );
        assert_eq!(
            icon_for_path(&PathBuf::from("script.py"), false, false),
            "\u{eb9c}"
        );
        assert_eq!(
            icon_for_path(&PathBuf::from("src"), true, false),
            icons::codicons::FOLDER
        );
        assert_eq!(
            icon_for_path(&PathBuf::from("src"), true, true),
            icons::codicons::FOLDER_OPENED
        );
    }

    #[test]
    fn test_fuzzy_matcher_ranking() {
        let mut searcher = FuzzySearcher::new();
        let paths = vec![
            PathBuf::from("src/core/buffer.rs"),
            PathBuf::from("src/main.rs"),
            PathBuf::from("crates/peek-tree/src/fuzzy.rs"),
            PathBuf::from("README.md"),
        ];

        let results = searcher.search("srcmain", &paths, 10);
        assert!(!results.is_empty());
        assert_eq!(results[0].path, PathBuf::from("src/main.rs"));
        assert!(!results[0].indices.is_empty());
    }

    #[test]
    fn test_fuzzy_search_10k_performance() {
        let mut searcher = FuzzySearcher::new();
        let mut paths = Vec::with_capacity(10_000);
        for i in 0..10_000 {
            paths.push(PathBuf::from(format!(
                "crates/module_{}/src/component_{}.rs",
                i % 50,
                i
            )));
        }

        let start = Instant::now();
        let results = searcher.search("mod12comp34", &paths, 20);
        let elapsed = start.elapsed();

        assert!(!results.is_empty());
        // Nucleo SIMD matching 10,000 paths should comfortably complete under 10ms
        assert!(elapsed.as_millis() < 50, "Fuzzy search took {:?}", elapsed);
    }

    #[test]
    fn test_flat_tree_toggle() {
        let mut tree = FlatTree::new(".");
        let test_path = PathBuf::from("test_dir");
        assert!(!tree.is_expanded(&test_path));

        let expanded = tree.toggle_expand(&test_path);
        assert!(expanded);
        assert!(tree.is_expanded(&test_path));

        let collapsed = tree.toggle_expand(&test_path);
        assert!(!collapsed);
        assert!(!tree.is_expanded(&test_path));
    }

    #[test]
    fn hidden_config_is_visible_without_build_folders() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for folder in [".git", ".github", "nested", "node_modules", "target"] {
            std::fs::create_dir(root.join(folder)).unwrap();
        }
        for file in [
            ".env",
            ".env.local",
            ".github/config.yml",
            "nested/.env.production",
            "node_modules/.env",
            "target/output.rs",
            "ignored.txt",
        ] {
            std::fs::write(root.join(file), "example").unwrap();
        }
        std::fs::write(root.join(".gitignore"), ".env*\nignored.txt\n").unwrap();
        let files = walk_project_files(root);
        assert!(files.contains(&root.join(".env")));
        assert!(files.contains(&root.join(".env.local")));
        assert!(files.contains(&root.join("nested/.env.production")));
        assert!(files.contains(&root.join(".github/config.yml")));
        assert!(!files.contains(&root.join("node_modules/.env")));
        assert!(!files.contains(&root.join("ignored.txt")));
        let nodes = FlatTree::new(root).visible_nodes();
        assert!(nodes.iter().any(|n| n.name == ".env"));
        assert!(nodes.iter().any(|n| n.name == ".github"));
        assert!(!nodes.iter().any(|n| n.name == "node_modules"));
        assert!(!nodes.iter().any(|n| n.name == ".git"));
        assert!(walker::walk_project_files_cancellable(root, || true).is_none());
    }
}
