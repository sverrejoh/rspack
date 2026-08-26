use rspack_core::{
  BoxDependency, ContextMode, ContextOptions, DependencyCategory, get_context,
  try_convert_str_to_context_mode,
};
use rspack_regex::RspackRegex;
use rspack_util::SpanExt;
use swc_next_ecma_ast::{CallExpression, GetSpan};

use super::JavascriptParserPlugin;
use crate::{
  dependency::RequireContextDependency,
  visitors::{JavascriptParser, clean_regexp_in_context_module, default_context_reg_exp},
};

pub struct RequireContextDependencyParserPlugin;

#[rspack_macros::implemented_javascript_parser_hooks]
impl<'p, 'a> JavascriptParserPlugin<'p, 'a> for RequireContextDependencyParserPlugin {
  fn call(
    &self,
    parser: &mut JavascriptParser<'p>,
    expr: CallExpression,
    for_name: &str,
  ) -> Option<bool> {
    if for_name != "require.context" {
      return None;
    }
    let ast = parser.ast.ast;
    let arguments = expr.arguments(ast);

    let mode = if arguments.len() == 4 {
      let mode_expr = parser.evaluate_expression(arguments.get_node(ast, 3)?.as_expr(ast)?);
      if !mode_expr.is_string() {
        // FIXME: return `None` in webpack
        ContextMode::Sync
      } else if let Some(mode_expr) = try_convert_str_to_context_mode(mode_expr.string()) {
        mode_expr
      } else {
        ContextMode::Sync
      }
    } else {
      ContextMode::Sync
    };

    let (reg_exp, reg_exp_span) = if arguments.len() >= 3 {
      let argument = arguments.get_node(ast, 2)?.as_expr(ast)?;
      let reg_exp_expr = parser.evaluate_expression(argument);
      let reg_exp = if !reg_exp_expr.is_regexp() {
        // FIXME: return `None` in webpack
        default_context_reg_exp()
      } else {
        let (expr, flags) = reg_exp_expr.regexp();
        RspackRegex::with_flags(expr.as_str(), flags.as_str()).expect("reg should success")
      };
      (reg_exp, Some(argument.span(ast).into()))
    } else {
      (default_context_reg_exp(), None)
    };

    let recursive = if arguments.len() >= 2 {
      let recursive_expr = parser.evaluate_expression(arguments.get_node(ast, 1)?.as_expr(ast)?);
      if !recursive_expr.is_bool() {
        // FIXME: return `None` in webpack
        true
      } else {
        recursive_expr.bool()
      }
    } else {
      true
    };

    if let Some(arg) = arguments.get_node(ast, 0).and_then(|arg| arg.as_expr(ast)) {
      let request_expr = parser.evaluate_expression(arg);
      if !request_expr.is_string() {
        return None;
      }

      let reg_exp = clean_regexp_in_context_module(reg_exp, reg_exp_span, parser);
      parser.add_dependency(BoxDependency::new(RequireContextDependency::new(
        ContextOptions {
          mode,
          recursive,
          pattern: reg_exp.into(),
          category: DependencyCategory::CommonJS,
          request: request_expr.string().to_owned(),
          context: get_context(parser.resource_data).to_string(),
          compiler_context: parser.compiler_options.context.clone(),
          start: expr.span(ast).real_lo(),
          end: expr.span(ast).real_hi(),
          ..Default::default()
        },
        expr.span(ast).into(),
        parser.in_try,
      )));
      return Some(true);
    }

    None
  }
}
