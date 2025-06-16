use indexmap::IndexMap;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;

use tree_sitter::{Parser, Query, QueryCursor, StreamingIteratorMut};

#[derive(Debug, thiserror::Error)]
pub enum DependencyError {
    #[error("Failed to set language: {0}")]
    LanguageSetup(String),

    #[error("Failed to create query: {0}")]
    QueryCreation(String),

    #[error("Failed to parse source code")]
    ParseError,

    #[error("UTF-8 decoding error: {0}")]
    Utf8Error(String),

    #[error("Package not found: {0}")]
    PackageNotFound(String),

    #[error("Circular dependency detected")]
    CircularDependency,

    #[error("IO error: {0}")]
    IoError(String),
}

#[derive(Debug, Clone)]
pub struct PackageDependency {
    pub name: String,
    pub imports: HashSet<String>,
    pub instability: f64, // TODO: implement instability metric
}

pub struct DependencyGraph {
    /// Number of incoming edges for each package
    in_degree: IndexMap<String, usize>,
    /// List of packages that each package depends on
    adj: IndexMap<String, Vec<String>>,
}

const PACKAGE_QUERY: &str = r#"(package_clause (package_identifier) @package)"#;

const IMPORT_QUERY: &str = r#"
; Capture all import cases at once
(import_declaration
    (import_spec_list
    (import_spec
        name: (package_identifier)? @alias
        path: (interpreted_string_literal) @import)))

; Single import case
(import_declaration
    (import_spec
    name: (package_identifier)? @alias
    path: (interpreted_string_literal) @import))"#;

const GNO_LAND_PREFIX: &str = "gno.land/";
const GNO_FILE_EXTENSION: &str = "gno";

pub struct DependencyResolver {
    parser: Parser,
    package_query: Query,
    import_query: Query,
    cursor: QueryCursor,
    /// Strategy for resolving dependencies
    strategy: Box<dyn ResolutionStrategy>,
}

impl DependencyResolver {
    /// Creates a new DependencyResolver instance
    pub fn new() -> Result<Self, DependencyError> {
        let mut parser = Parser::new();
        let language = tree_sitter_go::LANGUAGE;

        parser
            .set_language(&language.into())
            .map_err(|e| DependencyError::LanguageSetup(e.to_string()))?;

        let package_query = Query::new(&language.into(), PACKAGE_QUERY)
            .map_err(|e| DependencyError::QueryCreation(format!("package query: {}", e)))?;

        // TODO: Consider raw strings and support dot imports
        let import_query = Query::new(&language.into(), IMPORT_QUERY)
            .map_err(|e| DependencyError::QueryCreation(format!("import query: {}", e)))?;

        Ok(Self {
            parser,
            package_query,
            import_query,
            cursor: QueryCursor::new(),
            strategy: Box::new(TopoSort),
        })
    }

    /// Extract dependencies from Gno source code
    pub fn extract_dependencies(
        &mut self,
        source_code: &str,
    ) -> Result<(String, HashSet<String>), DependencyError> {
        let tree = self
            .parser
            .parse(source_code, None)
            .ok_or(DependencyError::ParseError)?;

        let root_node = tree.root_node();
        let bytes = source_code.as_bytes();

        let package_name = self.extract_package_name(root_node, bytes)?;
        let imports = self.extract_imports(root_node, bytes)?;

        Ok((package_name, imports))
    }

    /// Extract dependencies from all .gno files in a directory recursively
    pub fn extract_dependencies_from_directory(
        &mut self,
        dir: &Path,
    ) -> Result<HashMap<String, PackageDependency>, DependencyError> {
        let mut packages: HashMap<String, PackageDependency> = HashMap::new();
        self.visit_directory(dir, &mut packages)?;
        Ok(packages)
    }

    /// Generate deployment order for packages based on their dependencies
    pub fn generate_deployment_order(
        &self,
        packages: &HashMap<String, PackageDependency>,
    ) -> Vec<String> {
        let graph = self.build_dependency_graph(packages);
        self.strategy.resolve(&graph)
    }

    /// Set the resolution strategy for the dependency resolver
    #[allow(unused)]
    pub fn with_strategy<S: ResolutionStrategy + 'static>(mut self, strategy: S) -> Self {
        self.strategy = Box::new(strategy);
        self
    }

    /// Extract package name from the parsed tree
    fn extract_package_name(
        &mut self,
        root_node: tree_sitter::Node,
        bytes: &[u8],
    ) -> Result<String, DependencyError> {
        let mut package_name = String::new();
        let mut matches = self.cursor.matches(&self.package_query, root_node, bytes);

        while let Some(matched) = matches.next_mut() {
            for capture in matched.captures {
                if self.package_query.capture_names()[capture.index as usize] == "package" {
                    package_name = capture
                        .node
                        .utf8_text(bytes)
                        .map_err(|e| DependencyError::Utf8Error(e.to_string()))?
                        .to_string();
                    break;
                }
            }
        }

        Ok(package_name)
    }

    /// Extract imports from the parsed tree
    fn extract_imports(
        &mut self,
        root_node: tree_sitter::Node,
        bytes: &[u8],
    ) -> Result<HashSet<String>, DependencyError> {
        let mut imports = HashSet::new();
        let mut matches = self.cursor.matches(&self.import_query, root_node, bytes);

        while let Some(matched) = matches.next_mut() {
            for capture in matched.captures {
                if self.import_query.capture_names()[capture.index as usize] == "import" {
                    let import_text = capture
                        .node
                        .utf8_text(bytes)
                        .map_err(|e| DependencyError::Utf8Error(e.to_string()))?
                        .trim_matches('"')
                        .to_string();

                    // Only include gno.land imports, not standard library imports
                    if import_text.starts_with(GNO_LAND_PREFIX) {
                        imports.insert(import_text);
                    }
                }
            }
        }

        Ok(imports)
    }

    /// Recursively visit directory and process .gno files
    fn visit_directory(
        &mut self,
        dir: &Path,
        packages: &mut HashMap<String, PackageDependency>,
    ) -> Result<(), DependencyError> {
        if !dir.is_dir() {
            return Ok(());
        }

        let entries = fs::read_dir(dir)
            .map_err(|e| DependencyError::IoError(format!("Failed to read directory: {}", e)))?;

        for entry in entries {
            let entry = entry
                .map_err(|e| DependencyError::IoError(format!("Failed to read entry: {}", e)))?;
            let path = entry.path();

            if path.is_dir() {
                self.visit_directory(&path, packages)?;
            } else if self.is_gno_file(&path) {
                self.process_gno_file(&path, packages)?;
            }
        }

        Ok(())
    }

    /// Check if a path is a .gno file
    fn is_gno_file(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|s| s.to_str())
            .map(|ext| ext == GNO_FILE_EXTENSION)
            .unwrap_or(false)
    }

    /// Process a single .gno file and add its dependencies to the packages map
    fn process_gno_file(
        &mut self,
        path: &Path,
        packages: &mut HashMap<String, PackageDependency>,
    ) -> Result<(), DependencyError> {
        let content = fs::read_to_string(path)
            .map_err(|e| DependencyError::IoError(format!("Failed to read file: {}", e)))?;

        let (package_name, imports) = self.extract_dependencies(&content)?;

        packages
            .entry(package_name.clone())
            .and_modify(|pkg| {
                // Merge imports if package already exists
                pkg.imports.extend(imports.clone());
            })
            .or_insert(PackageDependency {
                name: package_name,
                imports,
                instability: 0.0,
            });

        Ok(())
    }

    /// Build a dependency graph from packages
    fn build_dependency_graph(
        &self,
        packages: &HashMap<String, PackageDependency>,
    ) -> DependencyGraph {
        let mut in_degree: IndexMap<String, usize> = IndexMap::new();
        let mut adj: IndexMap<String, Vec<String>> = IndexMap::new();

        // Initialize all packages with zero in-degree
        for package_name in packages.keys() {
            in_degree.insert(package_name.clone(), 0);
            adj.insert(package_name.clone(), Vec::new());
        }

        // Build dependency relationships
        for (pkg_name, pkg) in packages {
            for import in &pkg.imports {
                if packages.contains_key(import) {
                    // Increment in-degree for the importing package
                    *in_degree.get_mut(pkg_name).unwrap() += 1;
                    // Add the importing package as a dependent of the imported package
                    adj.get_mut(import).unwrap().push(pkg_name.clone());
                }
            }
        }

        DependencyGraph { in_degree, adj }
    }
}

/// Strategy trait for dependency resolution algorithms
pub trait ResolutionStrategy {
    fn resolve(&self, graph: &DependencyGraph) -> Vec<String>;
}

/// Topological sort implementation for dependency resolution
pub struct TopoSort;

impl ResolutionStrategy for TopoSort {
    fn resolve(&self, graph: &DependencyGraph) -> Vec<String> {
        let mut in_degree = graph.in_degree.clone();
        let mut queue = VecDeque::new();
        let mut order = Vec::new();

        // Start with packages that have no dependencies (in-degree = 0)
        for (node, &degree) in &in_degree {
            if degree == 0 {
                queue.push_back(node.clone());
            }
        }

        // Process packages in topological order
        while let Some(current) = queue.pop_front() {
            order.push(current.clone());

            // Update in-degrees of dependent packages
            if let Some(dependents) = graph.adj.get(&current) {
                for dependent in dependents {
                    let degree = in_degree.get_mut(dependent).unwrap();
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(dependent.clone());
                    }
                }
            }
        }

        // Handle any remaining packages (indicates cycles)
        for (node, &degree) in &in_degree {
            if degree > 0 && !order.contains(node) {
                order.push(node.clone());
            }
        }

        order
    }
}

/// SAT solver strategy for dependency resolution (placeholder for future implementation)
#[allow(unused)]
struct SatResolver;

impl ResolutionStrategy for SatResolver {
    fn resolve(&self, _graph: &DependencyGraph) -> Vec<String> {
        unimplemented!("SAT solver strategy not yet implemented")
    }
}
