/// CTC decode: blank=0, characters start at index 1
/// Collapse repeats, remove blank, map indices to character strings
pub fn ctc_decode(probs: &[Vec<f32>], keys: &[String]) -> String {
    let blank_idx = 0;
    let mut result = String::new();
    let mut prev_char_idx = blank_idx;

    for timestep in probs {
        let max_idx = timestep
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(idx, _)| idx)
            .unwrap_or(blank_idx);

        if max_idx != blank_idx && max_idx != prev_char_idx {
            let keys_idx = max_idx - 1;
            if keys_idx < keys.len() {
                result.push_str(&keys[keys_idx]);
            }
        }
        prev_char_idx = max_idx;
    }

    result
}
