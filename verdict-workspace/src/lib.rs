//! verdict-workspace: IDE/editor-shaped workspace state for Verdict agents.
//! 
//! Provides WorkspaceState, OpenFile, Diff, Diagnostic, CursorPosition.
//! These compose into SessionState for long-lived coding agents.

use serde::{Serialize, Deserialize};
use std::path::PathBuf;
use std::collections::VecDeque;

/// Tracks the state of an editor-style workspace across agent turns.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceState {
    pub root: PathBuf,
    pub open_files: Vec<OpenFile>,
    pub recent_diffs: VecDeque<WorkspaceDiff>,
    pub diagnostics: Vec<WorkspaceDiagnostic>,
    pub cursor: Option<CursorPosition>,
}

/// A file open in the editor session (may have an in-memory dirty buffer)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenFile {
    pub path: PathBuf,
    /// In-memory buffer content; None means use disk content
    pub buffer: Option<String>,
    pub dirty: bool,
    pub language: Option<String>,
}

/// A unified diff recorded during a workspace session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceDiff {
    pub path: PathBuf,
    pub unified: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// A diagnostic message (compiler error, linter warning, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceDiagnostic {
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub file: Option<PathBuf>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub source: Option<String>, // e.g. "rustc", "clippy"
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

/// Editor cursor position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorPosition {
    pub file: PathBuf,
    pub line: u32,
    pub column: u32,
}

impl WorkspaceState {
    pub fn new(root: PathBuf) -> Self {
        WorkspaceState {
            root,
            open_files: vec![],
            recent_diffs: VecDeque::new(),
            diagnostics: vec![],
            cursor: None,
        }
    }

    /// Add or update an open file. If path already open, replaces it.
    pub fn open_file(&mut self, file: OpenFile) {
        if let Some(existing) = self.open_files.iter_mut().find(|f| f.path == file.path) {
            *existing = file;
        } else {
            self.open_files.push(file);
        }
    }

    /// Close a file by path
    pub fn close_file(&mut self, path: &PathBuf) {
        self.open_files.retain(|f| &f.path != path);
    }

    /// Record a diff, keeping only the last 20
    pub fn record_diff(&mut self, diff: WorkspaceDiff) {
        if self.recent_diffs.len() >= 20 {
            self.recent_diffs.pop_front();
        }
        self.recent_diffs.push_back(diff);
    }

    /// Replace all diagnostics
    pub fn set_diagnostics(&mut self, diagnostics: Vec<WorkspaceDiagnostic>) {
        self.diagnostics = diagnostics;
    }

    /// Count diagnostics by severity
    pub fn error_count(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.severity == DiagnosticSeverity::Error).count()
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.severity == DiagnosticSeverity::Warning).count()
    }

    /// Get a summary string suitable for injection into a prompt
    pub fn to_prompt_summary(&self) -> String {
        let mut parts = vec![];
        if !self.open_files.is_empty() {
            let names: Vec<_> = self.open_files.iter()
                .map(|f| f.path.display().to_string())
                .collect();
            parts.push(format!("Open files: {}", names.join(", ")));
        }
        if self.error_count() > 0 {
            parts.push(format!("{} compile error(s)", self.error_count()));
        }
        if self.warning_count() > 0 {
            parts.push(format!("{} warning(s)", self.warning_count()));
        }
        if let Some(cursor) = &self.cursor {
            parts.push(format!("Cursor at {}:{}", cursor.file.display(), cursor.line));
        }
        if parts.is_empty() {
            "No workspace context available.".to_string()
        } else {
            parts.join("\n")
        }
    }
}

impl OpenFile {
    pub fn new(path: PathBuf) -> Self {
        OpenFile { path, buffer: None, dirty: false, language: None }
    }

    pub fn with_buffer(mut self, content: String) -> Self {
        self.buffer = Some(content);
        self.dirty = true;
        self
    }

    pub fn with_language(mut self, lang: String) -> Self {
        self.language = Some(lang);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_open_close_file() {
        let mut ws = WorkspaceState::new(PathBuf::from("/project"));
        let file = OpenFile::new(PathBuf::from("/project/src/main.rs"));
        
        ws.open_file(file.clone());
        assert_eq!(ws.open_files.len(), 1);
        assert_eq!(ws.open_files[0].path, file.path);
        
        ws.close_file(&PathBuf::from("/project/src/main.rs"));
        assert_eq!(ws.open_files.len(), 0);
    }
    
    #[test]
    fn test_open_file_replaces_existing() {
        let mut ws = WorkspaceState::new(PathBuf::from("/project"));
        let path = PathBuf::from("/project/src/main.rs");
        
        let file1 = OpenFile::new(path.clone());
        ws.open_file(file1);
        assert_eq!(ws.open_files.len(), 1);
        
        let file2 = OpenFile::new(path.clone()).with_language("rust".to_string());
        ws.open_file(file2);
        assert_eq!(ws.open_files.len(), 1);
        assert_eq!(ws.open_files[0].language, Some("rust".to_string()));
    }
    
    #[test]
    fn test_diff_rolling_window() {
        let mut ws = WorkspaceState::new(PathBuf::from("/project"));
        
        // Add 25 diffs
        for i in 0..25 {
            let diff = WorkspaceDiff {
                path: PathBuf::from(format!("/project/file{}.rs", i)),
                unified: format!("diff {}", i),
                created_at: chrono::Utc::now(),
            };
            ws.record_diff(diff);
        }
        
        // Only the last 20 should be kept
        assert_eq!(ws.recent_diffs.len(), 20);
        // The first added diff (0) should be gone, oldest remaining is 5
        assert_eq!(ws.recent_diffs[0].unified, "diff 5");
        // The last added diff (24) should be present
        assert_eq!(ws.recent_diffs[19].unified, "diff 24");
    }
    
    #[test]
    fn test_prompt_summary() {
        let mut ws = WorkspaceState::new(PathBuf::from("/project"));
        
        // No context
        assert_eq!(ws.to_prompt_summary(), "No workspace context available.");
        
        // Add a file
        ws.open_file(OpenFile::new(PathBuf::from("/project/src/main.rs")));
        let summary = ws.to_prompt_summary();
        assert!(summary.contains("Open files:"));
        assert!(summary.contains("main.rs"));
        
        // Add an error
        ws.set_diagnostics(vec![
            WorkspaceDiagnostic {
                severity: DiagnosticSeverity::Error,
                message: "compile error".to_string(),
                file: None,
                line: None,
                column: None,
                source: None,
            }
        ]);
        let summary = ws.to_prompt_summary();
        assert!(summary.contains("1 compile error(s)"));
    }
    
    #[test]
    fn test_diagnostic_counts() {
        let mut ws = WorkspaceState::new(PathBuf::from("/project"));
        
        ws.set_diagnostics(vec![
            WorkspaceDiagnostic {
                severity: DiagnosticSeverity::Error,
                message: "error 1".to_string(),
                file: None,
                line: None,
                column: None,
                source: None,
            },
            WorkspaceDiagnostic {
                severity: DiagnosticSeverity::Error,
                message: "error 2".to_string(),
                file: None,
                line: None,
                column: None,
                source: None,
            },
            WorkspaceDiagnostic {
                severity: DiagnosticSeverity::Warning,
                message: "warning 1".to_string(),
                file: None,
                line: None,
                column: None,
                source: None,
            },
        ]);
        
        assert_eq!(ws.error_count(), 2);
        assert_eq!(ws.warning_count(), 1);
    }
    
    #[test]
    fn test_cursor_position() {
        let mut ws = WorkspaceState::new(PathBuf::from("/project"));
        let cursor = CursorPosition {
            file: PathBuf::from("/project/src/lib.rs"),
            line: 42,
            column: 15,
        };
        
        ws.cursor = Some(cursor.clone());
        let summary = ws.to_prompt_summary();
        assert!(summary.contains("Cursor at"));
        assert!(summary.contains("lib.rs"));
        assert!(summary.contains("42"));
    }
    
    #[test]
    fn test_open_file_with_buffer() {
        let path = PathBuf::from("/project/src/main.rs");
        let content = "fn main() {}".to_string();
        
        let file = OpenFile::new(path).with_buffer(content.clone()).with_language("rust".to_string());
        
        assert!(file.dirty);
        assert_eq!(file.buffer, Some(content));
        assert_eq!(file.language, Some("rust".to_string()));
    }
}
