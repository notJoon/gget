use gget::dependency::{DependencyResolver, PackageDependency};
use std::collections::{HashMap, HashSet};

#[test]
fn test_dependency_resolver_creation() {
    let resolver = DependencyResolver::new();
    assert!(
        resolver.is_ok(),
        "DependencyResolver should be created successfully"
    );
}

#[test]
fn test_extract_dependencies_simple() {
    let mut resolver = DependencyResolver::new().unwrap();

    let gno_source = r#"
        package main
        import (
            "gno.land/p/demo/avl"
            "gno.land/p/demo/ufmt"
        )
        func main() {
            avl.NewTree()
            ufmt.Println("Hello")
        }
    "#;

    let result = resolver.extract_dependencies(gno_source);
    assert!(result.is_ok(), "Should parse Gno source successfully");

    let (package_name, imports) = result.unwrap();
    assert_eq!(package_name, "main");
    assert_eq!(imports.len(), 2);
    assert!(imports.contains("gno.land/p/demo/avl"));
    assert!(imports.contains("gno.land/p/demo/ufmt"));
}

#[test]
fn test_extract_dependencies_with_aliases() {
    let mut resolver = DependencyResolver::new().unwrap();

    let gno_source = r#"
        package aliases
        import (
            avl "gno.land/p/demo/avl"
            fmt "gno.land/p/demo/ufmt"
            utils "gno.land/p/demo/testutils"
        )
    "#;

    let (package_name, imports) = resolver.extract_dependencies(gno_source).unwrap();
    assert_eq!(package_name, "aliases");
    assert_eq!(imports.len(), 3);
    assert!(imports.contains("gno.land/p/demo/avl"));
    assert!(imports.contains("gno.land/p/demo/ufmt"));
    assert!(imports.contains("gno.land/p/demo/testutils"));
}

#[test]
fn test_extract_dependencies_blank_imports() {
    let mut resolver = DependencyResolver::new().unwrap();

    let gno_source = r#"
        package blank
        import (
            _ "gno.land/p/demo/avl"
            _ "gno.land/p/demo/ufmt"
            "gno.land/p/demo/testutils"
        )
    "#;

    let (package_name, imports) = resolver.extract_dependencies(gno_source).unwrap();
    assert_eq!(package_name, "blank");
    assert_eq!(imports.len(), 3);
    assert!(imports.contains("gno.land/p/demo/avl"));
    assert!(imports.contains("gno.land/p/demo/ufmt"));
    assert!(imports.contains("gno.land/p/demo/testutils"));
}

#[test]
fn test_extract_dependencies_mixed_import_styles() {
    let mut resolver = DependencyResolver::new().unwrap();

    let gno_source = r#"
        package mixed
        import (
            "fmt"
            avl "gno.land/p/demo/avl"
            _ "gno.land/p/demo/ufmt"
            "strings"
            "gno.land/p/demo/testutils"
        )
    "#;

    let (package_name, imports) = resolver.extract_dependencies(gno_source).unwrap();
    assert_eq!(package_name, "mixed");
    // Should only include gno.land imports, not standard library
    assert_eq!(imports.len(), 3);
    assert!(imports.contains("gno.land/p/demo/avl"));
    assert!(imports.contains("gno.land/p/demo/ufmt"));
    assert!(imports.contains("gno.land/p/demo/testutils"));
    // Should not include standard library imports
    assert!(!imports.contains("fmt"));
    assert!(!imports.contains("strings"));
}

#[test]
fn test_extract_dependencies_with_standard_library() {
    let mut resolver = DependencyResolver::new().unwrap();

    let gno_source = r#"
        package demo
        import (
            "fmt"
            "strings"
            "gno.land/p/demo/avl"
        )
    "#;

    let (package_name, imports) = resolver.extract_dependencies(gno_source).unwrap();
    assert_eq!(package_name, "demo");
    // Should only include gno.land imports, not standard library
    assert_eq!(imports.len(), 1);
    assert!(imports.contains("gno.land/p/demo/avl"));
    assert!(!imports.contains("fmt"));
    assert!(!imports.contains("strings"));
}

#[test]
fn test_extract_dependencies_single_import() {
    let mut resolver = DependencyResolver::new().unwrap();

    let gno_source = r#"
        package single
        import "gno.land/p/demo/testutils"
    "#;

    let (package_name, imports) = resolver.extract_dependencies(gno_source).unwrap();
    assert_eq!(package_name, "single");
    assert_eq!(imports.len(), 1);
    assert!(imports.contains("gno.land/p/demo/testutils"));
}

#[test]
fn test_extract_dependencies_no_imports() {
    let mut resolver = DependencyResolver::new().unwrap();

    let gno_source = r#"
        package standalone
        
        func Hello() string {
            return "Hello World"
        }
    "#;

    let (package_name, imports) = resolver.extract_dependencies(gno_source).unwrap();
    assert_eq!(package_name, "standalone");
    assert_eq!(imports.len(), 0);
}

#[test]
fn test_deployment_order_simple_chain() {
    let mut packages = HashMap::new();

    // Create dependency chain: A -> B -> C
    packages.insert(
        "gno.land/p/demo/A".to_string(),
        PackageDependency {
            name: "gno.land/p/demo/A".to_string(),
            imports: {
                let mut set = HashSet::new();
                set.insert("gno.land/p/demo/B".to_string());
                set
            },
            instability: 0.0,
        },
    );

    packages.insert(
        "gno.land/p/demo/B".to_string(),
        PackageDependency {
            name: "gno.land/p/demo/B".to_string(),
            imports: {
                let mut set = HashSet::new();
                set.insert("gno.land/p/demo/C".to_string());
                set
            },
            instability: 0.0,
        },
    );

    packages.insert(
        "gno.land/p/demo/C".to_string(),
        PackageDependency {
            name: "gno.land/p/demo/C".to_string(),
            imports: HashSet::new(),
            instability: 0.0,
        },
    );

    let resolver = DependencyResolver::new().unwrap();
    let deployment_order = resolver.generate_deployment_order(&packages);

    assert_eq!(deployment_order.len(), 3);

    // C should come first (no dependencies), then B, then A
    let c_pos = deployment_order
        .iter()
        .position(|p| p == "gno.land/p/demo/C")
        .unwrap();
    let b_pos = deployment_order
        .iter()
        .position(|p| p == "gno.land/p/demo/B")
        .unwrap();
    let a_pos = deployment_order
        .iter()
        .position(|p| p == "gno.land/p/demo/A")
        .unwrap();

    assert!(c_pos < b_pos, "C should come before B");
    assert!(b_pos < a_pos, "B should come before A");
}

#[test]
fn test_deployment_order_complex_dependencies() {
    let mut packages = HashMap::new();

    // Create complex dependency graph:
    // A -> B, C
    // B -> D
    // C -> D
    // D -> (no dependencies)
    // E -> A, D

    packages.insert(
        "gno.land/p/demo/A".to_string(),
        PackageDependency {
            name: "gno.land/p/demo/A".to_string(),
            imports: {
                let mut set = HashSet::new();
                set.insert("gno.land/p/demo/B".to_string());
                set.insert("gno.land/p/demo/C".to_string());
                set
            },
            instability: 0.0,
        },
    );

    packages.insert(
        "gno.land/p/demo/B".to_string(),
        PackageDependency {
            name: "gno.land/p/demo/B".to_string(),
            imports: {
                let mut set = HashSet::new();
                set.insert("gno.land/p/demo/D".to_string());
                set
            },
            instability: 0.0,
        },
    );

    packages.insert(
        "gno.land/p/demo/C".to_string(),
        PackageDependency {
            name: "gno.land/p/demo/C".to_string(),
            imports: {
                let mut set = HashSet::new();
                set.insert("gno.land/p/demo/D".to_string());
                set
            },
            instability: 0.0,
        },
    );

    packages.insert(
        "gno.land/p/demo/D".to_string(),
        PackageDependency {
            name: "gno.land/p/demo/D".to_string(),
            imports: HashSet::new(),
            instability: 0.0,
        },
    );

    packages.insert(
        "gno.land/p/demo/E".to_string(),
        PackageDependency {
            name: "gno.land/p/demo/E".to_string(),
            imports: {
                let mut set = HashSet::new();
                set.insert("gno.land/p/demo/A".to_string());
                set.insert("gno.land/p/demo/D".to_string());
                set
            },
            instability: 0.0,
        },
    );

    let resolver = DependencyResolver::new().unwrap();
    let deployment_order = resolver.generate_deployment_order(&packages);

    assert_eq!(deployment_order.len(), 5);

    // Verify topological ordering constraints
    let d_pos = deployment_order
        .iter()
        .position(|p| p == "gno.land/p/demo/D")
        .unwrap();
    let b_pos = deployment_order
        .iter()
        .position(|p| p == "gno.land/p/demo/B")
        .unwrap();
    let c_pos = deployment_order
        .iter()
        .position(|p| p == "gno.land/p/demo/C")
        .unwrap();
    let a_pos = deployment_order
        .iter()
        .position(|p| p == "gno.land/p/demo/A")
        .unwrap();
    let e_pos = deployment_order
        .iter()
        .position(|p| p == "gno.land/p/demo/E")
        .unwrap();

    // D must come before B, C, A, and E
    assert!(d_pos < b_pos, "D should come before B");
    assert!(d_pos < c_pos, "D should come before C");
    assert!(d_pos < a_pos, "D should come before A");
    assert!(d_pos < e_pos, "D should come before E");

    // B and C must come before A
    assert!(b_pos < a_pos, "B should come before A");
    assert!(c_pos < a_pos, "C should come before A");

    // A must come before E
    assert!(a_pos < e_pos, "A should come before E");
}

#[test]
fn test_deployment_order_cyclic_dependencies() {
    let mut packages = HashMap::new();

    // Create a cycle: X -> Y -> X
    packages.insert(
        "gno.land/p/demo/X".to_string(),
        PackageDependency {
            name: "gno.land/p/demo/X".to_string(),
            imports: {
                let mut set = HashSet::new();
                set.insert("gno.land/p/demo/Y".to_string());
                set
            },
            instability: 0.0,
        },
    );

    packages.insert(
        "gno.land/p/demo/Y".to_string(),
        PackageDependency {
            name: "gno.land/p/demo/Y".to_string(),
            imports: {
                let mut set = HashSet::new();
                set.insert("gno.land/p/demo/X".to_string());
                set
            },
            instability: 0.0,
        },
    );

    let resolver = DependencyResolver::new().unwrap();
    let deployment_order = resolver.generate_deployment_order(&packages);

    // Even with a cycle, should return all packages
    assert_eq!(deployment_order.len(), 2);

    let has_x = deployment_order.iter().any(|p| p == "gno.land/p/demo/X");
    let has_y = deployment_order.iter().any(|p| p == "gno.land/p/demo/Y");
    assert!(has_x, "Should include package X");
    assert!(has_y, "Should include package Y");
}

#[test]
fn test_parser_reuse_across_multiple_calls() {
    let mut resolver = DependencyResolver::new().unwrap();

    let source1 = r#"
        package pkg1
        import "gno.land/p/demo/avl"
    "#;

    let source2 = r#"
        package pkg2
        import (
            "gno.land/p/demo/ufmt"
            "gno.land/p/demo/testutils"
        )
    "#;

    // Test that the same resolver can be used multiple times
    let result1 = resolver.extract_dependencies(source1);
    assert!(result1.is_ok());
    let (pkg1, imports1) = result1.unwrap();
    assert_eq!(pkg1, "pkg1");
    assert_eq!(imports1.len(), 1);

    let result2 = resolver.extract_dependencies(source2);
    assert!(result2.is_ok());
    let (pkg2, imports2) = result2.unwrap();
    assert_eq!(pkg2, "pkg2");
    assert_eq!(imports2.len(), 2);
}

#[test]
fn test_invalid_gno_source() {
    let mut resolver = DependencyResolver::new().unwrap();

    // Completely invalid Go/Gno syntax
    let invalid_source = r#"
        this is not valid gno code at all
        ;;; syntax error ;;;
    "#;

    // Should handle parsing errors gracefully
    let result = resolver.extract_dependencies(invalid_source);
    // The parser might still succeed but return empty results
    // or it might fail - either is acceptable for invalid input
    match result {
        Ok((pkg, imports)) => {
            // If it succeeds, it should return empty/minimal results
            println!(
                "Parsed invalid source as package: '{}', imports: {:?}",
                pkg, imports
            );
        }
        Err(_) => {
            // If it fails, that's also acceptable for invalid input
            println!("Failed to parse invalid source (expected)");
        }
    }
}

#[test]
fn test_empty_source() {
    let mut resolver = DependencyResolver::new().unwrap();

    let result = resolver.extract_dependencies("");
    assert!(result.is_ok());
    let (package_name, imports) = result.unwrap();
    assert!(package_name.is_empty());
    assert!(imports.is_empty());
}

#[test]
fn test_package_only_no_imports() {
    let mut resolver = DependencyResolver::new().unwrap();

    let source = r#"package mypackage"#;

    let result = resolver.extract_dependencies(source);
    assert!(result.is_ok());
    let (package_name, imports) = result.unwrap();
    assert_eq!(package_name, "mypackage");
    assert!(imports.is_empty());
}
