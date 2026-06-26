/// CSV compression/tabular formatting.

pub fn compress_csv(raw_csv: &str) -> String {
    let mut lines = raw_csv.lines();
    let mut result = String::new();
    if let Some(header) = lines.next() {
        result.push_str(&format!("Headers: {}\n", header));
    }
    let mut count = 0;
    for line in lines.by_ref() {
        if count < 3 {
            result.push_str(&format!("Row {}: {}\n", count + 1, line));
        }
        count += 1;
    }
    result.push_str(&format!("[CCR Summary: CSV contains {} rows total]", count + 1));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_csv() {
        let csv = "id,name,age\n1,alice,30\n2,bob,25\n3,charlie,35\n4,david,40";
        let res = compress_csv(csv);
        assert!(res.contains("Headers: id,name,age"));
        assert!(res.contains("Row 1: 1,alice,30"));
        assert!(res.contains("CSV contains 5 rows total"));
    }
}
