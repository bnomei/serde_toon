#[cfg(feature = "parallel")]
mod parallel_tests {
    use serde_json::json;

    #[test]
    fn from_str_parallel_handles_tabular_array() {
        let mut input = String::from("[64]{id}:\n");
        for i in 1..=64 {
            if i == 64 {
                input.push_str(&format!("  {i}"));
            } else {
                input.push_str(&format!("  {i}\n"));
            }
        }

        let values =
            serde_toon::from_str_parallel::<serde_json::Value>(&input).expect("from_str_parallel");
        assert_eq!(values.len(), 64);
        assert_eq!(values.first(), Some(&json!({"id": 1})));
        assert_eq!(values.last(), Some(&json!({"id": 64})));
    }
}
