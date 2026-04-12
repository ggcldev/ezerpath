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

#[cfg(test)]
mod tests {
    use super::cosine_similarity;

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
}
