use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use codespan_reporting::files::SimpleFiles;
use simplexpr::SimplExpr;

use super::{
    file_provider::{FilesError, YuckFiles},
    script_var_definition::ScriptVarDefinition,
    validate::ValidationError,
    var_definition::VarDefinition,
    widget_definition::WidgetDefinition,
    widget_use::WidgetUse,
    window_definition::WindowDefinition,
};
use crate::{
    config::script_var_definition::{ListenScriptVar, PollScriptVar},
    error::{AstError, AstResult, OptionAstErrorExt},
    parser::{
        ast::Ast,
        ast_iterator::AstIterator,
        from_ast::{FromAst, FromAstElementContent},
    },
};
use eww_shared_util::{AttrName, Span, VarName};

pub static TOP_LEVEL_DEFINITION_NAMES: &[&str] = &[
    WidgetDefinition::ELEMENT_NAME,
    WindowDefinition::ELEMENT_NAME,
    VarDefinition::ELEMENT_NAME,
    ListenScriptVar::ELEMENT_NAME,
    PollScriptVar::ELEMENT_NAME,
    Include::ELEMENT_NAME,
];

#[derive(Debug, PartialEq, Eq, Clone, serde::Serialize)]
pub struct Include {
    pub path: String,
    pub path_span: Span,
}

impl FromAstElementContent for Include {
    const ELEMENT_NAME: &'static str = "include";

    fn from_tail<I: Iterator<Item = Ast>>(span: Span, mut iter: AstIterator<I>) -> AstResult<Self> {
        let (path_span, path) = iter.expect_literal()?;
        iter.expect_done()?;
        Ok(Include { path: path.to_string(), path_span })
    }
}

pub enum TopLevel {
    Include(Include),
    VarDefinition(VarDefinition),
    ScriptVarDefinition(ScriptVarDefinition),
    WidgetDefinition(WidgetDefinition),
    WindowDefinition(WindowDefinition),
}

impl FromAst for TopLevel {
    fn from_ast(e: Ast) -> AstResult<Self> {
        let span = e.span();
        let mut iter = e.try_ast_iter()?;
        let (sym_span, element_name) = iter.expect_symbol()?;
        Ok(match element_name.as_str() {
            x if x == Include::ELEMENT_NAME => Self::Include(Include::from_tail(span, iter)?),
            x if x == WidgetDefinition::ELEMENT_NAME => Self::WidgetDefinition(WidgetDefinition::from_tail(span, iter)?),
            x if x == VarDefinition::ELEMENT_NAME => Self::VarDefinition(VarDefinition::from_tail(span, iter)?),
            x if x == PollScriptVar::ELEMENT_NAME => {
                Self::ScriptVarDefinition(ScriptVarDefinition::Poll(PollScriptVar::from_tail(span, iter)?))
            }
            x if x == ListenScriptVar::ELEMENT_NAME => {
                Self::ScriptVarDefinition(ScriptVarDefinition::Listen(ListenScriptVar::from_tail(span, iter)?))
            }
            x if x == WindowDefinition::ELEMENT_NAME => Self::WindowDefinition(WindowDefinition::from_tail(span, iter)?),
            x => return Err(AstError::UnknownToplevel(sym_span, x.to_string())),
        })
    }
}

#[derive(Debug, PartialEq, Eq, Clone, serde::Serialize)]
pub struct Config {
    pub widget_definitions: HashMap<String, WidgetDefinition>,
    pub window_definitions: HashMap<String, WindowDefinition>,
    pub var_definitions: HashMap<VarName, VarDefinition>,
    pub script_vars: HashMap<VarName, ScriptVarDefinition>,
}

impl Config {
    fn append_toplevel(&mut self, files: &mut YuckFiles, toplevel: TopLevel) -> AstResult<()> {
        match toplevel {
            TopLevel::VarDefinition(x) => {
                if self.var_definitions.contains_key(&x.name) || self.script_vars.contains_key(&x.name) {
                    return Err(AstError::ValidationError(ValidationError::VariableDefinedTwice {
                        name: x.name.clone(),
                        span: x.span,
                    }));
                } else {
                    self.var_definitions.insert(x.name.clone(), x);
                }
            }
            TopLevel::ScriptVarDefinition(x) => {
                if self.var_definitions.contains_key(x.name()) || self.script_vars.contains_key(x.name()) {
                    return Err(AstError::ValidationError(ValidationError::VariableDefinedTwice {
                        name: x.name().clone(),
                        span: x.name_span(),
                    }));
                } else {
                    self.script_vars.insert(x.name().clone(), x);
                }
            }
            TopLevel::WidgetDefinition(x) => {
                self.widget_definitions.insert(x.name.clone(), x);
            }
            TopLevel::WindowDefinition(x) => {
                self.window_definitions.insert(x.name.clone(), x);
            }
            TopLevel::Include(include) => {
                let (file_id, toplevels) = files.load_file(PathBuf::from(&include.path)).map_err(|err| match err {
                    FilesError::IoError(_) => AstError::IncludedFileNotFound(include),
                    FilesError::AstError(x) => x,
                })?;
                for element in toplevels {
                    self.append_toplevel(files, TopLevel::from_ast(element)?)?;
                }
            }
        }
        Ok(())
    }

    pub fn generate(files: &mut YuckFiles, elements: Vec<Ast>) -> AstResult<Self> {
        let mut config = Self {
            widget_definitions: HashMap::new(),
            window_definitions: HashMap::new(),
            var_definitions: HashMap::new(),
            script_vars: HashMap::new(),
        };
        for element in elements {
            config.append_toplevel(files, TopLevel::from_ast(element)?)?;
        }
        Ok(config)
    }

    pub fn generate_from_main_file(files: &mut YuckFiles, path: impl AsRef<Path>) -> AstResult<Self> {
        let (span, top_levels) = files.load_file(path.as_ref().to_path_buf()).map_err(|err| match err {
            FilesError::IoError(err) => AstError::Other(Span::DUMMY, Box::new(err)),
            FilesError::AstError(x) => x,
        })?;
        Self::generate(files, top_levels)
    }
}
