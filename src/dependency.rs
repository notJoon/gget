use indexmap::IndexMap;
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
    /// strategy for resolving dependencies
    strategy: Box<dyn ResolutionStrategy>,
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
            strategy: Box::new(TopoSort),
        })
    }

    /// extract dependencies from Gno source code
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

        // extract package name first
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

        // extract imports separately for better performance
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

                    // only include `gno.land` imports for dependency resolution for now.
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
        let graph = self.build_dependency_graph(packages);
        self.strategy.resolve(&graph)
    }

    fn build_dependency_graph(
        &self,
        packages: &HashMap<String, PackageDependency>,
    ) -> DependencyGraph {
        // when deploying, the dependency order of packages is important.
        // so we need to ensure consistency in topological sorting.
        let mut in_degree: IndexMap<String, usize> = IndexMap::new();
        let mut adj: IndexMap<String, Vec<String>> = IndexMap::new();

        // initialize all packages with zero dependencies
        for package_name in packages.keys() {
            in_degree.insert(package_name.clone(), 0);
            adj.insert(package_name.clone(), Vec::new());
        }

        // build dependency relationships
        for (pkg_name, pkg) in packages {
            for import in &pkg.imports {
                if packages.contains_key(import) {
                    // increment dependency count for the importing package
                    *in_degree.get_mut(pkg_name).unwrap() += 1;
                    // add the importing package as a dependent of the imported package
                    adj.get_mut(import).unwrap().push(pkg_name.clone());
                }
            }
        }

        DependencyGraph { in_degree, adj }
    }

    /// set the resolution strategy for the dependency resolver
    #[allow(unused)]
    fn with_strategy<S: ResolutionStrategy + 'static>(mut self, st: S) -> Self {
        self.strategy = Box::new(st);
        self
    }
}

struct DependencyGraph {
    /// number of incoming edges for each package
    in_degree: IndexMap<String, usize>,
    /// list of packages that each package depends on
    adj: IndexMap<String, Vec<String>>,
}

/// Strategy that takes a dependency graph and returns deployment order (or failure info.)
/// This type is designed to allow using different SAT solvers when needed,
/// as Gno's package system may change in the future to consider other metadata such as versions.
trait ResolutionStrategy {
    fn resolve(&self, graph: &DependencyGraph) -> Vec<String>;
}

pub struct TopoSort;

impl ResolutionStrategy for TopoSort {
    fn resolve(&self, graph: &DependencyGraph) -> Vec<String> {
        let mut in_deg = graph.in_degree.clone();
        let mut q = VecDeque::new();
        let mut order = Vec::new();

        // start with 0-degree nodes
        for (node, &deg) in &in_deg {
            if deg == 0 {
                q.push_back(node.clone());
            }
        }

        while let Some(u) = q.pop_front() {
            order.push(u.clone());
            for v in &graph.adj[&u] {
                let e = in_deg.get_mut(v).unwrap();
                *e -= 1;
                if *e == 0 {
                    q.push_back(v.clone());
                }
            }
        }

        // add any remaining packages (cycles)
        for (node, &deg) in &in_deg {
            if deg > 0 && !order.contains(node) {
                order.push(node.clone());
            }
        }

        order
    }
}

/// Resolution strategy that uses a SAT solver to find a valid deployment order.
/// This is a placeholder for future implementation.
#[allow(unused)]
struct SatResolver;

impl ResolutionStrategy for SatResolver {
    fn resolve(&self, _graph: &DependencyGraph) -> Vec<String> {
        unimplemented!()
    }
}
