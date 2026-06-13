use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use anyhow::{Context, Result};
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, AssignmentExpression, BindingIdentifier, BindingPattern, BlockStatement,
    CallExpression, ChainExpression, Expression, FormalParameterRest, Function,
    IdentifierReference, ImportDeclaration, ImportDeclarationSpecifier, LogicalExpression,
    ModuleExportName, PropertyKey, SpreadElement, Statement, StaticMemberExpression,
    VariableDeclaration, VariableDeclarationKind, VariableDeclarator,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_semantic::{Semantic, SemanticBuilder};
use oxc_span::{GetSpan, SourceType, Span};
use oxc_syntax::{
    operator::{AssignmentOperator, LogicalOperator},
    operator::{BinaryOperator, UnaryOperator, UpdateOperator},
    scope::ScopeFlags,
};

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
            runtime_object_scopes: vec![HashMap::new()],
            detections: Vec::new(),
            seen: HashSet::new(),
        };
        visitor.emit_file_support();
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
    runtime_object_scopes: Vec<HashMap<String, String>>,
    detections: Vec<DetectedFeature>,
    seen: DetectionSeen,
}

impl<'a> Visit<'a> for AstVisitor<'a, '_> {
    fn visit_expression(&mut self, expression: &Expression<'a>) {
        self.emit_expression_syntax(expression);
        walk::walk_expression(self, expression);
    }

    fn visit_statement(&mut self, statement: &Statement<'a>) {
        self.emit_statement_syntax(statement);
        walk::walk_statement(self, statement);
    }

    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        self.local_scopes.push(HashSet::new());
        self.runtime_object_scopes.push(HashMap::new());
        walk::walk_function(self, function, flags);
        self.runtime_object_scopes.pop();
        self.local_scopes.pop();
    }

    fn visit_block_statement(&mut self, block: &BlockStatement<'a>) {
        self.local_scopes.push(HashSet::new());
        self.runtime_object_scopes.push(HashMap::new());
        walk::walk_block_statement(self, block);
        self.runtime_object_scopes.pop();
        self.local_scopes.pop();
    }

    fn visit_binding_identifier(&mut self, ident: &BindingIdentifier<'a>) {
        if let Some(scope) = self.local_scopes.last_mut() {
            scope.insert(ident.name.as_str().to_owned());
        }
        walk::walk_binding_identifier(self, ident);
    }

    fn visit_import_declaration(&mut self, declaration: &ImportDeclaration<'a>) {
        self.emit_support("module.esm", declaration.span);
        self.record_import(declaration);
        walk::walk_import_declaration(self, declaration);
    }

    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        self.record_require(declarator);
        self.record_runtime_object(declarator);
        walk::walk_variable_declarator(self, declarator);
    }

    fn visit_variable_declaration(&mut self, declaration: &VariableDeclaration<'a>) {
        if let Some(syntax) = variable_declaration_syntax(declaration.kind) {
            self.emit_syntax(syntax, declaration.span);
        }
        walk::walk_variable_declaration(self, declaration);
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
        let property_feature = self.runtime.match_property(property);

        let mut emitted_member_feature = None;
        if let Some(raw_chain) = member_chain(member) {
            self.emit_module_support_from_chain(&raw_chain, member.span);
            let chain = self.canonicalize_chain(&raw_chain);
            if is_unresolved_sqlite_instance_alias(&raw_chain, &chain) {
                walk::walk_static_member_expression(self, member);
                return;
            }
            if let Some(feature) = self.runtime.match_member_chain(&chain) {
                self.emit(feature, member.span);
                emitted_member_feature = Some(feature.id);
            }
        }

        if let Some(feature_name) = self.iterator_helper_member(member)
            && let Some(feature) = self.runtime.match_member_chain(&feature_name)
        {
            self.emit(feature, member.span);
            emitted_member_feature = Some(feature.id);
        }

        if let Some(feature_name) = self.temporal_instance_member(member)
            && let Some(feature) = self.runtime.match_member_chain(&feature_name)
        {
            self.emit(feature, member.span);
            emitted_member_feature = Some(feature.id);
        }

        if let Some(feature) = property_feature
            && emitted_member_feature != Some(feature.id)
        {
            self.emit(feature, member.property.span);
        }

        walk::walk_static_member_expression(self, member);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        self.emit_module_support_from_call(call);
        walk::walk_call_expression(self, call);
    }

    fn visit_assignment_expression(&mut self, expression: &AssignmentExpression<'a>) {
        if let Some(syntax) = assignment_operator_syntax(expression.operator) {
            self.emit_syntax(syntax, expression.span);
        }

        walk::walk_assignment_expression(self, expression);
    }

    fn visit_logical_expression(&mut self, expression: &LogicalExpression<'a>) {
        if let Some(syntax) = logical_operator_syntax(expression.operator) {
            self.emit_syntax(syntax, expression.span);
        }

        walk::walk_logical_expression(self, expression);
    }

    fn visit_chain_expression(&mut self, expression: &ChainExpression<'a>) {
        self.emit_syntax("operators.optional_chaining", expression.span);
        walk::walk_chain_expression(self, expression);
    }

    fn visit_spread_element(&mut self, expression: &SpreadElement<'a>) {
        self.emit_syntax("operators.spread", expression.span);
        walk::walk_spread_element(self, expression);
    }

    fn visit_formal_parameter_rest(&mut self, rest: &FormalParameterRest<'a>) {
        self.emit_syntax("functions.rest_parameters", rest.span);
        walk::walk_formal_parameter_rest(self, rest);
    }
}

impl AstVisitor<'_, '_> {
    fn emit_file_support(&mut self) {
        match self
            .path
            .extension()
            .and_then(|extension| extension.to_str())
        {
            Some("mjs" | "mts") => self.emit_support("module.esm", Span::new(0, 0)),
            Some("cjs" | "cts") => self.emit_support("module.commonjs", Span::new(0, 0)),
            _ => {}
        }

        if self
            .path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| matches!(extension, "ts" | "tsx" | "mts" | "cts"))
        {
            self.emit_support("typescript.native", Span::new(0, 0));
        }
    }

    fn emit_expression_syntax(&mut self, expression: &Expression<'_>) {
        match expression {
            Expression::BooleanLiteral(literal) => {
                self.emit_syntax("grammar.boolean_literals", literal.span);
            }
            Expression::NullLiteral(literal) => {
                self.emit_syntax("grammar.null_literal", literal.span)
            }
            Expression::NumericLiteral(literal) => {
                self.emit_syntax("grammar.decimal_numeric_literals", literal.span);
            }
            Expression::BigIntLiteral(literal) => {
                self.emit_syntax("grammar.bigint_literals", literal.span);
            }
            Expression::RegExpLiteral(literal) => {
                self.emit_syntax("grammar.regular_expression_literals", literal.span);
            }
            Expression::StringLiteral(literal) => {
                self.emit_syntax("grammar.string_literals", literal.span)
            }
            Expression::TemplateLiteral(literal) => {
                self.emit_syntax("grammar.template_literals", literal.span);
            }
            Expression::ArrayExpression(expression) => {
                self.emit_syntax("grammar.array_literals", expression.span);
            }
            Expression::ArrowFunctionExpression(expression) => {
                self.emit_syntax("functions.arrow_functions", expression.span);
            }
            Expression::AwaitExpression(expression) => {
                self.emit_syntax("operators.await", expression.span)
            }
            Expression::BinaryExpression(expression) => {
                if let Some(syntax) = binary_operator_syntax(expression.operator) {
                    self.emit_syntax(syntax, expression.span);
                }
            }
            Expression::ClassExpression(expression) => {
                self.emit_syntax("operators.class", expression.span);
                self.emit_syntax("statements.class", expression.span);
            }
            Expression::ConditionalExpression(expression) => {
                self.emit_syntax("operators.conditional", expression.span);
            }
            Expression::FunctionExpression(expression) => {
                self.emit_syntax("operators.function", expression.span);
                if expression.generator {
                    self.emit_syntax("operators.generator_function", expression.span);
                }
                if expression.r#async {
                    self.emit_syntax("operators.async_function", expression.span);
                }
            }
            Expression::LogicalExpression(expression) => {
                if let Some(syntax) = logical_operator_syntax(expression.operator) {
                    self.emit_syntax(syntax, expression.span);
                }
            }
            Expression::NewExpression(expression) => {
                self.emit_syntax("operators.new", expression.span)
            }
            Expression::ObjectExpression(expression) => {
                self.emit_syntax("operators.object_initializer", expression.span);
            }
            Expression::SequenceExpression(expression) => {
                self.emit_syntax("operators.comma", expression.span)
            }
            Expression::Super(expression) => self.emit_syntax("operators.super", expression.span),
            Expression::ThisExpression(expression) => {
                self.emit_syntax("operators.this", expression.span)
            }
            Expression::UnaryExpression(expression) => {
                if let Some(syntax) = unary_operator_syntax(expression.operator) {
                    self.emit_syntax(syntax, expression.span);
                }
            }
            Expression::UpdateExpression(expression) => {
                if let Some(syntax) = update_operator_syntax(expression.operator) {
                    self.emit_syntax(syntax, expression.span);
                }
            }
            Expression::YieldExpression(expression) => {
                self.emit_syntax("operators.yield", expression.span)
            }
            Expression::PrivateInExpression(expression) => {
                self.emit_syntax("classes.private_class_fields_in", expression.span);
            }
            _ => {}
        }
    }

    fn emit_statement_syntax(&mut self, statement: &Statement<'_>) {
        match statement {
            Statement::BlockStatement(statement) => {
                self.emit_syntax("statements.block", statement.span)
            }
            Statement::BreakStatement(statement) => {
                self.emit_syntax("statements.break", statement.span)
            }
            Statement::ContinueStatement(statement) => {
                self.emit_syntax("statements.continue", statement.span);
            }
            Statement::DebuggerStatement(statement) => {
                self.emit_syntax("statements.debugger", statement.span);
            }
            Statement::DoWhileStatement(statement) => {
                self.emit_syntax("statements.do_while", statement.span);
            }
            Statement::EmptyStatement(statement) => {
                self.emit_syntax("statements.empty", statement.span)
            }
            Statement::ForInStatement(statement) => {
                self.emit_syntax("statements.for_in", statement.span)
            }
            Statement::ForOfStatement(statement) => {
                self.emit_syntax("statements.for_of", statement.span)
            }
            Statement::ForStatement(statement) => {
                self.emit_syntax("statements.for", statement.span)
            }
            Statement::IfStatement(statement) => {
                self.emit_syntax("statements.if_else", statement.span)
            }
            Statement::LabeledStatement(statement) => {
                self.emit_syntax("statements.label", statement.span)
            }
            Statement::ReturnStatement(statement) => {
                self.emit_syntax("statements.return", statement.span)
            }
            Statement::SwitchStatement(statement) => {
                self.emit_syntax("statements.switch", statement.span)
            }
            Statement::ThrowStatement(statement) => {
                self.emit_syntax("statements.throw", statement.span)
            }
            Statement::TryStatement(statement) => {
                self.emit_syntax("statements.try_catch", statement.span)
            }
            Statement::WhileStatement(statement) => {
                self.emit_syntax("statements.while", statement.span)
            }
            Statement::WithStatement(statement) => {
                self.emit_syntax("statements.with", statement.span)
            }
            Statement::FunctionDeclaration(function) => {
                self.emit_syntax("statements.function", function.span);
                if function.generator {
                    self.emit_syntax("statements.generator_function", function.span);
                }
                if function.r#async {
                    self.emit_syntax("statements.async_function", function.span);
                }
            }
            Statement::ClassDeclaration(class) => {
                self.emit_syntax("statements.class", class.span);
                self.emit_syntax("operators.class", class.span);
            }
            Statement::ImportDeclaration(declaration) => {
                self.emit_support("module.esm", declaration.span);
                self.emit_syntax("statements.import", declaration.span);
                self.emit_syntax("operators.import", declaration.span);
            }
            Statement::ExportNamedDeclaration(declaration) => {
                self.emit_support("module.esm", declaration.span);
                self.emit_syntax("statements.export", declaration.span);
            }
            Statement::ExportDefaultDeclaration(declaration) => {
                self.emit_support("module.esm", declaration.span);
                self.emit_syntax("statements.export", declaration.span);
                self.emit_syntax("statements.export.default", declaration.span);
            }
            Statement::ExportAllDeclaration(declaration) => {
                self.emit_support("module.esm", declaration.span);
                self.emit_syntax("statements.export", declaration.span);
                self.emit_syntax("statements.export.namespace", declaration.span);
            }
            _ => {}
        }
    }

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
        self.emit_support("module.commonjs", init.span());

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

    fn record_runtime_object(&mut self, declarator: &VariableDeclarator<'_>) {
        let Some(init) = declarator.init.as_ref() else {
            return;
        };
        let Some(runtime_type) = self.expression_runtime_type(init) else {
            return;
        };

        if let BindingPattern::BindingIdentifier(binding) = &declarator.id
            && let Some(scope) = self.runtime_object_scopes.last_mut()
        {
            scope.insert(binding.name.as_str().to_owned(), runtime_type);
        }
    }

    fn expression_runtime_type(&self, expression: &Expression<'_>) -> Option<String> {
        match expression {
            Expression::NewExpression(expression) => {
                let chain = self.canonicalize_expression_chain(&expression.callee)?;
                match chain.as_str() {
                    "sqlite.DatabaseSync" => Some("sqlite.DatabaseSync".to_owned()),
                    _ => None,
                }
            }
            Expression::CallExpression(expression) => {
                let chain = self.canonicalize_expression_chain(&expression.callee)?;
                match chain.as_str() {
                    "sqlite.DatabaseSync.prepare" => Some("sqlite.StatementSync".to_owned()),
                    _ => None,
                }
            }
            Expression::ParenthesizedExpression(expression) => {
                self.expression_runtime_type(&expression.expression)
            }
            Expression::TSAsExpression(expression) => {
                self.expression_runtime_type(&expression.expression)
            }
            Expression::TSSatisfiesExpression(expression) => {
                self.expression_runtime_type(&expression.expression)
            }
            Expression::TSNonNullExpression(expression) => {
                self.expression_runtime_type(&expression.expression)
            }
            Expression::TSTypeAssertion(expression) => {
                self.expression_runtime_type(&expression.expression)
            }
            _ => None,
        }
    }

    fn canonicalize_expression_chain(&self, expression: &Expression<'_>) -> Option<String> {
        expression_chain(expression).map(|parts| self.canonicalize_chain(&parts))
    }

    fn canonicalize_chain(&self, parts: &[&str]) -> String {
        let Some((root, tail)) = parts.split_first() else {
            return String::new();
        };
        let root = self
            .runtime_object_type(root)
            .or_else(|| self.named_imports.get(*root).map(String::as_str))
            .or_else(|| self.namespace_imports.get(*root).map(String::as_str))
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

    fn runtime_object_type(&self, name: &str) -> Option<&str> {
        self.runtime_object_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).map(String::as_str))
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

    fn emit_syntax(&mut self, syntax: &str, span: Span) {
        if let Some(feature) = self.runtime.match_syntax(syntax) {
            self.emit(feature, span);
        }
    }

    fn emit_support(&mut self, support: &str, span: Span) {
        if let Some(feature) = self.runtime.match_support(support) {
            self.emit(feature, span);
        }
    }

    fn emit_module_support_from_call(&mut self, call: &CallExpression<'_>) {
        if is_require_call(call) {
            self.emit_support("module.commonjs", call.span);
        }
        if is_iife_call(call) {
            self.emit_support("module.iife", call.span);
        }
    }

    fn emit_module_support_from_chain(&mut self, parts: &[&str], span: Span) {
        match parts {
            ["module", "exports", ..] | ["exports", ..] => {
                self.emit_support("module.commonjs", span);
            }
            ["define", "amd", ..] => {
                self.emit_support("module.umd", span);
            }
            _ => {}
        }
    }

    fn iterator_helper_member(&self, member: &StaticMemberExpression<'_>) -> Option<String> {
        let method = member.property.name.as_str();
        if !is_iterator_helper_method(method) || !self.is_iterator_expression(&member.object) {
            return None;
        }
        Some(format!("Iterator.{method}"))
    }

    fn is_iterator_expression(&self, expression: &Expression<'_>) -> bool {
        match expression {
            Expression::CallExpression(call) => self.is_iterator_call(call),
            Expression::ParenthesizedExpression(expression) => {
                self.is_iterator_expression(&expression.expression)
            }
            Expression::TSAsExpression(expression) => {
                self.is_iterator_expression(&expression.expression)
            }
            Expression::TSSatisfiesExpression(expression) => {
                self.is_iterator_expression(&expression.expression)
            }
            Expression::TSNonNullExpression(expression) => {
                self.is_iterator_expression(&expression.expression)
            }
            Expression::TSTypeAssertion(expression) => {
                self.is_iterator_expression(&expression.expression)
            }
            _ => false,
        }
    }

    fn is_iterator_call(&self, call: &CallExpression<'_>) -> bool {
        let Some(chain) = self.canonicalize_expression_chain(&call.callee) else {
            return false;
        };
        chain == "Iterator.from"
            || chain
                .strip_prefix("Iterator.")
                .is_some_and(is_iterator_helper_method)
    }

    fn temporal_instance_member(&self, member: &StaticMemberExpression<'_>) -> Option<String> {
        let owner = self.temporal_expression_type(&member.object)?;
        Some(format!("{owner}.{}", member.property.name.as_str()))
    }

    fn temporal_expression_type(&self, expression: &Expression<'_>) -> Option<String> {
        match expression {
            Expression::CallExpression(call) => {
                if let Some(chain) = self.canonicalize_expression_chain(&call.callee) {
                    return temporal_call_result_type(&chain).map(str::to_owned);
                }
                if let Expression::StaticMemberExpression(member) = &call.callee
                    && let Some(owner) = self.temporal_expression_type(&member.object)
                    && self
                        .runtime
                        .match_member_chain(&format!("{}.{}", owner, member.property.name.as_str()))
                        .is_some()
                {
                    return Some(owner);
                }
                None
            }
            Expression::ParenthesizedExpression(expression) => {
                self.temporal_expression_type(&expression.expression)
            }
            Expression::TSAsExpression(expression) => {
                self.temporal_expression_type(&expression.expression)
            }
            Expression::TSSatisfiesExpression(expression) => {
                self.temporal_expression_type(&expression.expression)
            }
            Expression::TSNonNullExpression(expression) => {
                self.temporal_expression_type(&expression.expression)
            }
            Expression::TSTypeAssertion(expression) => {
                self.temporal_expression_type(&expression.expression)
            }
            _ => None,
        }
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

fn is_iife_call(call: &CallExpression<'_>) -> bool {
    match &call.callee {
        Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_) => true,
        Expression::ParenthesizedExpression(expression) => matches!(
            &expression.expression,
            Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_)
        ),
        _ => false,
    }
}

fn is_iterator_helper_method(method: &str) -> bool {
    matches!(
        method,
        "drop"
            | "every"
            | "filter"
            | "find"
            | "flatMap"
            | "forEach"
            | "map"
            | "reduce"
            | "some"
            | "take"
            | "toArray"
    )
}

fn temporal_call_result_type(chain: &str) -> Option<&'static str> {
    for owner in [
        "Temporal.Duration",
        "Temporal.Instant",
        "Temporal.PlainDate",
        "Temporal.PlainDateTime",
        "Temporal.PlainMonthDay",
        "Temporal.PlainTime",
        "Temporal.PlainYearMonth",
        "Temporal.ZonedDateTime",
    ] {
        let Some(member) = chain
            .strip_prefix(owner)
            .and_then(|tail| tail.strip_prefix('.'))
        else {
            continue;
        };
        if !member.is_empty() {
            return Some(owner);
        }
    }
    None
}

fn is_unresolved_sqlite_instance_alias(raw_parts: &[&str], canonical_chain: &str) -> bool {
    let Some(root) = raw_parts.first() else {
        return false;
    };
    matches!(*root, "database" | "statement" | "sqlTagStore")
        && canonical_chain == raw_parts.join(".")
}

fn assignment_operator_syntax(operator: AssignmentOperator) -> Option<&'static str> {
    match operator {
        AssignmentOperator::Assign => Some("operators.assignment"),
        AssignmentOperator::Addition => Some("operators.addition_assignment"),
        AssignmentOperator::Subtraction => Some("operators.subtraction_assignment"),
        AssignmentOperator::Multiplication => Some("operators.multiplication_assignment"),
        AssignmentOperator::Division => Some("operators.division_assignment"),
        AssignmentOperator::Remainder => Some("operators.remainder_assignment"),
        AssignmentOperator::Exponential => Some("operators.exponentiation_assignment"),
        AssignmentOperator::ShiftLeft => Some("operators.left_shift_assignment"),
        AssignmentOperator::ShiftRight => Some("operators.right_shift_assignment"),
        AssignmentOperator::ShiftRightZeroFill => Some("operators.unsigned_right_shift_assignment"),
        AssignmentOperator::BitwiseOR => Some("operators.bitwise_or_assignment"),
        AssignmentOperator::BitwiseXOR => Some("operators.bitwise_xor_assignment"),
        AssignmentOperator::BitwiseAnd => Some("operators.bitwise_and_assignment"),
        AssignmentOperator::LogicalAnd => Some("operators.logical_and_assignment"),
        AssignmentOperator::LogicalOr => Some("operators.logical_or_assignment"),
        AssignmentOperator::LogicalNullish => Some("operators.nullish_coalescing_assignment"),
    }
}

fn logical_operator_syntax(operator: LogicalOperator) -> Option<&'static str> {
    match operator {
        LogicalOperator::And => Some("operators.logical_and"),
        LogicalOperator::Or => Some("operators.logical_or"),
        LogicalOperator::Coalesce => Some("operators.nullish_coalescing"),
    }
}

fn binary_operator_syntax(operator: BinaryOperator) -> Option<&'static str> {
    match operator {
        BinaryOperator::Equality => Some("operators.equality"),
        BinaryOperator::Inequality => Some("operators.inequality"),
        BinaryOperator::StrictEquality => Some("operators.strict_equality"),
        BinaryOperator::StrictInequality => Some("operators.strict_inequality"),
        BinaryOperator::LessThan => Some("operators.less_than"),
        BinaryOperator::LessEqualThan => Some("operators.less_than_or_equal"),
        BinaryOperator::GreaterThan => Some("operators.greater_than"),
        BinaryOperator::GreaterEqualThan => Some("operators.greater_than_or_equal"),
        BinaryOperator::Addition => Some("operators.addition"),
        BinaryOperator::Subtraction => Some("operators.subtraction"),
        BinaryOperator::Multiplication => Some("operators.multiplication"),
        BinaryOperator::Division => Some("operators.division"),
        BinaryOperator::Remainder => Some("operators.remainder"),
        BinaryOperator::Exponential => Some("operators.exponentiation"),
        BinaryOperator::ShiftLeft => Some("operators.left_shift"),
        BinaryOperator::ShiftRight => Some("operators.right_shift"),
        BinaryOperator::ShiftRightZeroFill => Some("operators.unsigned_right_shift"),
        BinaryOperator::BitwiseOR => Some("operators.bitwise_or"),
        BinaryOperator::BitwiseXOR => Some("operators.bitwise_xor"),
        BinaryOperator::BitwiseAnd => Some("operators.bitwise_and"),
        BinaryOperator::In => Some("operators.in"),
        BinaryOperator::Instanceof => Some("operators.instanceof"),
    }
}

fn unary_operator_syntax(operator: UnaryOperator) -> Option<&'static str> {
    match operator {
        UnaryOperator::UnaryPlus => Some("operators.unary_plus"),
        UnaryOperator::UnaryNegation => Some("operators.unary_negation"),
        UnaryOperator::LogicalNot => Some("operators.logical_not"),
        UnaryOperator::BitwiseNot => Some("operators.bitwise_not"),
        UnaryOperator::Typeof => Some("operators.typeof"),
        UnaryOperator::Void => Some("operators.void"),
        UnaryOperator::Delete => Some("operators.delete"),
    }
}

fn update_operator_syntax(operator: UpdateOperator) -> Option<&'static str> {
    match operator {
        UpdateOperator::Increment => Some("operators.increment"),
        UpdateOperator::Decrement => Some("operators.decrement"),
    }
}

fn variable_declaration_syntax(kind: VariableDeclarationKind) -> Option<&'static str> {
    match kind {
        VariableDeclarationKind::Var => Some("statements.var"),
        VariableDeclarationKind::Let => Some("statements.let"),
        VariableDeclarationKind::Const => Some("statements.const"),
        VariableDeclarationKind::Using => Some("statements.using"),
        VariableDeclarationKind::AwaitUsing => Some("statements.await_using"),
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

    #[test]
    fn canonicalizes_sqlite_statement_methods_from_ast_flow() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sqlite.ts");
        fs::write(
            &path,
            "import { DatabaseSync } from 'node:sqlite';\nconst db = new DatabaseSync('db.sqlite');\nconst statement = db.prepare('select 1');\nstatement.run();\nstatement.columns();",
        )
        .unwrap();

        let detections = analyze(&path, &fs::read_to_string(&path).unwrap());
        assert!(
            detections
                .iter()
                .any(|feature| feature == "sqlite.DatabaseSync")
        );
        assert!(
            detections
                .iter()
                .any(|feature| feature == "sqlite.DatabaseSync.prepare")
        );
        assert!(
            detections
                .iter()
                .any(|feature| feature == "sqlite.StatementSync.run")
        );
        assert!(
            detections
                .iter()
                .any(|feature| feature == "sqlite.StatementSync.columns")
        );
        assert!(!detections.iter().any(|feature| feature == "statement.run"));
        assert!(
            !detections
                .iter()
                .any(|feature| feature == "statement.columns")
        );
    }

    #[test]
    fn does_not_count_unresolved_sqlite_statement_aliases() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("local.ts");
        fs::write(
            &path,
            "const statement = { run() {}, columns() {} };\nstatement.run();\nstatement.columns();",
        )
        .unwrap();

        let detections = analyze(&path, &fs::read_to_string(&path).unwrap());
        assert!(
            !detections
                .iter()
                .any(|feature| feature.starts_with("sqlite.StatementSync"))
        );
        assert!(!detections.iter().any(|feature| feature == "statement.run"));
        assert!(
            !detections
                .iter()
                .any(|feature| feature == "statement.columns")
        );
    }

    #[test]
    fn detects_logical_assignment_syntax() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("syntax.ts");
        fs::write(
            &path,
            "let value;\nvalue ??= 1;\nvalue ||= 2;\nvalue &&= 3;\n",
        )
        .unwrap();

        let detections = analyze(&path, &fs::read_to_string(&path).unwrap());
        assert!(
            detections
                .iter()
                .any(|feature| feature == "syntax.operators.nullish_coalescing_assignment")
        );
        assert!(
            detections
                .iter()
                .any(|feature| feature == "syntax.operators.logical_or_assignment")
        );
        assert!(
            detections
                .iter()
                .any(|feature| feature == "syntax.operators.logical_and_assignment")
        );
    }

    #[test]
    fn detects_optional_chaining_and_nullish_coalescing_syntax() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("syntax.ts");
        fs::write(&path, "const value = input?.name ?? 'fallback';\n").unwrap();

        let detections = analyze(&path, &fs::read_to_string(&path).unwrap());
        assert!(
            detections
                .iter()
                .any(|feature| feature == "syntax.operators.optional_chaining")
        );
        assert!(
            detections
                .iter()
                .any(|feature| feature == "syntax.operators.nullish_coalescing")
        );
    }

    #[test]
    fn detects_module_format_and_native_typescript_support() {
        let dir = tempdir().unwrap();
        let esm_path = dir.path().join("module.mts");
        fs::write(
            &esm_path,
            "import value from './value.js';\nexport default value;\n",
        )
        .unwrap();
        let esm = analyze(&esm_path, &fs::read_to_string(&esm_path).unwrap());
        assert!(esm.iter().any(|feature| feature == "module.esm"));
        assert!(esm.iter().any(|feature| feature == "typescript.native"));

        let cjs_path = dir.path().join("module.cjs");
        fs::write(
            &cjs_path,
            "const value = require('./value.js');\nmodule.exports = value;\n",
        )
        .unwrap();
        let cjs = analyze(&cjs_path, &fs::read_to_string(&cjs_path).unwrap());
        assert!(cjs.iter().any(|feature| feature == "module.commonjs"));

        let iife_path = dir.path().join("bundle.js");
        fs::write(&iife_path, "(function () { return 1; })();\n").unwrap();
        let iife = analyze(&iife_path, &fs::read_to_string(&iife_path).unwrap());
        assert!(iife.iter().any(|feature| feature == "module.iife"));

        let umd_path = dir.path().join("umd.js");
        fs::write(
            &umd_path,
            "if (typeof define === 'function' && define.amd) {}\n",
        )
        .unwrap();
        let umd = analyze(&umd_path, &fs::read_to_string(&umd_path).unwrap());
        assert!(umd.iter().any(|feature| feature == "module.umd"));
    }

    #[test]
    fn does_not_count_ordinary_methods_as_iterator_helpers() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("arrays.ts");
        fs::write(
            &path,
            "[1, 2, 3].map((value) => value).filter(Boolean).flatMap((value) => [value]);\n",
        )
        .unwrap();

        let detections = analyze(&path, &fs::read_to_string(&path).unwrap());
        assert!(
            !detections
                .iter()
                .any(|feature| feature.starts_with("Iterator."))
        );
    }

    #[test]
    fn counts_explicit_iterator_helper_member_chains() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("iterator.ts");
        fs::write(&path, "Iterator.from([1, 2, 3]).map((value) => value);\n").unwrap();

        let detections = analyze(&path, &fs::read_to_string(&path).unwrap());
        assert!(detections.iter().any(|feature| feature == "Iterator.from"));
        assert!(detections.iter().any(|feature| feature == "Iterator.map"));
    }

    #[test]
    fn does_not_count_owner_bound_methods_from_generic_property_names() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("duration.ts");
        fs::write(
            &path,
            "Duration.fromInput(value);\nduration.add(other).round('second').toString();\n",
        )
        .unwrap();

        let detections = analyze(&path, &fs::read_to_string(&path).unwrap());
        assert!(
            !detections
                .iter()
                .any(|feature| feature.starts_with("Temporal."))
        );
    }

    #[test]
    fn counts_explicit_temporal_member_chains() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("temporal.ts");
        fs::write(
            &path,
            "Temporal.Duration.from({ seconds: 1 }).add({ seconds: 1 }).round('second');\n",
        )
        .unwrap();

        let detections = analyze(&path, &fs::read_to_string(&path).unwrap());
        assert!(
            detections
                .iter()
                .any(|feature| feature == "Temporal.Duration.from")
        );
        assert!(
            detections
                .iter()
                .any(|feature| feature == "Temporal.Duration.add")
        );
        assert!(
            detections
                .iter()
                .any(|feature| feature == "Temporal.Duration.round")
        );
    }

    #[test]
    fn detects_common_statement_expression_and_literal_syntax() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("syntax.ts");
        fs::write(
            &path,
            r#"
var oldValue = 1;
let value = 2;
const list = [1, 2, 3];
class Box { method() { return `${value}`; } }
const lambda = (...items) => items.length == list.length ? /ok/u : null;
for (const item of list) {
  value += item;
}
for (const key in { ...list }) {
  value++;
}
"#,
        )
        .unwrap();

        let detections = analyze(&path, &fs::read_to_string(&path).unwrap());
        for expected in [
            "syntax.statements.var",
            "syntax.statements.let",
            "syntax.statements.const",
            "syntax.statements.class",
            "syntax.statements.for_of",
            "syntax.statements.for_in",
            "syntax.functions.arrow_functions",
            "syntax.functions.rest_parameters",
            "syntax.operators.equality",
            "syntax.operators.addition_assignment",
            "syntax.operators.increment",
            "syntax.operators.spread",
            "syntax.operators.conditional",
            "syntax.grammar.array_literals",
            "syntax.grammar.template_literals",
            "syntax.grammar.regular_expression_literals",
            "syntax.grammar.null_literal",
        ] {
            assert!(
                detections.iter().any(|feature| feature == expected),
                "missing {expected}"
            );
        }
    }
}
