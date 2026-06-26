/// Unified diff parsing and compression.

#[derive(Debug, Clone, PartialEq)]
pub struct DiffFile {
    pub path: String,
    pub insertions: usize,
    pub deletions: usize,
    pub hunks_count: usize,
    pub is_binary: bool,
    pub is_new: bool,
    pub is_deleted: bool,
    pub contexts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiffSummary {
    pub files: Vec<DiffFile>,
    pub total_insertions: usize,
    pub total_deletions: usize,
}

fn clean_path(p: &str) -> String {
    let p = p.split('\t').next().unwrap_or(p).trim();
    // Strip standard prefixes like a/, b/, etc.
    if (p.starts_with("a/") || p.starts_with("b/")) && p.len() > 2 {
        p[2..].to_string()
    } else {
        p.to_string()
    }
}

fn commit_current_file(files: &mut Vec<DiffFile>, current: &mut Option<DiffFile>) {
    if let Some(f) = current.take() {
        if !f.path.is_empty() && f.path != "/dev/null" {
            files.push(f);
        }
    }
}

pub fn parse_unified_diff(text: &str) -> DiffSummary {
    let mut files = Vec::new();
    let mut current_file: Option<DiffFile> = None;

    for line in text.lines() {
        if line.starts_with("diff --git ") {
            commit_current_file(&mut files, &mut current_file);
        } else if line.starts_with("--- ") {
            let path_part = &line[4..];
            let cleaned = clean_path(path_part);
            if cleaned == "/dev/null" {
                if let Some(ref mut f) = current_file {
                    f.is_new = true;
                } else {
                    current_file = Some(DiffFile {
                        path: "/dev/null".to_string(),
                        insertions: 0,
                        deletions: 0,
                        hunks_count: 0,
                        is_binary: false,
                        is_new: true,
                        is_deleted: false,
                        contexts: Vec::new(),
                    });
                }
            } else {
                if current_file.is_none() {
                    current_file = Some(DiffFile {
                        path: cleaned,
                        insertions: 0,
                        deletions: 0,
                        hunks_count: 0,
                        is_binary: false,
                        is_new: false,
                        is_deleted: false,
                        contexts: Vec::new(),
                    });
                } else {
                    let mut f = current_file.take().unwrap();
                    if f.path == "/dev/null" {
                        f.path = cleaned;
                    } else if f.path != cleaned {
                        files.push(f);
                        current_file = Some(DiffFile {
                            path: cleaned,
                            insertions: 0,
                            deletions: 0,
                            hunks_count: 0,
                            is_binary: false,
                            is_new: false,
                            is_deleted: false,
                            contexts: Vec::new(),
                        });
                        continue;
                    }
                    current_file = Some(f);
                }
            }
        } else if line.starts_with("+++ ") {
            let path_part = &line[4..];
            let cleaned = clean_path(path_part);
            if cleaned == "/dev/null" {
                if let Some(ref mut f) = current_file {
                    f.is_deleted = true;
                } else {
                    current_file = Some(DiffFile {
                        path: "/dev/null".to_string(),
                        insertions: 0,
                        deletions: 0,
                        hunks_count: 0,
                        is_binary: false,
                        is_new: false,
                        is_deleted: true,
                        contexts: Vec::new(),
                    });
                }
            } else {
                if let Some(ref mut f) = current_file {
                    if f.path == "/dev/null" {
                        f.path = cleaned;
                    }
                } else {
                    current_file = Some(DiffFile {
                        path: cleaned,
                        insertions: 0,
                        deletions: 0,
                        hunks_count: 0,
                        is_binary: false,
                        is_new: false,
                        is_deleted: false,
                        contexts: Vec::new(),
                    });
                }
            }
        } else if line.starts_with("@@") {
            if let Some(pos) = line.rfind("@@") {
                let context = line[pos + 2..].trim();
                if let Some(ref mut f) = current_file {
                    f.hunks_count += 1;
                    if !context.is_empty() {
                        if !f.contexts.contains(&context.to_string()) {
                            f.contexts.push(context.to_string());
                        }
                    }
                }
            }
        } else if line.starts_with("Binary files ") && line.contains(" differ") {
            commit_current_file(&mut files, &mut current_file);
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 5 && parts[1] == "files" && parts[3] == "and" {
                let path = clean_path(parts[2]);
                current_file = Some(DiffFile {
                    path,
                    insertions: 0,
                    deletions: 0,
                    hunks_count: 0,
                    is_binary: true,
                    is_new: false,
                    is_deleted: false,
                    contexts: Vec::new(),
                });
                commit_current_file(&mut files, &mut current_file);
            }
        } else if line.starts_with('+') {
            if let Some(ref mut f) = current_file {
                if !f.is_binary {
                    f.insertions += 1;
                }
            }
        } else if line.starts_with('-') {
            if let Some(ref mut f) = current_file {
                if !f.is_binary {
                    f.deletions += 1;
                }
            }
        }
    }

    commit_current_file(&mut files, &mut current_file);

    let mut total_insertions = 0;
    let mut total_deletions = 0;
    for f in &files {
        total_insertions += f.insertions;
        total_deletions += f.deletions;
    }

    DiffSummary {
        files,
        total_insertions,
        total_deletions,
    }
}

pub fn compress_diff(diff_text: &str) -> String {
    let summary = parse_unified_diff(diff_text);
    if summary.files.is_empty() {
        return "No files changed in diff.".to_string();
    }

    let files_count = summary.files.len();
    let mut output = format!(
        "Diff Summary: {} file{} changed, {} insertion{}(+), {} deletion{}(-)\n\nModified files:",
        files_count,
        if files_count == 1 { "" } else { "s" },
        summary.total_insertions,
        if summary.total_insertions == 1 { "" } else { "s" },
        summary.total_deletions,
        if summary.total_deletions == 1 { "" } else { "s" }
    );

    for f in summary.files {
        output.push_str("\n- ");
        output.push_str(&f.path);
        output.push_str(": ");
        if f.is_binary {
            output.push_str("binary file changed");
        } else {
            output.push_str(&format!("+{}/-{}", f.insertions, f.deletions));
            if f.hunks_count > 0 {
                output.push_str(&format!(" ({} hunk{})", f.hunks_count, if f.hunks_count == 1 { "" } else { "s" }));
            }
            if !f.contexts.is_empty() {
                output.push_str(" (modified: ");
                output.push_str(&f.contexts.join(", "));
                output.push(')');
            }
        }
        if f.is_new {
            output.push_str(" [NEW]");
        } else if f.is_deleted {
            output.push_str(" [DELETED]");
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_git_diff() {
        let diff = r#"diff --git a/src/server.rs b/src/server.rs
index e69de29..d2345ef 100644
--- a/src/server.rs
+++ b/src/server.rs
@@ -10,3 +10,4 @@
 line1
 line2
-old_line
+new_line
"#;
        let summary = parse_unified_diff(diff);
        assert_eq!(summary.files.len(), 1);
        assert_eq!(summary.files[0].path, "src/server.rs");
        assert_eq!(summary.files[0].insertions, 1);
        assert_eq!(summary.files[0].deletions, 1);
        assert_eq!(summary.files[0].hunks_count, 1);
        assert_eq!(summary.files[0].is_binary, false);
        assert_eq!(summary.files[0].is_new, false);
        assert_eq!(summary.files[0].is_deleted, false);
    }

    #[test]
    fn test_parse_multi_file_diff() {
        let diff = r#"diff --git a/src/server.rs b/src/server.rs
--- a/src/server.rs
+++ b/src/server.rs
@@ -10,3 +10,4 @@
-old
+new
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,5 +1,6 @@
+added_at_start
"#;
        let summary = parse_unified_diff(diff);
        assert_eq!(summary.files.len(), 2);
        assert_eq!(summary.files[0].path, "src/server.rs");
        assert_eq!(summary.files[0].insertions, 1);
        assert_eq!(summary.files[0].deletions, 1);
        assert_eq!(summary.files[1].path, "src/main.rs");
        assert_eq!(summary.files[1].insertions, 1);
        assert_eq!(summary.files[1].deletions, 0);
        assert_eq!(summary.total_insertions, 2);
        assert_eq!(summary.total_deletions, 1);
    }

    #[test]
    fn test_binary_and_new_deleted() {
        let diff = r#"diff --git a/assets/image.png b/assets/image.png
Binary files a/assets/image.png and b/assets/image.png differ
diff --git a/new_file.txt b/new_file.txt
new file mode 100644
--- /dev/null
+++ b/new_file.txt
@@ -0,0 +1,1 @@
+hello
diff --git a/old_file.txt b/old_file.txt
deleted file mode 100644
--- a/old_file.txt
+++ /dev/null
@@ -1,1 +0,0 @@
-goodbye
"#;
        let summary = parse_unified_diff(diff);
        assert_eq!(summary.files.len(), 3);
        
        assert_eq!(summary.files[0].path, "assets/image.png");
        assert_eq!(summary.files[0].is_binary, true);
        
        assert_eq!(summary.files[1].path, "new_file.txt");
        assert_eq!(summary.files[1].is_new, true);
        assert_eq!(summary.files[1].insertions, 1);
        
        assert_eq!(summary.files[2].path, "old_file.txt");
        assert_eq!(summary.files[2].is_deleted, true);
        assert_eq!(summary.files[2].deletions, 1);
    }

    #[test]
    fn test_compress_diff_formatting() {
        let diff = r#"diff --git a/src/server.rs b/src/server.rs
--- a/src/server.rs
+++ b/src/server.rs
@@ -10,3 +10,4 @@ fn my_func()
-old
+new
"#;
        let compressed = compress_diff(diff);
        assert!(compressed.contains("Diff Summary: 1 file changed, 1 insertion(+), 1 deletion(-)"));
        assert!(compressed.contains("- src/server.rs: +1/-1 (1 hunk) (modified: fn my_func())"));
    }
}
