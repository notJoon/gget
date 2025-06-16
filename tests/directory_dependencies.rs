use gget::dependency::DependencyResolver;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_read_all_gno_files_and_extract_dependencies() {
    // Create a temporary directory with some gno files
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    // Create test gno files
    let test_files = vec![
        (
            "main.gno",
            r#"package main
import (
    "gno.land/p/demo/avl"
    "gno.land/p/demo/ufmt"
)

func main() {
    avl.NewTree()
    ufmt.Println("Hello")
}"#,
        ),
        (
            "utils.gno",
            r#"package main
import (
    "gno.land/p/demo/testutils"
    "strings"
)

func TestHelper() {
    // test code
}"#,
        ),
        (
            "subdir/helper.gno",
            r#"package helper
import (
    "gno.land/p/demo/json"
    "gno.land/r/demo/users"
)

func Parse() {
    // parse code
}"#,
        ),
        (
            "noImports.gno",
            r#"package standalone

func Compute() int {
    return 42
}"#,
        ),
    ];

    // Create subdirectory
    fs::create_dir(temp_path.join("subdir")).unwrap();

    // Write test files
    for (filename, content) in &test_files {
        let file_path = temp_path.join(filename);
        fs::write(&file_path, content).unwrap();
    }

    // Also create a non-gno file to ensure it's ignored
    fs::write(temp_path.join("readme.md"), "This is a readme").unwrap();

    // Now test reading all gno files and extracting dependencies
    let mut resolver = DependencyResolver::new().unwrap();
    let result = resolver.extract_dependencies_from_directory(temp_path);

    assert!(result.is_ok(), "Should successfully read directory");
    let packages = result.unwrap();

    // We should have 4 packages (main appears twice but should be merged)
    assert!(packages.len() >= 3, "Should have at least 3 packages");

    // Check main package dependencies
    if let Some(main_pkg) = packages.get("main") {
        assert!(main_pkg.imports.contains("gno.land/p/demo/avl"));
        assert!(main_pkg.imports.contains("gno.land/p/demo/ufmt"));
        assert!(main_pkg.imports.contains("gno.land/p/demo/testutils"));
        // Should not include standard library
        assert!(!main_pkg.imports.contains("strings"));
    } else {
        panic!("Main package not found");
    }

    // Check helper package dependencies
    if let Some(helper_pkg) = packages.get("helper") {
        assert!(helper_pkg.imports.contains("gno.land/p/demo/json"));
        assert!(helper_pkg.imports.contains("gno.land/r/demo/users"));
    } else {
        panic!("Helper package not found");
    }

    // Check standalone package
    if let Some(standalone_pkg) = packages.get("standalone") {
        assert!(standalone_pkg.imports.is_empty());
    } else {
        panic!("Standalone package not found");
    }
}

#[test]
fn test_empty_directory() {
    let temp_dir = TempDir::new().unwrap();
    let mut resolver = DependencyResolver::new().unwrap();
    let result = resolver.extract_dependencies_from_directory(temp_dir.path());

    assert!(result.is_ok());
    let packages = result.unwrap();
    assert!(
        packages.is_empty(),
        "Empty directory should return no packages"
    );
}

#[test]
fn test_directory_with_no_gno_files() {
    let temp_dir = TempDir::new().unwrap();

    // Create some non-gno files
    fs::write(temp_dir.path().join("main.go"), "package main").unwrap();
    fs::write(temp_dir.path().join("README.md"), "# README").unwrap();
    fs::write(temp_dir.path().join("config.json"), "{}").unwrap();

    let mut resolver = DependencyResolver::new().unwrap();
    let result = resolver.extract_dependencies_from_directory(temp_dir.path());

    assert!(result.is_ok());
    let packages = result.unwrap();
    assert!(
        packages.is_empty(),
        "Directory with no .gno files should return no packages"
    );
}

#[test]
fn test_multiple_files_import_different_libraries_from_same_package() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();

    // Create test files that import different libraries from the same packages
    let test_files = vec![
        (
            "file1.gno",
            r#"package myapp
import (
    "gno.land/p/demo/avl"
    "gno.land/p/demo/ufmt"
    "gno.land/p/demo/json/parser"
)

func UseAvl() {
    // uses avl
}"#,
        ),
        (
            "file2.gno",
            r#"package myapp
import (
    "gno.land/p/demo/avl/node"
    "gno.land/p/demo/testutils"
    "gno.land/p/demo/json/encoder"
)

func UseNode() {
    // uses avl/node
}"#,
        ),
        (
            "file3.gno",
            r#"package myapp
import (
    "gno.land/p/demo/avl/tree"
    "gno.land/p/demo/ufmt/sprintf"
    "gno.land/p/demo/json"
)

func UseTree() {
    // uses avl/tree
}"#,
        ),
    ];

    // Write test files
    for (filename, content) in &test_files {
        let file_path = temp_path.join(filename);
        fs::write(&file_path, content).unwrap();
    }

    // Test reading all files and extracting dependencies
    let mut resolver = DependencyResolver::new().unwrap();
    let result = resolver.extract_dependencies_from_directory(temp_path);

    assert!(result.is_ok(), "Should successfully read directory");
    let packages = result.unwrap();

    // Should have only one package "myapp" with all imports merged
    assert_eq!(packages.len(), 1, "Should have exactly 1 package");

    // Check that all imports from different files are merged
    if let Some(myapp_pkg) = packages.get("myapp") {
        // Check all avl-related imports
        assert!(myapp_pkg.imports.contains("gno.land/p/demo/avl"));
        assert!(myapp_pkg.imports.contains("gno.land/p/demo/avl/node"));
        assert!(myapp_pkg.imports.contains("gno.land/p/demo/avl/tree"));

        // Check all ufmt-related imports
        assert!(myapp_pkg.imports.contains("gno.land/p/demo/ufmt"));
        assert!(myapp_pkg.imports.contains("gno.land/p/demo/ufmt/sprintf"));

        // Check all json-related imports
        assert!(myapp_pkg.imports.contains("gno.land/p/demo/json"));
        assert!(myapp_pkg.imports.contains("gno.land/p/demo/json/parser"));
        assert!(myapp_pkg.imports.contains("gno.land/p/demo/json/encoder"));

        // Check other imports
        assert!(myapp_pkg.imports.contains("gno.land/p/demo/testutils"));

        // Total unique imports should be 9
        assert_eq!(
            myapp_pkg.imports.len(),
            9,
            "Should have 9 unique imports total"
        );
    } else {
        panic!("myapp package not found");
    }
}
