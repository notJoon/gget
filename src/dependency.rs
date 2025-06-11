use std::collections::{HashMap, HashSet, VecDeque};

use tree_sitter::{Parser, Query, StreamingIteratorMut};

use crate::fetch::PackageManagerError;

#[derive(Debug, Clone)]
pub struct PackageDependency {
    pub name: String,
    pub imports: HashSet<String>,
    pub instability: f64, // TODO
}

pub struct DependencyResolver {
    parser: Parser,
    package_query: Query,
    import_query: Query,
}

impl DependencyResolver {
    pub fn new() -> Result<Self, PackageManagerError> {
        let mut parser = Parser::new();
        let language = tree_sitter_go::LANGUAGE;
        parser
            .set_language(&language.into())
            .map_err(|e| PackageManagerError::Rpc(format!("Failed to set language: {}", e)))?;

        let package_query = Query::new(
            &language.into(),
            r#"(package_clause (package_identifier) @package)"#,
        )
        .map_err(|e| PackageManagerError::Rpc(format!("Failed to create package query: {}", e)))?;

        // TODO: should we consider raw strings?
        // TODO: support dot imports
        let import_query = Query::new(
            &language.into(),
            r#"
            ; Single import with interpreted string (double quotes)
            (import_declaration
              (import_spec 
                path: (interpreted_string_literal) @import))
            
            ; Group import with interpreted string
            (import_declaration
              (import_spec_list
                (import_spec
                  path: (interpreted_string_literal) @import)))
            
            ; Named import with interpreted string (e.g., alias "path")
            (import_declaration
              (import_spec 
                name: (package_identifier) @alias
                path: (interpreted_string_literal) @import))

            ; Group named import with interpreted string
            (import_declaration
              (import_spec_list
                (import_spec
                  name: (package_identifier) @alias
                  path: (interpreted_string_literal) @import)))

            ; Blank import (e.g., _ "path") - capture it but filtering it out
            (import_declaration
              (import_spec 
                name: (blank_identifier)
                path: (interpreted_string_literal) @import))
            "#,
        )
        .map_err(|e| PackageManagerError::Rpc(format!("Failed to create import query: {}", e)))?;

        Ok(Self {
            parser,
            package_query,
            import_query,
        })
    }

    /// Extract dependencies from Gno source code
    pub fn extract_dependencies(
        &mut self,
        source_code: &str,
    ) -> Result<(String, HashSet<String>), PackageManagerError> {
        let tree = self
            .parser
            .parse(source_code, None)
            .ok_or_else(|| PackageManagerError::Rpc("Failed to parse source code".to_string()))?;

        let root_node = tree.root_node();
        let mut current_package = String::new();
        let mut imports = HashSet::new();

        // Extract package name first
        let mut cursor = tree_sitter::QueryCursor::new();
        let mut package_matches =
            cursor.matches(&self.package_query, root_node, source_code.as_bytes());

        while let Some(matched) = package_matches.next_mut() {
            for capture in matched.captures {
                if self.package_query.capture_names()[capture.index as usize] == "package" {
                    current_package = capture
                        .node
                        .utf8_text(source_code.as_bytes())
                        .map_err(|e| PackageManagerError::Rpc(format!("UTF8 error: {}", e)))?
                        .to_string();
                }
            }
        }

        // Extract imports separately for better performance
        let mut cursor = tree_sitter::QueryCursor::new();
        let mut import_matches =
            cursor.matches(&self.import_query, root_node, source_code.as_bytes());

        while let Some(matched) = import_matches.next_mut() {
            for capture in matched.captures {
                if self.import_query.capture_names()[capture.index as usize] == "import" {
                    let import_text = capture
                        .node
                        .utf8_text(source_code.as_bytes())
                        .map_err(|e| PackageManagerError::Rpc(format!("UTF8 error: {}", e)))?
                        .trim_matches('"');

                    // Only include `gno.land` imports for dependency resolution for now.
                    // TODO: Once bridge support begins, other prefixes besides `gno.land` may be supported in the future.
                    if import_text.starts_with("gno.land/") {
                        imports.insert(import_text.to_string());
                    }
                }
            }
        }

        Ok((current_package, imports))
    }

    pub fn generate_deployment_order(
        &self,
        packages: &HashMap<String, PackageDependency>,
    ) -> Vec<String> {
        let (mut dependency_count, dependents) = self.build_dependency_graph(packages);

        let mut queue: VecDeque<String> = dependency_count
            .iter()
            .filter(|(_, count)| **count == 0)
            .map(|(name, _)| name.clone())
            .collect();

        let mut result = Vec::new();

        while let Some(pkg_name) = queue.pop_front() {
            result.push(pkg_name.clone());

            if let Some(deps) = dependents.get(&pkg_name) {
                for dep in deps {
                    if let Some(count) = dependency_count.get_mut(dep) {
                        *count -= 1;
                        if *count == 0 {
                            queue.push_back(dep.clone());
                        }
                    }
                }
            }
        }

        // add any remaining packages (cycles)
        for (name, count) in dependency_count {
            if count > 0 && !result.contains(&name) {
                result.push(name);
            }
        }

        result
    }

    fn build_dependency_graph(
        &self,
        packages: &HashMap<String, PackageDependency>,
    ) -> (HashMap<String, usize>, HashMap<String, Vec<String>>) {
        let mut dependency_count: HashMap<String, usize> = HashMap::new();
        let mut dependents: HashMap<String, Vec<String>> = HashMap::new();

        // Initialize all packages with zero dependencies
        for package_name in packages.keys() {
            dependency_count.insert(package_name.clone(), 0);
            dependents.insert(package_name.clone(), Vec::new());
        }

        // Build dependency relationships
        for (pkg_name, pkg) in packages {
            for import in &pkg.imports {
                if packages.contains_key(import) {
                    // Increment dependency count for the importing package
                    *dependency_count.entry(pkg_name.clone()).or_insert(0) += 1;
                    // Add the importing package as a dependent of the imported package
                    dependents
                        .entry(import.clone())
                        .or_default()
                        .push(pkg_name.clone());
                }
            }
        }

        (dependency_count, dependents)
    }
}
