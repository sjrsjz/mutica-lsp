use mutica::mutica_compiler::parser::{ParseError, WithLocation, ast::TypeAst};

/// 递归清理 AST 中的 ParseError 节点，将其替换为 Bottom
pub fn sanitize_ast<'input>(ast: WithLocation<TypeAst<'input>>) -> WithLocation<TypeAst<'input>> {
    ast.map(|value| match value {
        TypeAst::ParseError(_) => TypeAst::Bottom,
        TypeAst::Tuple(items) => TypeAst::Tuple(items.into_iter().map(sanitize_ast).collect()),
        TypeAst::List(items) => TypeAst::List(items.into_iter().map(sanitize_ast).collect()),
        TypeAst::Generalize(items) => {
            TypeAst::Generalize(items.into_iter().map(sanitize_ast).collect())
        }
        TypeAst::Specialize(items) => {
            TypeAst::Specialize(items.into_iter().map(sanitize_ast).collect())
        }
        TypeAst::Invoke {
            func,
            arg,
            continuation,
        } => TypeAst::Invoke {
            func: Box::new(sanitize_ast(*func)),
            arg: Box::new(sanitize_ast(*arg)),
            continuation: Box::new(sanitize_ast(*continuation)),
        },
        TypeAst::Expression {
            binding_patterns,
            binding_types,
            body,
        } => TypeAst::Expression {
            binding_patterns: binding_patterns.into_iter().map(sanitize_ast).collect(),
            binding_types: binding_types.into_iter().map(sanitize_ast).collect(),
            body: Box::new(sanitize_ast(*body)),
        },
        TypeAst::Match {
            value,
            match_branch,
            else_branch,
        } => TypeAst::Match {
            value: value.map(|value| Box::new(sanitize_ast(*value))),
            match_branch: match_branch
                .into_iter()
                .map(|(pattern, expr)| (sanitize_ast(pattern), sanitize_ast(expr)))
                .collect(),
            else_branch: else_branch.map(|b| Box::new(sanitize_ast(*b))),
        },
        TypeAst::Closure {
            pattern,
            body,
            fail_branch,
        } => TypeAst::Closure {
            pattern: Box::new(sanitize_ast(*pattern)),
            body: Box::new(sanitize_ast(*body)),
            fail_branch: fail_branch.map(|b| Box::new(sanitize_ast(*b))),
        },
        TypeAst::Apply { func, arg } => TypeAst::Apply {
            func: Box::new(sanitize_ast(*func)),
            arg: Box::new(sanitize_ast(*arg)),
        },
        TypeAst::Eq { left, right } => TypeAst::Eq {
            left: Box::new(sanitize_ast(*left)),
            right: Box::new(sanitize_ast(*right)),
        },
        TypeAst::Neq { left, right } => TypeAst::Neq {
            left: Box::new(sanitize_ast(*left)),
            right: Box::new(sanitize_ast(*right)),
        },
        TypeAst::Not { value } => TypeAst::Not {
            value: Box::new(sanitize_ast(*value)),
        },
        TypeAst::FixPoint { param_name, expr } => TypeAst::FixPoint {
            param_name,
            expr: Box::new(sanitize_ast(*expr)),
        },
        TypeAst::Namespace { tag, expr } => TypeAst::Namespace {
            tag,
            expr: Box::new(sanitize_ast(*expr)),
        },
        TypeAst::Pattern { name, expr } => TypeAst::Pattern {
            name,
            expr: Box::new(sanitize_ast(*expr)),
        },
        TypeAst::Literal(inner) => TypeAst::Literal(Box::new(sanitize_ast(*inner))),
        // 基础类型保持不变
        other => other,
    })
}

/// 将 ParseError 转换为友好的单行消息
pub fn perr_to_message(err: &ParseError) -> Option<String> {
    match err {
        ParseError::UseBeforeDeclaration(_, name) => {
            Some(format!("Use of undeclared variable '{}'", name))
        }
        ParseError::RedeclaredPattern(_, name) => {
            Some(format!("Redeclared pattern variable '{}'", name.value()))
        }
        ParseError::UnusedVariable(_, names) => {
            let vars: Vec<String> = names.iter().map(|n| n.value().clone()).collect();
            Some(format!("Unused variables: {}", vars.join(", ")))
        }
        ParseError::AmbiguousPattern(_) => Some("Ambiguous pattern".to_string()),
        ParseError::PatternOutOfParameterDefinition(_) => {
            Some("Pattern out of parameter definition".to_string())
        }
        ParseError::MissingBranch(_) => Some("Missing required branch".to_string()),
        ParseError::InternalError(msg) => Some(format!("Internal error: {}", msg)),
    }
}
