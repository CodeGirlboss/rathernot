use anyhow::Context;
use serde::{Deserialize, Serialize};
use swc_plugin::{ast::*, plugin_transform, syntax_pos::DUMMY_SP, TransformPluginProgramMetadata};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

mod macros;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct Config {
  // #[serde(default)]
  // ignore: Vec<JsWord>,

  // #[serde(default = "default_prefix_pattern")]
  // prefix_pattern: String,

  #[serde(default)]
  is_client: bool,

  #[serde(default)]
  pub filename: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginContext {
  #[serde(default)]
  pub filename: Option<String>,
}

#[derive(Hash)]
struct ExprsWithFilename<'a> {
  expr:  Option<&'a Expr>,
  call_expr:  Option<&'a CallExpr>,
  filename: String
}

fn calculate_hash<T: Hash>(t: T) -> u64 {
  let mut s = DefaultHasher::new();
  t.hash(&mut s);
  s.finish()
}

impl Default for Config {
  fn default() -> Self {
    serde_json::from_str("{}").unwrap()
  }
}

struct TransformVisitor {
  config: Config,
}

impl TransformVisitor {
  pub fn new(config: Config) -> Self {
    Self { config }
  }

  fn void_zero (&self) -> Expr {
    swc_plugin::ast::Expr::Unary(UnaryExpr {
      span: DUMMY_SP,
      op: UnaryOp::Void,
      arg: Box::new(Expr::Lit(Lit::Num(Number {
        raw: None,
        span: DUMMY_SP,
        value: 0.0
      })))
    })
  }

  fn void_decl (&self) -> Decl {
    Decl::Var( VarDecl {
      span: DUMMY_SP,
      kind: VarDeclKind::Let,
      declare: false,
      decls: Vec::from([VarDeclarator {
        span: DUMMY_SP,
        definite: false,
        name: Pat::Ident(BindingIdent {
          id: Ident {
            span: DUMMY_SP,
            sym: JsWord::from("_RNOS_VOID"),
            optional: false
          },
          type_ann: None
        }),
        init: None
      }])
    })
  }

  fn gen_id_literal (&self, expr: Option<&Expr>, call_expr: Option<&CallExpr>) -> ExprOrSpread {
    let filename = self.config.filename.as_deref().unwrap_or_default();
    let id = calculate_hash(ExprsWithFilename {
      expr: expr,
      call_expr: call_expr,
      filename: filename.to_string()
    });

    ExprOrSpread {
      spread: None,
      expr: Box::new(Expr::Lit(Lit::Num(Number {
        raw: None,
        span: DUMMY_SP,
        value: id as f64
      })))
    }
  }

  // transforms
  // "Service(myobj)" => "_RNOS_CLIENT(15352409104929483000)"
  fn rnos_client_call (&self, id_literal: ExprOrSpread) -> Expr {
    swc_plugin::ast::Expr::Call(CallExpr {
      span: DUMMY_SP,
      callee: Callee::Expr(
        Box::new(
          Expr::Ident(
            Ident::from((JsWord::from("_RNOS_CLIENT"), DUMMY_SP.ctxt))))),
      args: Vec::from([id_literal]),
      type_args: None
    })
  }

  // transforms
  // "Service(myobj)" => "Service._RNOS_SERVER(15352409104929483000, myobj)"
  fn rnos_server_call(&self, id_literal: ExprOrSpread,  service_expr: &CallExpr) -> Expr {
    if let Callee::Expr(callee) = &service_expr.callee {
      let member = swc_plugin::ast::Expr::Member(MemberExpr {
        span: DUMMY_SP,
        prop: MemberProp::Ident(Ident {
          
            span: DUMMY_SP,
            sym: JsWord::from("_RNOS_SERVER"),
            optional: false
          
        }),
        obj: callee.clone()
      });

      let mut args = service_expr.args.clone();

      args.insert(0, id_literal);

      return swc_plugin::ast::Expr::Call(CallExpr {
        span: DUMMY_SP,
        callee: Callee::Expr(Box::new(member)),
        args: args,
        type_args: None
      })
    }
    // we kinda know this won't happen?
    // todo: panic????
    println!("This is a problematic situation: Service was called incorrectly! {:?}", service_expr);
    return self.void_zero();
  }

  fn replace_service_call (&self, service_expr: &CallExpr) -> Expr {
    let service_id_literal = self.gen_id_literal(None, Some(service_expr));//todo: copy
    if self.config.is_client {
      return self.rnos_client_call(service_id_literal)
    } else {
      return self.rnos_server_call(service_id_literal, service_expr)
    }
  }

  // _RNOS_MAIN('slug', <this expr>)
  fn main_invocation (&self, expr: Box<Expr>) -> Expr {
    let literal = self.gen_id_literal(Some(&*expr), None);
    let expr_or_spread = ExprOrSpread {
      spread: None,
      expr: expr
    };
    Expr::Call(CallExpr {
      span: DUMMY_SP,
      callee: Callee::Expr(Box::new(Expr::Ident(Ident {
        span: DUMMY_SP,
        sym: JsWord::from("_RNOS_MAIN"),
        optional: false
      }))),
      args: Vec::from([literal, expr_or_spread]),
      type_args: None
    })
  }

  // let main  = _RNOS_MAIN('slug', <this fn dec>)
  fn replace_main_stmt (&self, expr: Box<Expr>) -> Decl {
    if !self.config.is_client {
      return self.void_decl();
    }
    Decl::Var(VarDecl {
      span: DUMMY_SP,
      kind: VarDeclKind::Let,
      declare: false,
      decls: Vec::from([VarDeclarator {
        span: DUMMY_SP,
        definite: false,
        name: Pat::Ident(BindingIdent {
          id: Ident {
            span: DUMMY_SP,
            sym: JsWord::from("main"),
            optional: false
          },
          type_ann: None
        }),
        init: Some(Box::new(self.main_invocation(expr)))
      }])
    })
  }
}

impl VisitMut for TransformVisitor {
  noop_visit_mut_type!();

  fn visit_mut_expr(&mut self, expr: &mut Expr) {
    expr.visit_mut_children_with(self);
    if_let_chain! {[
      let Expr::Call(call_expr) = &expr,
      let Callee::Expr(callee) = &call_expr.callee,
      let Expr::Ident(id) = &**callee
    ], {
      if &*id.sym == "Service" {
        *expr = self.replace_service_call(&call_expr)
      }
    }};
  }

  // looks for:
  // function main (root) { }
  // produces:
  // let main  = _RNOS_MAIN('slug', function main (root) { })
  fn visit_mut_decl(&mut self, decl: &mut Decl) {
    decl.visit_mut_children_with(self);

    let mut main_stmt = None;
    match decl {
      // function main (root) { }
      Decl::Fn(fn_decl) => {
        if &*fn_decl.ident.sym == "main" {
          main_stmt = Some(self.replace_main_stmt(Box::new(Expr::Fn(FnExpr {
            ident: None,
            function: fn_decl.function.clone()
          }))));
        }
      },
      // var main = (root) => 
      Decl::Var(var_decl) => {
        for var in var_decl.decls.iter() {
          match &var.name {
            Pat::Ident(id_pat) => {
              if &*id_pat.id.sym == "main" {
                if let Some(init) = &var.init {
                  match &**init {
                    Expr::Fn(..) |
                    Expr::Arrow(..) =>
                      main_stmt = Some(self.replace_main_stmt(Box::new(*init.clone()))),
                    _ => ()
                  }
                }
              }
            },
            // destructuring assignments
            // these are hard because we can't resolve the expression on RHS
            // const [ main ] = [ (root) => ... ]
            Pat::Array(..) |
            // const { main } = { main: (root) => ... }
            Pat::Object(..) => (),
            _ => ()
          }
        }
      },
      _ => ()
    }
    if let Some(v) = main_stmt {
      *decl = v;
    }
  }
}

/// An entrypoint to the SWC's transform plugin.
/// `plugin_transform` macro handles necessary interop to communicate with the host,
/// and entrypoint function name (`process_transform`) can be anything else.
///
/// If plugin need to handle low-level ptr directly,
/// it is possible to opt out from macro by writing transform fn manually via raw interface
///
/// `__plugin_process_impl(
///     ast_ptr: *const u8,
///     ast_ptr_len: i32,
///     config_str_ptr: *const u8,
///     config_str_ptr_len: i32) ->
///     i32 /*  0 for success, fail otherwise.
///             Note this is only for internal pointer interop result,
///             not actual transform result */
///
/// However, this means plugin author need to handle all of serialization/deserialization
/// steps with communicating with host. Refer `swc_plugin_macro` for more details.
#[plugin_transform]
pub fn process_transform(program: Program, _metadata: TransformPluginProgramMetadata) -> Program {
  let mut config: Config = serde_json::from_str(&_metadata.plugin_config)
    .context("failed to parse plugin config")
    .unwrap();

  let context: PluginContext = serde_json::from_str(&_metadata.transform_context)
    .context("failed to parse plugin context")
    .unwrap();

  config.filename = context.filename;

  program.fold_with(&mut as_folder(TransformVisitor::new(config)))
}

// I can't get these to compile lol

// #[cfg(test)]
// mod transform_visitor_tests {
//     use swc_ecma_transforms_testing::test;

//     use super::*;

//     fn transform_visitor(config: Config) -> impl 'static + Fold + VisitMut {
//         as_folder(TransformVisitor::new(config))
//     }

//     test!(
//         ::swc_ecma_parser::Syntax::default(),
//         |_| transform_visitor(Config {
//             filename: Some("test.js".to_owned()),
//             ..Default::default()
//         }),
//         adds_default_prefix_when_filename_is_some,
//         r#"console.log("hello world");"#,
//         r#"console.log("test.js", "hello world");"#
//     );

//     test!(
//         ::swc_ecma_parser::Syntax::default(),
//         |_| transform_visitor(Default::default()),
//         doesnt_add_default_prefix_when_filename_is_none,
//         r#"console.log("hello world");"#,
//         r#"console.log("hello world");"#
//     );

//     test!(
//         ::swc_ecma_parser::Syntax::default(),
//         |_| transform_visitor(Config {
//             prefix_pattern: "custom-prefix:".to_owned(),
//             ..Default::default()
//         }),
//         adds_custom_prefix_to_console_logs,
//         r#"console.log("hello world");"#,
//         r#"console.log("custom-prefix:", "hello world");"#
//     );

//     test!(
//         ::swc_ecma_parser::Syntax::default(),
//         |_| transform_visitor(Config {
//             filename: Some("test.js".to_owned()),
//             ..Default::default()
//         }),
//         adds_prefix_when_nested,
//         r#"console.log("hello world", console.log("hello world"));"#,
//         r#"console.log("test.js", "hello world", console.log("test.js", "hello world"));"#
//     );

//     test!(
//         ::swc_ecma_parser::Syntax::default(),
//         |_| transform_visitor(Default::default()),
//         does_not_alter_console_table,
//         r#"console.table(["apples", "oranges", "bananas"]);"#,
//         r#"console.table(["apples", "oranges", "bananas"]);"#
//     );

//     test!(
//         ::swc_ecma_parser::Syntax::default(),
//         |_| transform_visitor(Config {
//             ignore: vec![JsWord::from("log")],
//             ..Default::default()
//         }),
//         ignores_console_members,
//         r#"console.log("hello world");"#,
//         r#"console.log("hello world");"#
//     );

//     test!(
//         ::swc_ecma_parser::Syntax::default(),
//         |_| transform_visitor(Config {
//             prefix_pattern: "file: [filename]".to_owned(),
//             filename: Some("test.js".to_owned()),
//             ..Default::default()
//         }),
//         adds_filename,
//         r#"console.log("hello world");"#,
//         r#"console.log("file: test.js", "hello world");"#
//     );
// }