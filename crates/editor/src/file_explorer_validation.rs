use anyhow::{Result, anyhow};

/// A type-safe representation of a file path in the file explorer
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FilePath(String);

impl FilePath {
    pub fn new(path: String) -> Result<Self> {
        if path.is_empty() {
            return Err(anyhow!("Path cannot be empty"));
        }

        // Validate path for macOS-specific restrictions
        if Self::has_invalid_macos_characters(&path) {
            return Err(anyhow!("Path contains invalid characters: {}", path));
        }

        // Check for path traversal attempts
        if path.contains("..") {
            return Err(anyhow!("Path traversal is not allowed: {}", path));
        }

        Ok(FilePath(path))
    }

    fn has_invalid_macos_characters(path: &str) -> bool {
        path.chars().any(|c| matches!(c, ':' | '\0'))
    }
}

/// Represents a validated file explorer operation
#[derive(Debug)]
pub struct ValidatedFileOperation {
    pub old_name: String,
    pub new_name: String,
    pub operation_type: FileOperationType,
}

#[derive(Debug)]
pub enum FileOperationType {
    Rename,
    Delete,
}

/// Represents validation errors for file operations
#[derive(Debug)]
pub enum ValidationError {
    CountMismatch { expected: usize, actual: usize },
    InvalidCharacter { line: usize, character: char },
    PathTraversalAttempt { path: String },

    TooLong { line: usize, length: usize },
    DuplicateName { line: usize, name: String },
}

impl ValidationError {
    pub fn to_user_friendly_message(&self) -> String {
        match self {
            ValidationError::CountMismatch { expected, actual } => {
                format!(
                    "Number of files has changed from {} to {}. This indicates external changes to the directory.",
                    expected, actual
                )
            }
            ValidationError::InvalidCharacter { line, character } => {
                format!(
                    "Invalid character '{}' on line {} (':' and null characters are not allowed on macOS)",
                    character,
                    line + 1
                )
            }
            ValidationError::PathTraversalAttempt { path } => {
                format!(
                    "Path traversal attempt detected in '{}'. '..' segments are not allowed.",
                    path
                )
            }

            ValidationError::TooLong { line, length } => {
                format!(
                    "Filename on line {} is too long ({} characters). macOS limits filenames to ~255 characters.",
                    line + 1,
                    length
                )
            }
            ValidationError::DuplicateName { line, name } => {
                format!(
                    "Duplicate filename '{}' on line {} conflicts with another entry.",
                    name,
                    line + 1
                )
            }
        }
    }
}

/// Validates file operations according to macOS constraints
pub fn validate_file_operations(
    current_entries: &[String],
    stored_state: &[String],
) -> Result<Vec<ValidatedFileOperation>, ValidationError> {
    if current_entries.len() != stored_state.len() {
        return Err(ValidationError::CountMismatch {
            expected: stored_state.len(),
            actual: current_entries.len(),
        });
    }

    let mut operations = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    for (idx, (current, original)) in current_entries.iter().zip(stored_state.iter()).enumerate() {
        let trimmed_current = current.trim();
        let trimmed_original = original.trim();

        // Check for empty names (deletion)
        if trimmed_current.is_empty() {
            if !seen_names.insert(trimmed_original.to_string()) {
                return Err(ValidationError::DuplicateName {
                    line: idx,
                    name: trimmed_original.to_string(),
                });
            }
            operations.push(ValidatedFileOperation {
                old_name: trimmed_original.to_string(),
                new_name: String::new(),
                operation_type: FileOperationType::Delete,
            });
        } else {
            // Check for invalid characters
            if has_invalid_filename_chars(trimmed_current) {
                if let Some(invalid_char) =
                    trimmed_current.chars().find(|&c| matches!(c, ':' | '\0'))
                {
                    return Err(ValidationError::InvalidCharacter {
                        line: idx,
                        character: invalid_char,
                    });
                }
            }

            // Check for duplicates
            let clean_name = trimmed_current.trim_end_matches('/');
            if !seen_names.insert(clean_name.to_string()) {
                return Err(ValidationError::DuplicateName {
                    line: idx,
                    name: clean_name.to_string(),
                });
            }

            // Check length
            if clean_name.len() > 255 {
                return Err(ValidationError::TooLong {
                    line: idx,
                    length: clean_name.len(),
                });
            }

            // Check for path traversal
            if trimmed_current.contains("..") {
                return Err(ValidationError::PathTraversalAttempt {
                    path: trimmed_current.to_string(),
                });
            }

            if trimmed_current != trimmed_original {
                operations.push(ValidatedFileOperation {
                    old_name: trimmed_original.to_string(),
                    new_name: trimmed_current.to_string(),
                    operation_type: FileOperationType::Rename,
                });
            }
        }
    }

    Ok(operations)
}

/// Check if filename contains invalid characters for macOS
fn has_invalid_filename_chars(name: &str) -> bool {
    name.chars().any(|c| matches!(c, ':' | '\0'))
}

/// Analyze changes to determine operation types

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_filename() {
        let result = FilePath::new("valid_file.txt".to_string());
        assert!(result.is_ok());
    }

    #[test]
    fn test_invalid_colon_character() {
        let result = FilePath::new("invalid:file.txt".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_null_character() {
        let result = FilePath::new("file\0with_null.txt".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_path_traversal() {
        let result = FilePath::new("../secret_file".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_simple_rename() {
        let current = vec!["file1.txt".to_string(), "file2.txt".to_string()];
        let stored = vec!["old_file1.txt".to_string(), "file2.txt".to_string()];

        let result = validate_file_operations(&current, &stored);
        assert!(result.is_ok());

        let operations = result.unwrap();
        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0].operation_type, FileOperationType::Rename);
        assert_eq!(operations[0].old_name, "old_file1.txt");
        assert_eq!(operations[0].new_name, "file1.txt");
    }

    #[test]
    fn test_validate_deletion() {
        let current = vec!["".to_string(), "file2.txt".to_string()];
        let stored = vec!["to_delete.txt".to_string(), "file2.txt".to_string()];

        let result = validate_file_operations(&current, &stored);
        assert!(result.is_ok());

        let operations = result.unwrap();
        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0].operation_type, FileOperationType::Delete);
        assert_eq!(operations[0].old_name, "to_delete.txt");
    }

    #[test]
    fn test_invalid_character_validation() {
        let current = vec!["file:with:colons.txt".to_string()];
        let stored = vec!["old_file.txt".to_string()];

        let result = validate_file_operations(&current, &stored);
        assert!(matches!(
            result,
            Err(ValidationError::InvalidCharacter { .. })
        ));
    }

    #[test]
    fn test_count_mismatch() {
        let current = vec!["file1.txt".to_string(), "file2.txt".to_string()];
        let stored = vec!["file1.txt".to_string()];

        let result = validate_file_operations(&current, &stored);
        assert!(matches!(result, Err(ValidationError::CountMismatch { .. })));
    }
}
