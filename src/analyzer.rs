use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use anyhow::{Context, Result};
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, BindingIdentifier, BindingPattern, BlockStatement, CallExpression, Expression,
    Function, IdentifierReference, ImportDeclaration, ImportDeclarationSpecifier, ModuleExportName,
    PropertyKey, StaticMemberExpression, VariableDeclarator,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_semantic::{Semantic, SemanticBuilder};
use oxc_span::{SourceType, Span};
use oxc_syntax::scope::ScopeFlags;

use crate::{
    data::{Feature, RuntimeDb},
    scanner::{DetectedFeature, DetectionSeen, SourceFile, push_detection},
};

pub fn analyze_files_for_runtimes(
    root: &Path,
    files: &[SourceFile],
    runtimes: &[&RuntimeDb],
) -> Result<Vec<Vec<DetectedFeature>>> {
    let mut detections_by_runtime = vec![Vec::new(); runtimes.len()];
    for file in files {
        analyze_file_for_runtimes(file, runtimes, &mut detections_by_runtime)
            .with_context(|| format!("failed to analyze {}", file.path.display()))?;
    }
    let _ = root;
    Ok(detections_by_runtime)
}

fn analyze_file_for_runtimes(
    file: &SourceFile,
    runtimes: &[&RuntimeDb],
    detections_by_runtime: &mut [Vec<DetectedFeature>],
) -> Result<()> {
    let allocator = Allocator::default();
    let source_type =
        SourceType::from_path(&file.path).unwrap_or_else(|_| SourceType::unambiguous());
    let parsed = Parser::new(&allocator, &file.text, source_type).parse();
    if !parsed.errors.is_empty() {
        return Ok(());
    }

    let semantic = SemanticBuilder::new_compiler().build(&parsed.program);
    if !semantic.errors.is_empty() {
        return Ok(());
    }

    let line_index = LineIndex::new(&file.text);
    for (runtime, detections) in runtimes.iter().zip(detections_by_runtime.iter_mut()) {
        let mut visitor = AstVisitor {
            runtime,
            semantic: &semantic.semantic,
            line_index: &line_index,
            path: file.path.as_path(),
            namespace_imports: HashMap::new(),
            named_imports: HashMap::new(),
            local_scopes: vec![HashSet::new()],
            detections: Vec::new(),
            seen: HashSet::new(),
        };
        visitor.visit_program(&parsed.program);
        detections.append(&mut visitor.detections);
    }
    Ok(())
}

struct AstVisitor<'a, 'db> {
    runtime: &'db RuntimeDb,
    semantic: &'a Semantic<'a>,
    line_index: &'a LineIndex,
    path: &'a Path,
    namespace_imports: HashMap<String, String>,
    named_imports: HashMap<String, String>,
    local_scopes: Vec<HashSet<String>>,
    detections: Vec<DetectedFeature>,
    seen: DetectionSeen,
}

impl<'a> Visit<'a> for AstVisitor<'a, '_> {
    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        self.local_scopes.push(HashSet::new());
        walk::walk_function(self, function, flags);
        self.local_scopes.pop();
    }

    fn visit_block_statement(&mut self, block: &BlockStatement<'a>) {
        self.local_scopes.push(HashSet::new());
        walk::walk_block_statement(self, block);
        self.local_scopes.pop();
    }

    fn visit_binding_identifier(&mut self, ident: &BindingIdentifier<'a>) {
        if let Some(scope) = self.local_scopes.last_mut() {
            scope.insert(ident.name.as_str().to_owned());
        }
        walk::walk_binding_identifier(self, ident);
    }

    fn visit_import_declaration(&mut self, declaration: &ImportDeclaration<'a>) {
        self.record_import(declaration);
        walk::walk_import_declaration(self, declaration);
    }

    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        self.record_require(declarator);
        walk::walk_variable_declarator(self, declarator);
    }

    fn visit_identifier_reference(&mut self, ident: &IdentifierReference<'a>) {
        let name = ident.name.as_str();
        if !self.is_shadowed(name)
            && self.semantic.is_reference_to_global_variable(ident)
            && let Some(feature) = self.runtime.match_global(name)
        {
            self.emit(feature, ident.span);
        }

        if let Some(feature_name) = self.named_imports.get(name)
            && let Some(feature) = self.runtime.match_member_chain(feature_name)
        {
            self.emit(feature, ident.span);
        }

        walk::walk_identifier_reference(self, ident);
    }

    fn visit_static_member_expression(&mut self, member: &StaticMemberExpression<'a>) {
        let property = member.property.name.as_str();
        if let Some(feature) = self.runtime.match_property(property) {
            self.emit(feature, member.property.span);
        }

        if let Some(chain) = member_chain(member) {
            let chain = self.canonicalize_chain(chain);
            if let Some(feature) = self.runtime.match_member_chain(&chain) {
                self.emit(feature, member.span);
            }
        }

        walk::walk_static_member_expression(self, member);
    }
}

impl AstVisitor<'_, '_> {
    fn record_import(&mut self, declaration: &ImportDeclaration<'_>) {
        let Some(module) = normalize_module_name(declaration.source.value.as_str()) else {
            return;
        };
        let Some(specifiers) = &declaration.specifiers else {
            return;
        };

        for specifier in specifiers {
            match specifier {
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier) => {
                    self.namespace_imports
                        .insert(specifier.local.name.as_str().to_owned(), module.clone());
                }
                ImportDeclarationSpecifier::ImportSpecifier(specifier) => {
                    let Some(imported) = module_export_name(&specifier.imported) else {
                        continue;
                    };
                    let local = specifier.local.name.as_str().to_owned();
                    self.named_imports
                        .insert(local, format!("{module}.{imported}"));
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier) => {
                    self.namespace_imports
                        .insert(specifier.local.name.as_str().to_owned(), module.clone());
                }
            }
        }
    }

    fn record_require(&mut self, declarator: &VariableDeclarator<'_>) {
        let Some(init) = declarator.init.as_ref() else {
            return;
        };
        let Some(module) = require_module(init) else {
            return;
        };

        match &declarator.id {
            BindingPattern::BindingIdentifier(binding) => {
                self.namespace_imports
                    .insert(binding.name.as_str().to_owned(), module);
            }
            BindingPattern::ObjectPattern(pattern) => {
                for property in &pattern.properties {
                    let Some(imported) = property_key_name(&property.key) else {
                        continue;
                    };
                    if let BindingPattern::BindingIdentifier(local) = &property.value {
                        self.named_imports.insert(
                            local.name.as_str().to_owned(),
                            format!("{module}.{imported}"),
                        );
                    }
                }
            }
            _ => {}
        }
    }

    fn canonicalize_chain(&self, parts: Vec<&str>) -> String {
        let Some((root, tail)) = parts.split_first() else {
            return String::new();
        };
        let root = self
            .namespace_imports
            .get(*root)
            .map(String::as_str)
            .unwrap_or(*root);
        let capacity = root.len() + tail.iter().map(|part| part.len() + 1).sum::<usize>();
        let mut chain = String::with_capacity(capacity);
        chain.push_str(root);
        for part in tail {
            chain.push('.');
            chain.push_str(part);
        }
        chain
    }

    fn is_shadowed(&self, name: &str) -> bool {
        self.local_scopes
            .iter()
            .rev()
            .any(|scope| scope.contains(name))
    }

    fn emit(&mut self, feature: &Feature, span: Span) {
        let (line, column) = self.line_index.line_column(span.start as usize);
        push_detection(
            &mut self.detections,
            &mut self.seen,
            feature,
            0,
            self.path,
            line,
            column,
        );
    }
}

fn member_chain<'a>(member: &'a StaticMemberExpression<'a>) -> Option<Vec<&'a str>> {
    let mut parts = expression_chain(&member.object)?;
    parts.push(member.property.name.as_str());
    Some(parts)
}

fn expression_chain<'a>(expression: &'a Expression<'a>) -> Option<Vec<&'a str>> {
    match expression {
        Expression::Identifier(ident) => Some(vec![ident.name.as_str()]),
        Expression::StaticMemberExpression(member) => member_chain(member),
        Expression::ParenthesizedExpression(expression) => expression_chain(&expression.expression),
        Expression::TSAsExpression(expression) => expression_chain(&expression.expression),
        Expression::TSSatisfiesExpression(expression) => expression_chain(&expression.expression),
        Expression::TSNonNullExpression(expression) => expression_chain(&expression.expression),
        Expression::TSTypeAssertion(expression) => expression_chain(&expression.expression),
        _ => None,
    }
}

fn require_module(expression: &Expression<'_>) -> Option<String> {
    let Expression::CallExpression(call) = expression else {
        return None;
    };
    if !is_require_call(call) {
        return None;
    }
    let first = call.arguments.first()?;
    match first {
        Argument::StringLiteral(literal) => normalize_module_name(literal.value.as_str()),
        _ => None,
    }
}

fn is_require_call(call: &CallExpression<'_>) -> bool {
    matches!(&call.callee, Expression::Identifier(ident) if ident.name.as_str() == "require")
}

fn normalize_module_name(module: &str) -> Option<String> {
    let module = module.strip_prefix("node:").unwrap_or(module);
    match module {
        "fs/promises" => Some("fsPromises".to_owned()),
        "timers/promises" => Some("timersPromises".to_owned()),
        value
            if value
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '/') =>
        {
            Some(value.replace('/', "."))
        }
        _ => None,
    }
}

fn module_export_name(name: &ModuleExportName<'_>) -> Option<String> {
    match name {
        ModuleExportName::IdentifierName(name) => Some(name.name.as_str().to_owned()),
        ModuleExportName::IdentifierReference(name) => Some(name.name.as_str().to_owned()),
        ModuleExportName::StringLiteral(name) => Some(name.value.as_str().to_owned()),
    }
}

fn property_key_name(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(name) => Some(name.name.as_str().to_owned()),
        PropertyKey::StringLiteral(name) => Some(name.value.as_str().to_owned()),
        _ => None,
    }
}

struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut starts = vec![0];
        for (index, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                starts.push(index + 1);
            }
        }
        Self { starts }
    }

    fn line_column(&self, offset: usize) -> (u64, u64) {
        let line_index = self
            .starts
            .partition_point(|start| *start <= offset)
            .saturating_sub(1);
        let line_start = self.starts[line_index];
        ((line_index + 1) as u64, (offset - line_start + 1) as u64)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use tempfile::tempdir;

    use crate::{analyzer::analyze_files_for_runtimes, data::node_runtime, scanner::SourceFile};

    fn analyze(path: &Path, text: &str) -> Vec<String> {
        let runtime = node_runtime().unwrap();
        let files = vec![SourceFile {
            path: path.to_path_buf(),
            text: text.to_owned(),
        }];
        analyze_files_for_runtimes(path.parent().unwrap(), &files, &[runtime])
            .unwrap()
            .pop()
            .unwrap()
            .into_iter()
            .map(|detection| detection.feature)
            .collect()
    }

    #[test]
    fn detects_global_temporal_but_not_local_binding() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("date.ts");
        fs::write(
            &path,
            "Temporal.Now.instant();\nfunction shadow() { const Temporal = local;\nTemporal.Now.instant();\n}",
        )
        .unwrap();

        let detections = analyze(&path, &fs::read_to_string(&path).unwrap());
        assert!(detections.iter().any(|feature| feature == "Temporal"));
        assert_eq!(
            detections
                .iter()
                .filter(|feature| feature.as_str() == "Temporal")
                .count(),
            1
        );
    }

    #[test]
    fn detects_imported_fs_members() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("fs.ts");
        fs::write(
            &path,
            "import * as fs from 'node:fs';\nimport { cp } from 'node:fs/promises';\nfs.cp('a', 'b', () => {});\ncp('a', 'b');",
        )
        .unwrap();

        let detections = analyze(&path, &fs::read_to_string(&path).unwrap());
        assert!(detections.iter().any(|feature| feature == "fs.cp"));
        assert!(detections.iter().any(|feature| feature == "fsPromises.cp"));
    }
}
