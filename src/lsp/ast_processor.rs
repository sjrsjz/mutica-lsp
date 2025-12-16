use mutica::mutica_compiler::parser::ParseError;

/// 将 ParseError 转换为友好的单行消息
pub fn perr_to_message(err: &ParseError) -> Option<String> {
    match err {
        ParseError::UseBeforeDeclaration(_, name) => {
            Some(format!("Use of undeclared variable '{}'", name))
        }
        ParseError::RedeclaredCaptureValue(_, name) => {
            Some(format!("Redeclared capture variable '{}'", name.value()))
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
        ParseError::OutgoingFixPointReference(_, _, _) => {
            Some("Outgoing fix-point reference".to_string())
        }
        ParseError::WildcardOutOfConstraint(_) => Some("Wildcard out of constraint".to_string()),
        ParseError::AstNotDesugared(_) => Some("AST not desugared".to_string()),
    }
}
