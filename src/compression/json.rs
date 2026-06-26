/// JSON compression and minification.

use crate::compression::helpers::safe_truncate;

pub fn compress_json(raw_json: &str, threshold: usize) -> anyhow::Result<String> {
    let value: serde_json::Value = serde_json::from_str(raw_json)?;
    if let serde_json::Value::Array(arr) = value {
        if arr.is_empty() {
            return Ok("[]".to_string());
        }
        let total_count = arr.len();
        let mut keys = std::collections::BTreeSet::new();
        for item in &arr {
            if let serde_json::Value::Object(map) = item {
                for k in map.keys() {
                    keys.insert(k.clone());
                }
            }
        }

        let keys_str = keys.into_iter().collect::<Vec<String>>().join(", ");
        let first_item_str = serde_json::to_string_pretty(&arr[0]).unwrap_or_default();

        Ok(format!(
            "[CCR Summary: Array of {} objects. Keys: [{}]. \nFirst element:\n{}]",
            total_count, keys_str, first_item_str
        ))
    } else {
        let minified = serde_json::to_string(&value)?;
        if minified.char_indices().nth(threshold).is_some() {
            Ok(format!("{}...", safe_truncate(&minified, threshold)))
        } else {
            Ok(minified)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_json() {
        // Empty array
        assert_eq!(compress_json("[]", 10).unwrap(), "[]");
        
        // Single object inside array
        let single_obj = r#"[{"name": "test", "id": 1}]"#;
        let res = compress_json(single_obj, 10).unwrap();
        assert!(res.contains("Array of 1 objects"));
        assert!(res.contains("id, name"));
        
        // Non-array minified JSON
        let non_arr = r#"{"name": "hello", "nested": {"val": 123}}"#;
        let res_non_arr = compress_json(non_arr, 100).unwrap();
        assert_eq!(res_non_arr, "{\"name\":\"hello\",\"nested\":{\"val\":123}}");
        
        // Non-array JSON truncation
        let res_trunc = compress_json(non_arr, 15).unwrap();
        assert_eq!(res_trunc, "{\"name\":\"hello\"...");
    }
}
