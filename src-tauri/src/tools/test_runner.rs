use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use crate::tools::resolve_workspace_path;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestFramework {
    pub name: String,
    pub command: String,
    pub config_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub framework: String,
    pub command: String,
    pub success: bool,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub duration_ms: u64,
    pub output: String,
    pub failures: Vec<TestFailure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestFailure {
    pub name: String,
    pub message: String,
    pub location: Option<String>,
}

// ---------------------------------------------------------------------------
// Framework detection
// ---------------------------------------------------------------------------

fn detect_framework(workspace: &Path) -> Option<TestFramework> {
    // Check for various test frameworks
    let checks: Vec<(&str, &str, &str)> = vec![
        // Node.js / TypeScript
        ("package.json", "jest", "npx jest --no-coverage"),
        ("package.json", "vitest", "npx vitest run"),
        ("package.json", "mocha", "npx mocha"),
        ("package.json", "ava", "npx ava"),
        ("package.json", "jasmine", "npx jasmine"),
        // Rust
        ("Cargo.toml", "cargo", "cargo test"),
        // Python
        ("pyproject.toml", "pytest", "python -m pytest"),
        ("setup.py", "pytest", "python -m pytest"),
        ("requirements.txt", "pytest", "python -m pytest"),
        ("tox.ini", "pytest", "python -m pytest"),
        ("pytest.ini", "pytest", "python -m pytest"),
        // Go
        ("go.mod", "go test", "go test ./..."),
        // Java/Kotlin
        ("pom.xml", "maven", "mvn test"),
        ("build.gradle", "gradle", "gradle test"),
        ("build.gradle.kts", "gradle", "gradle test"),
    ];

    for (config_file, name, command) in &checks {
        if workspace.join(config_file).exists() {
            // For package.json, check if the specific framework is in dependencies
            if *config_file == "package.json" {
                if let Ok(content) = fs::read_to_string(workspace.join("package.json")) {
                    if content.contains(&format!("\"{}\"", name))
                        || content.contains(&format!("\"@types/{}\"", name))
                    {
                        return Some(TestFramework {
                            name: name.to_string(),
                            command: command.to_string(),
                            config_file: config_file.to_string(),
                        });
                    }
                }
            } else {
                return Some(TestFramework {
                    name: name.to_string(),
                    command: command.to_string(),
                    config_file: config_file.to_string(),
                });
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Test output parsing
// ---------------------------------------------------------------------------

fn parse_jest_output(output: &str) -> TestResult {
    static TOTAL_RE: OnceLock<Regex> = OnceLock::new();
    let total_re = TOTAL_RE.get_or_init(|| Regex::new(r"Tests:.*?(\d+)\s+total").unwrap_or_else(|_| Regex::new(r"Tests:.*").unwrap()));
    static PASS_RE: OnceLock<Regex> = OnceLock::new();
    let pass_re = PASS_RE.get_or_init(|| Regex::new(r"Tests:\s+(\d+)\s+passed").unwrap_or_else(|_| Regex::new(r"pass").unwrap()));
    static FAIL_RE: OnceLock<Regex> = OnceLock::new();
    let fail_re = FAIL_RE.get_or_init(|| Regex::new(r"Tests:\s+(\d+)\s+failed").unwrap_or_else(|_| Regex::new(r"fail").unwrap()));
    static SKIP_RE: OnceLock<Regex> = OnceLock::new();
    let skip_re = SKIP_RE.get_or_init(|| Regex::new(r"Skipped:\s+(\d+)").unwrap_or_else(|_| Regex::new(r"skip").unwrap()));

    let total = total_re.captures(output)
        .and_then(|c| c.get(1)?.as_str().parse::<usize>().ok())
        .unwrap_or(0);

    let passed = pass_re.captures(output)
        .and_then(|c| c.get(1)?.as_str().parse::<usize>().ok())
        .unwrap_or(0);

    let failed = fail_re.captures(output)
        .and_then(|c| c.get(1)?.as_str().parse::<usize>().ok())
        .unwrap_or(0);

    let skipped = skip_re.captures(output)
        .and_then(|c| c.get(1)?.as_str().parse::<usize>().ok())
        .unwrap_or(0);

    // Parse failures
    let mut failures = Vec::new();
    // regex crate has no look-ahead, so terminate each failure block at a blank
    // line (or end of input) instead of peeking for the next ● / Tests: marker.
    static FAIL_BLOCK_RE: OnceLock<Regex> = OnceLock::new();
    let fail_block_re = FAIL_BLOCK_RE.get_or_init(|| Regex::new(r"●\s+(.+?)\n([\s\S]*?)(?:\n\s*\n|\z)").unwrap());
    for cap in fail_block_re.captures_iter(output) {
        failures.push(TestFailure {
            name: cap.get(1).map(|m| m.as_str().trim().to_string()).unwrap_or_default(),
            message: cap.get(2).map(|m| m.as_str().trim().to_string()).unwrap_or_default(),
            location: None,
        });
    }

    TestResult {
        framework: "jest".to_string(),
        command: String::new(),
        success: failed == 0,
        total,
        passed,
        failed,
        skipped,
        duration_ms: 0,
        output: output.to_string(),
        failures,
    }
}

fn parse_cargo_test_output(output: &str) -> TestResult {
    static TEST_RE: OnceLock<Regex> = OnceLock::new();
    let test_re = TEST_RE.get_or_init(|| Regex::new(r"test result:\s+\w+\.\s+(\d+)\s+passed.*?(\d+)\s+failed.*?(\d+)\s+ignored").unwrap_or_else(|_| Regex::new(r"test result").unwrap()));

    let (passed, failed, ignored) = if let Some(caps) = test_re.captures(output) {
        (
            caps.get(1).and_then(|m| m.as_str().parse::<usize>().ok()).unwrap_or(0),
            caps.get(2).and_then(|m| m.as_str().parse::<usize>().ok()).unwrap_or(0),
            caps.get(3).and_then(|m| m.as_str().parse::<usize>().ok()).unwrap_or(0),
        )
    } else {
        (0, 0, 0)
    };

    // Parse failures
    let mut failures = Vec::new();
    static FAIL_RE: OnceLock<Regex> = OnceLock::new();
    let fail_re = FAIL_RE.get_or_init(|| Regex::new(r"---- (\w+) stdout ----\n([\s\S]+?)\n---- ").unwrap());
    for cap in fail_re.captures_iter(output) {
        failures.push(TestFailure {
            name: cap.get(1).map(|m| m.as_str().to_string()).unwrap_or_default(),
            message: cap.get(2).map(|m| m.as_str().trim().to_string()).unwrap_or_default(),
            location: None,
        });
    }

    TestResult {
        framework: "cargo".to_string(),
        command: String::new(),
        success: failed == 0,
        total: passed + failed,
        passed,
        failed,
        skipped: ignored,
        duration_ms: 0,
        output: output.to_string(),
        failures,
    }
}

fn parse_pytest_output(output: &str) -> TestResult {
    static SUMMARY_RE: OnceLock<Regex> = OnceLock::new();
    let _summary_re = SUMMARY_RE.get_or_init(|| Regex::new(r"(\d+) passed.*?(\d+) failed").unwrap_or_else(|_| Regex::new(r"passed").unwrap()));
    static PASSED_RE: OnceLock<Regex> = OnceLock::new();
    let passed_re = PASSED_RE.get_or_init(|| Regex::new(r"(\d+) passed").unwrap_or_else(|_| Regex::new(r"passed").unwrap()));
    static FAILED_RE: OnceLock<Regex> = OnceLock::new();
    let failed_re = FAILED_RE.get_or_init(|| Regex::new(r"(\d+) failed").unwrap_or_else(|_| Regex::new(r"failed").unwrap()));
    static SKIP_RE: OnceLock<Regex> = OnceLock::new();
    let skip_re = SKIP_RE.get_or_init(|| Regex::new(r"(\d+) skipped").unwrap_or_else(|_| Regex::new(r"skipped").unwrap()));

    let passed = passed_re.captures(output)
        .and_then(|c| c.get(1)?.as_str().parse::<usize>().ok())
        .unwrap_or(0);

    let failed = failed_re.captures(output)
        .and_then(|c| c.get(1)?.as_str().parse::<usize>().ok())
        .unwrap_or(0);

    let skipped = skip_re.captures(output)
        .and_then(|c| c.get(1)?.as_str().parse::<usize>().ok())
        .unwrap_or(0);

    // Parse failures
    let mut failures = Vec::new();
    static FAIL_RE: OnceLock<Regex> = OnceLock::new();
    let fail_re = FAIL_RE.get_or_init(|| Regex::new(r"FAILED\s+(.+?)::(.+?)\s+-\s+(.+)").unwrap());
    for cap in fail_re.captures_iter(output) {
        failures.push(TestFailure {
            name: format!("{}::{}", 
                cap.get(1).map(|m| m.as_str()).unwrap_or(""),
                cap.get(2).map(|m| m.as_str()).unwrap_or("")
            ),
            message: cap.get(3).map(|m| m.as_str().trim().to_string()).unwrap_or_default(),
            location: None,
        });
    }

    TestResult {
        framework: "pytest".to_string(),
        command: String::new(),
        success: failed == 0,
        total: passed + failed,
        passed,
        failed,
        skipped,
        duration_ms: 0,
        output: output.to_string(),
        failures,
    }
}

fn parse_go_test_output(output: &str) -> TestResult {
    static OK_RE: OnceLock<Regex> = OnceLock::new();
    let _ok_re = OK_RE.get_or_init(|| Regex::new(r"ok\s+").unwrap());
    static FAIL_RE: OnceLock<Regex> = OnceLock::new();
    let fail_re = FAIL_RE.get_or_init(|| Regex::new(r"FAIL\s+").unwrap());
    static PASS_RE: OnceLock<Regex> = OnceLock::new();
    let pass_re = PASS_RE.get_or_init(|| Regex::new(r"--- PASS:\s+(\w+)").unwrap());
    static FAIL_DETAIL_RE: OnceLock<Regex> = OnceLock::new();
    let fail_detail_re = FAIL_DETAIL_RE.get_or_init(|| Regex::new(r"--- FAIL:\s+(\w+)[\s\S]*?Error Trace:.*?\nError:.*?\n").unwrap());

    let passed = pass_re.captures_iter(output).count();
    let failed = fail_detail_re.captures_iter(output).count();

    let mut failures = Vec::new();
    for cap in fail_detail_re.captures_iter(output) {
        failures.push(TestFailure {
            name: cap.get(1).map(|m| m.as_str().to_string()).unwrap_or_default(),
            message: cap.get(0).map(|m| m.as_str().trim().to_string()).unwrap_or_default(),
            location: None,
        });
    }

    TestResult {
        framework: "go test".to_string(),
        command: String::new(),
        success: fail_re.find(output).is_none(),
        total: passed + failed,
        passed,
        failed,
        skipped: 0,
        duration_ms: 0,
        output: output.to_string(),
        failures,
    }
}

// ---------------------------------------------------------------------------
// Main command
// ---------------------------------------------------------------------------

/// Detect test framework and run tests in the workspace.
///
/// Automatically detects the test framework (jest, vitest, cargo test, pytest, go test, etc.),
/// runs the tests, parses the output, and returns structured results.
#[tauri::command]
pub fn tool_test_runner(
    path: String,
    filter: Option<String>,
    verbose: Option<bool>,
) -> Result<TestResult, String> {
    let _ws = resolve_workspace_path(&path)?;
    let workspace = std::env::current_dir()
        .map_err(|e| format!("Failed to get workspace: {e}"))?;

    let framework = detect_framework(&workspace)
        .ok_or("No test framework detected. Supported: jest, vitest, cargo test, pytest, go test, maven, gradle")?;

    let mut command_str = framework.command.clone();
    
    // Add filter if provided
    if let Some(ref f) = filter {
        match framework.name.as_str() {
            "jest" | "vitest" => command_str.push_str(&format!(" --testPathPattern={}", f)),
            "cargo" => command_str.push_str(&format!(" {}", f)),
            "pytest" => command_str.push_str(&format!(" -k {}", f)),
            "go test" => command_str.push_str(&format!(" -run {}", f)),
            _ => {}
        }
    }

    // Add verbose flag
    if verbose.unwrap_or(false) {
        match framework.name.as_str() {
            "jest" | "vitest" => command_str.push_str(" --verbose"),
            "cargo" => command_str.push_str(" -- --nocapture"),
            "pytest" => command_str.push_str(" -v"),
            "go test" => command_str.push_str(" -v"),
            _ => {}
        }
    }

    let start = std::time::Instant::now();

    // Determine shell based on OS
    let (shell, shell_arg) = if cfg!(target_os = "windows") {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };

    let output = Command::new(shell)
        .arg(shell_arg)
        .arg(&command_str)
        .current_dir(&workspace)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to run test command '{}': {e}", command_str))?;

    let duration = start.elapsed().as_millis() as u64;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let full_output = format!("{}\n{}", stdout, stderr);

    let mut result = match framework.name.as_str() {
        "jest" | "vitest" => parse_jest_output(&full_output),
        "cargo" => parse_cargo_test_output(&full_output),
        "pytest" => parse_pytest_output(&full_output),
        "go test" => parse_go_test_output(&full_output),
        _ => TestResult {
            framework: framework.name.clone(),
            command: command_str.clone(),
            success: output.status.success(),
            total: 0,
            passed: 0,
            failed: 0,
            skipped: 0,
            duration_ms: duration,
            output: full_output.clone(),
            failures: Vec::new(),
        },
    };

    result.framework = framework.name;
    result.command = command_str;
    result.duration_ms = duration;
    result.output = full_output;

    Ok(result)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_jest_output() {
        let output = r#"
PASS  src/utils.test.ts
  Utils
    ✓ should add numbers (12 ms)
    ✓ should subtract numbers (3 ms)

Test Suites: 1 passed, 1 total
Tests:       2 passed, 2 total
Snapshots:   0 total
Time:        0.5 s
"#;
        let result = parse_jest_output(output);
        assert!(result.success);
        assert_eq!(result.total, 2);
        assert_eq!(result.passed, 2);
        assert_eq!(result.failed, 0);
    }

    #[test]
    fn test_parse_cargo_test_output() {
        let output = r#"
running 3 tests
test tests::test_add ... ok
test tests::test_sub ... FAILED
test tests::test_mul ... ok

failures:

---- tests::test_sub stdout ----
thread 'main' panicked at 'assertion failed'

test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
"#;
        let result = parse_cargo_test_output(output);
        assert!(!result.success);
        assert_eq!(result.passed, 2);
        assert_eq!(result.failed, 1);
    }

    #[test]
    fn test_parse_pytest_output() {
        let output = r#"
============================= test session starts ==============================
collected 4 items

tests/test_math.py::test_add PASSED                                      [ 25%]
tests/test_math.py::test_sub FAILED                                      [ 50%]
tests/test_math.py::test_mul PASSED                                      [ 75%]
tests/test_math.py::test_div PASSED                                      [100%]

============================= FAILURES ========================================
FAILED tests/test_math.py::test_sub - assert 1 == 2

=================== 1 failed, 3 passed in 0.15s ====================
"#;
        let result = parse_pytest_output(output);
        assert!(!result.success);
        assert_eq!(result.passed, 3);
        assert_eq!(result.failed, 1);
    }
}
