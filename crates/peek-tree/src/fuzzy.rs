use nucleo::{Config, Matcher, Utf32Str};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzyMatch {
    pub path: PathBuf,
    pub relative_path: String,
    pub score: u32,
    pub indices: Vec<u32>,
}

pub struct FuzzySearcher {
    matcher: Matcher,
}

impl Default for FuzzySearcher {
    fn default() -> Self {
        Self::new()
    }
}

impl FuzzySearcher {
    pub fn new() -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT),
        }
    }

    /// Searches against a list of paths and returns sorted matches by score descending.
    pub fn search<P: AsRef<Path>>(
        &mut self,
        query: &str,
        paths: &[P],
        limit: usize,
    ) -> Vec<FuzzyMatch> {
        if query.trim().is_empty() {
            return paths
                .iter()
                .take(limit)
                .map(|p| {
                    let path_buf = p.as_ref().to_path_buf();
                    let relative = path_buf.to_string_lossy().to_string();
                    FuzzyMatch {
                        path: path_buf,
                        relative_path: relative,
                        score: 0,
                        indices: Vec::new(),
                    }
                })
                .collect();
        }

        let mut matches = Vec::new();
        let mut query_buf = Vec::new();
        let query_utf32 = Utf32Str::new(query, &mut query_buf);

        for p in paths {
            let path_ref = p.as_ref();
            let path_str = path_ref.to_string_lossy().to_string();
            let mut target_buf = Vec::new();
            let target_utf32 = Utf32Str::new(&path_str, &mut target_buf);

            let mut indices = Vec::new();
            if let Some(score) = self
                .matcher
                .fuzzy_indices(target_utf32, query_utf32, &mut indices)
            {
                matches.push(FuzzyMatch {
                    path: path_ref.to_path_buf(),
                    relative_path: path_str,
                    score: score as u32,
                    indices,
                });
            }
        }

        // Sort by score descending
        matches.sort_by(|a, b| b.score.cmp(&a.score));
        matches.truncate(limit);
        matches
    }

    /// Searches projects by name and path.
    pub fn search_projects(
        &mut self,
        query: &str,
        items: &[peek_core::ProjectItem],
        limit: usize,
    ) -> Vec<peek_core::ProjectItem> {
        if query.trim().is_empty() {
            return items.iter().take(limit).cloned().collect();
        }

        let mut matches = Vec::new();
        let mut query_buf = Vec::new();
        let query_utf32 = Utf32Str::new(query, &mut query_buf);

        for item in items {
            let search_target = format!("{} {}", item.name, item.path.to_string_lossy());
            let mut target_buf = Vec::new();
            let target_utf32 = Utf32Str::new(&search_target, &mut target_buf);

            if let Some(score) = self.matcher.fuzzy_match(target_utf32, query_utf32) {
                matches.push((item.clone(), score));
            }
        }

        matches.sort_by(|a, b| b.1.cmp(&a.1));
        matches
            .into_iter()
            .take(limit)
            .map(|(item, _)| item)
            .collect()
    }
}
