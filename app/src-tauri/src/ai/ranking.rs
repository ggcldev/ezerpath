use std::collections::HashSet;

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a.sqrt() * norm_b.sqrt())
}

/// Rank `(job_id, vector)` pairs against `query_vec` by cosine similarity,
/// skipping anything in `exclude` or below `floor`. Returns the top `limit`
/// job IDs in descending order of similarity. Pulled out of the search route
/// so it can be unit-tested without touching the HTTP embedding service.
pub fn rank_embeddings_against_query(
    query_vec: &[f32],
    candidates: impl IntoIterator<Item = (i64, Vec<f32>)>,
    exclude: &HashSet<i64>,
    floor: f32,
    limit: usize,
) -> Vec<i64> {
    if limit == 0 {
        return Vec::new();
    }
    let mut scored: Vec<(f32, i64)> = candidates
        .into_iter()
        .filter(|(id, _)| !exclude.contains(id))
        .filter_map(|(id, vec)| {
            let sim = cosine_similarity(query_vec, &vec);
            if sim < floor {
                None
            } else {
                Some((sim, id))
            }
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);
    scored.into_iter().map(|(_, id)| id).collect()
}

#[cfg(test)]
mod tests {
    use super::{cosine_similarity, rank_embeddings_against_query};
    use std::collections::HashSet;

    #[test]
    fn cosine_similarity_handles_happy_path() {
        let a = vec![1.0, 0.0, 1.0];
        let b = vec![1.0, 0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim > 0.99);
    }

    #[test]
    fn cosine_similarity_handles_mismatch_len() {
        let a = vec![1.0, 0.0];
        let b = vec![1.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn rank_embeddings_orders_by_similarity_respects_exclude_and_floor() {
        let query = vec![1.0, 0.0, 0.0];
        let candidates = vec![
            (10i64, vec![1.0, 0.0, 0.0]),  // sim 1.00 — best
            (11i64, vec![0.9, 0.1, 0.0]),  // sim ~0.99
            (12i64, vec![0.0, 1.0, 0.0]),  // sim 0.00 — below floor
            (13i64, vec![0.5, 0.5, 0.0]),  // sim ~0.71
            (14i64, vec![1.0, 0.0, 0.0]),  // sim 1.00 but excluded
        ];
        let mut exclude = HashSet::new();
        exclude.insert(14i64);
        let top = rank_embeddings_against_query(&query, candidates, &exclude, 0.30, 3);
        assert_eq!(top, vec![10, 11, 13]);
    }

    #[test]
    fn rank_embeddings_returns_empty_on_zero_limit() {
        let query = vec![1.0, 0.0];
        let candidates = vec![(1i64, vec![1.0, 0.0])];
        let out = rank_embeddings_against_query(&query, candidates, &HashSet::new(), 0.0, 0);
        assert!(out.is_empty());
    }
}
