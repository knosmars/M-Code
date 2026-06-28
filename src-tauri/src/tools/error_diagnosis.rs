use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosisResult {
    pub error_type: String,
    pub severity: String, // error, warning, info
    pub message: String,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub suggestions: Vec<Suggestion>,
    pub related_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    pub title: String,
    pub description: String,
    pub code_example: Option<String>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPattern {
    pub pattern: String,
    pub error_type: String,
    pub message_template: String,
    pub suggestions: Vec<Suggestion>,
    pub related_patterns: Vec<String>,
}

// ---------------------------------------------------------------------------
// Error pattern library
// ---------------------------------------------------------------------------

fn get_error_patterns() -> Vec<ErrorPattern> {
    vec![
        // TypeScript/JavaScript patterns
        ErrorPattern {
            pattern: r"Property '(\w+)' does not exist on type '(\w+)'".to_string(),
            error_type: "typescript_property_missing".to_string(),
            message_template: "Property '{0}' does not exist on type '{1}'".to_string(),
            suggestions: vec![
                Suggestion {
                    title: "Check if property name is correct".to_string(),
                    description: "Verify the property exists on the type. Check for typos.".to_string(),
                    code_example: None,
                    confidence: 0.9,
                },
                Suggestion {
                    title: "Add property to type definition".to_string(),
                    description: "If this is a new property, add it to the interface/type.".to_string(),
                    code_example: Some("interface Foo {\n  bar: string;\n  baz?: number; // Add optional property\n}".to_string()),
                    confidence: 0.8,
                },
            ],
            related_patterns: vec![r"Property '(\w+)' is missing".to_string()],
        },
        ErrorPattern {
            pattern: r"Cannot find module '(\w+)'".to_string(),
            error_type: "module_not_found".to_string(),
            message_template: "Cannot find module '{0}'".to_string(),
            suggestions: vec![
                Suggestion {
                    title: "Install the package".to_string(),
                    description: "Run npm install <package> or yarn add <package>.".to_string(),
                    code_example: Some("npm install {0}".to_string()),
                    confidence: 0.9,
                },
                Suggestion {
                    title: "Check import path".to_string(),
                    description: "Verify the import path is correct and the module exists.".to_string(),
                    code_example: None,
                    confidence: 0.7,
                },
            ],
            related_patterns: vec![],
        },
        // Rust patterns
        ErrorPattern {
            pattern: r"cannot borrow `(\w+)` as mutable because it is also borrowed as (immutably|mutable)".to_string(),
            error_type: "rust_borrow_conflict".to_string(),
            message_template: "Cannot borrow '{0}' mutably because it is already borrowed".to_string(),
            suggestions: vec![
                Suggestion {
                    title: "Use separate scopes".to_string(),
                    description: "Ensure immutable and mutable borrows don't overlap.".to_string(),
                    code_example: Some("let immutable = &data;\n// ... use immutable ...\ndrop(immutable);\nlet mutable = &mut data;".to_string()),
                    confidence: 0.85,
                },
                Suggestion {
                    title: "Use RefCell for interior mutability".to_string(),
                    description: "If you need shared mutable access, use RefCell.".to_string(),
                    code_example: Some("use std::cell::RefCell;\nlet data = RefCell::new(vec![]);\ndata.borrow_mut().push(item);".to_string()),
                    confidence: 0.7,
                },
            ],
            related_patterns: vec![],
        },
        ErrorPattern {
            pattern: r"no method named `(\w+)` found for struct `(\w+)`".to_string(),
            error_type: "rust_method_not_found".to_string(),
            message_template: "No method named '{0}' found for struct '{1}'".to_string(),
            suggestions: vec![
                Suggestion {
                    title: "Check trait imports".to_string(),
                    description: "The method might be defined in a trait that needs to be imported.".to_string(),
                    code_example: Some("use std::io::Read; // Import the trait".to_string()),
                    confidence: 0.8,
                },
                Suggestion {
                    title: "Check method name".to_string(),
                    description: "Verify the method name is correct and exists on the type.".to_string(),
                    code_example: None,
                    confidence: 0.7,
                },
            ],
            related_patterns: vec![],
        },
        // Python patterns
        ErrorPattern {
            pattern: r"NameError: name '(\w+)' is not defined".to_string(),
            error_type: "python_name_error".to_string(),
            message_template: "NameError: name '{0}' is not defined".to_string(),
            suggestions: vec![
                Suggestion {
                    title: "Check variable scope".to_string(),
                    description: "The variable might be out of scope or not yet defined.".to_string(),
                    code_example: None,
                    confidence: 0.85,
                },
                Suggestion {
                    title: "Check imports".to_string(),
                    description: "The name might need to be imported.".to_string(),
                    code_example: Some("from module import {0}".to_string()),
                    confidence: 0.7,
                },
            ],
            related_patterns: vec![],
        },
        ErrorPattern {
            pattern: r"ImportError: cannot import name '(\w+)' from '(\w+)'".to_string(),
            error_type: "python_import_error".to_string(),
            message_template: "Cannot import name '{0}' from '{1}'".to_string(),
            suggestions: vec![
                Suggestion {
                    title: "Check if name exists in module".to_string(),
                    description: "The name might not be exported from the module.".to_string(),
                    code_example: None,
                    confidence: 0.8,
                },
                Suggestion {
                    title: "Check module path".to_string(),
                    description: "Verify the module path is correct.".to_string(),
                    code_example: None,
                    confidence: 0.7,
                },
            ],
            related_patterns: vec![],
        },
        // Go patterns
        ErrorPattern {
            pattern: r"undefined: (\w+)".to_string(),
            error_type: "go_undefined".to_string(),
            message_template: "undefined: {0}".to_string(),
            suggestions: vec![
                Suggestion {
                    title: "Check if name is defined".to_string(),
                    description: "The identifier might not be defined in the current scope.".to_string(),
                    code_example: None,
                    confidence: 0.8,
                },
                Suggestion {
                    title: "Check package imports".to_string(),
                    description: "The name might need to be imported from another package.".to_string(),
                    code_example: Some("import \"package/path\"".to_string()),
                    confidence: 0.7,
                },
            ],
            related_patterns: vec![],
        },
        // Common runtime errors
        ErrorPattern {
            pattern: r"TypeError: (Cannot read propert|Cannot call)".to_string(),
            error_type: "type_error".to_string(),
            message_template: "TypeError: {0}".to_string(),
            suggestions: vec![
                Suggestion {
                    title: "Add null/undefined check".to_string(),
                    description: "Check if the object is null or undefined before accessing.".to_string(),
                    code_example: Some("if (obj?.prop) {\n  // safe to access\n}".to_string()),
                    confidence: 0.85,
                },
                Suggestion {
                    title: "Use optional chaining".to_string(),
                    description: "Use ?. operator for safe property access.".to_string(),
                    code_example: Some("const value = obj?.prop?.nested;".to_string()),
                    confidence: 0.8,
                },
            ],
            related_patterns: vec![],
        },
        ErrorPattern {
            pattern: r"RangeError: (Maximum call stack|Invalid|Index out)".to_string(),
            error_type: "range_error".to_string(),
            message_template: "RangeError: {0}".to_string(),
            suggestions: vec![
                Suggestion {
                    title: "Check recursion depth".to_string(),
                    description: "Infinite recursion causes stack overflow. Add base case.".to_string(),
                    code_example: None,
                    confidence: 0.8,
                },
                Suggestion {
                    title: "Check array bounds".to_string(),
                    description: "Ensure array index is within valid range.".to_string(),
                    code_example: Some("if (index >= 0 && index < arr.length) {\n  // safe access\n}".to_string()),
                    confidence: 0.75,
                },
            ],
            related_patterns: vec![],
        },
    ]
}

// ---------------------------------------------------------------------------
// Diagnosis logic
// ---------------------------------------------------------------------------

fn diagnose_error(error_message: &str, file_path: Option<&str>) -> Vec<DiagnosisResult> {
    let patterns = get_error_patterns();
    let mut results = Vec::new();

    for pattern in &patterns {
        if let Ok(re) = Regex::new(&pattern.pattern) {
            if let Some(caps) = re.captures(error_message) {
                // Extract captured groups for message template
                let mut message = pattern.message_template.clone();
                for i in 1..=9 {
                    if let Some(m) = caps.get(i) {
                        message = message.replace(&format!("{{{}}}", i - 1), m.as_str());
                    }
                }

                results.push(DiagnosisResult {
                    error_type: pattern.error_type.clone(),
                    severity: "error".to_string(),
                    message,
                    file: file_path.map(|s| s.to_string()),
                    line: None,
                    suggestions: pattern.suggestions.clone(),
                    related_patterns: pattern.related_patterns.clone(),
                });
            }
        }
    }

    // If no patterns matched, provide generic advice
    if results.is_empty() {
        results.push(DiagnosisResult {
            error_type: "unknown".to_string(),
            severity: "info".to_string(),
            message: error_message.to_string(),
            file: file_path.map(|s| s.to_string()),
            line: None,
            suggestions: vec![
                Suggestion {
                    title: "Check error message carefully".to_string(),
                    description: "Read the error message to understand what went wrong.".to_string(),
                    code_example: None,
                    confidence: 0.5,
                },
                Suggestion {
                    title: "Search for similar errors".to_string(),
                    description: "Search the web for this error message to find solutions.".to_string(),
                    code_example: None,
                    confidence: 0.4,
                },
            ],
            related_patterns: vec![],
        });
    }

    results
}

// ---------------------------------------------------------------------------
// Main command
// ---------------------------------------------------------------------------

/// Diagnose an error message and provide suggestions for fixing it.
///
/// Matches the error against a library of known patterns and returns
/// contextual suggestions with code examples.
#[tauri::command]
pub fn tool_error_diagnosis(
    error_message: String,
    file_path: Option<String>,
    context: Option<String>,
) -> Result<Vec<DiagnosisResult>, String> {
    let results = diagnose_error(&error_message, file_path.as_deref());

    // If we have context, try to extract line number
    let mut final_results = results;
    if let Some(ref ctx) = context {
        static LINE_RE: OnceLock<Regex> = OnceLock::new();
        let line_re = LINE_RE.get_or_init(|| Regex::new(r"line (\d+)").unwrap());
        if let Some(caps) = line_re.captures(ctx) {
            if let Ok(line_num) = caps.get(1).unwrap().as_str().parse::<usize>() {
                for result in &mut final_results {
                    result.line = Some(line_num);
                }
            }
        }
    }

    Ok(final_results)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnose_typescript_error() {
        let results = diagnose_error(
            "Property 'name' does not exist on type 'User'",
            Some("src/types.ts"),
        );
        assert!(!results.is_empty());
        assert_eq!(results[0].error_type, "typescript_property_missing");
        assert!(!results[0].suggestions.is_empty());
    }

    #[test]
    fn test_diagnose_rust_borrow() {
        let results = diagnose_error(
            "cannot borrow `data` as mutable because it is also borrowed as immutably",
            Some("src/main.rs"),
        );
        assert!(!results.is_empty());
        assert_eq!(results[0].error_type, "rust_borrow_conflict");
    }

    #[test]
    fn test_diagnose_unknown_error() {
        let results = diagnose_error(
            "Something completely unknown went wrong",
            None,
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].error_type, "unknown");
    }
}
