use cssparser::{Parser, ParserInput, RuleListParser, ParseError};
use parcel_sourcemap::SourceMap;
use crate::rules::{CssRule, CssRuleList, MinifyContext};
use crate::parser::TopLevelRuleParser;
use crate::printer::Printer;
use crate::traits::ToCss;
use crate::targets::Browsers;
use crate::declaration::{DeclarationHandler, DeclarationBlock};
use crate::css_modules::{hash, CssModule, CssModuleExports};
use std::collections::{HashMap, HashSet};
use crate::dependencies::Dependency;
use crate::error::{ParserError, MinifyError, PrinterError};
use crate::logical::LogicalProperties;
use crate::compat::Feature;

pub use crate::parser::ParserOptions;
pub use crate::printer::PseudoClasses;

pub struct StyleSheet {
  pub filename: String,
  pub rules: CssRuleList,
  options: ParserOptions
}

#[derive(Default)]
pub struct PrinterOptions<'a> {
  pub minify: bool,
  pub source_map: bool,
  pub targets: Option<Browsers>,
  pub analyze_dependencies: bool,
  pub pseudo_classes: Option<PseudoClasses<'a>>
}

#[derive(Default)]
pub struct MinifyOptions {
  pub targets: Option<Browsers>,
  pub unused_symbols: HashSet<String>
}

pub struct ToCssResult {
  pub code: String,
  pub source_map: Option<SourceMap>,
  pub exports: Option<CssModuleExports>,
  pub dependencies: Option<Vec<Dependency>>
}

impl StyleSheet {
  pub fn new(filename: String, rules: CssRuleList, options: ParserOptions) -> StyleSheet {
    StyleSheet {
      filename,
      rules,
      options
    }
  }

  pub fn parse<'i>(filename: String, code: &'i str, options: ParserOptions) -> Result<StyleSheet, ParseError<'i, ParserError<'i>>> {
    let mut input = ParserInput::new(&code);
    let mut parser = Parser::new(&mut input);
    let rule_list_parser = RuleListParser::new_for_stylesheet(&mut parser, TopLevelRuleParser::new(&options));

    let mut rules = vec![];
    for rule in rule_list_parser {
      let rule = match rule {
        Ok((_, CssRule::Ignored)) => continue,
        Ok((_, rule)) => rule,
        Err((e, _)) => return Err(e)
      };

      rules.push(rule)
    }

    Ok(StyleSheet {
      filename,
      rules: CssRuleList(rules),
      options
    })
  }

  pub fn minify(&mut self, options: MinifyOptions) -> Result<(), MinifyError> {
    let mut logical_properties = LogicalProperties::new(options.targets);
    let mut handler = DeclarationHandler::new(options.targets);
    let mut important_handler = DeclarationHandler::new(options.targets);

    // @custom-media rules may be defined after they are referenced, but may only be defined at the top level
    // of a stylesheet. Do a pre-scan here and create a lookup table by name.
    let custom_media = if self.options.custom_media && options.targets.is_some() && !Feature::CustomMediaQueries.is_compatible(options.targets.unwrap()) {
      let mut custom_media = HashMap::new();
      for rule in &self.rules.0 {
        if let CssRule::CustomMedia(rule) = rule {
          custom_media.insert(rule.name.clone(), rule.clone());
        }
      }
      Some(custom_media)
    } else {
      None
    };

    self.rules.minify(&mut MinifyContext {
      targets: &options.targets,
      handler: &mut handler,
      important_handler: &mut important_handler,
      logical_properties: &mut logical_properties,
      unused_symbols: &options.unused_symbols,
      custom_media
    }, false)?;
    logical_properties.to_rules(&mut self.rules);
    Ok(())
  }

  pub fn to_css(&self, options: PrinterOptions) -> Result<ToCssResult, PrinterError> {
    let mut dest = String::new();
    let mut source_map = if options.source_map {
      let mut sm = SourceMap::new("/");
      sm.add_source(&self.filename);
      Some(sm)
    } else {
      None
    };

    let mut printer = Printer::new(&self.filename, &mut dest, source_map.as_mut(), options.minify, options.targets);

    let mut dependencies = if options.analyze_dependencies {
      Some(Vec::new())
    } else {
      None
    };

    printer.dependencies = dependencies.as_mut();
    printer.pseudo_classes = options.pseudo_classes;

    if self.options.css_modules {
      let h = hash(&self.filename);
      let mut exports = HashMap::new();
      printer.css_module = Some(CssModule {
        hash: &h,
        exports: &mut exports
      });

      self.rules.to_css(&mut printer)?;
      printer.newline()?;

      Ok(ToCssResult {
        code: dest,
        source_map,
        exports: Some(exports),
        dependencies
      })
    } else {
      self.rules.to_css(&mut printer)?;
      printer.newline()?;
      Ok(ToCssResult {
        code: dest,
        source_map,
        exports: None,
        dependencies
      })
    }
  }
}

pub struct StyleAttribute {
  pub declarations: DeclarationBlock
}

impl StyleAttribute {
  pub fn parse<'i>(code: &'i str) -> Result<StyleAttribute, ParseError<'i, ParserError<'i>>> {
    let mut input = ParserInput::new(&code);
    let mut parser = Parser::new(&mut input);
    let options = ParserOptions::default();
    Ok(StyleAttribute {
      declarations: DeclarationBlock::parse(&mut parser, &options)?
    })
  }

  pub fn minify(&mut self, options: MinifyOptions) {
    let mut logical_properties = LogicalProperties::new(None);
    let mut handler = DeclarationHandler::new(options.targets);
    let mut important_handler = DeclarationHandler::new(options.targets);
    self.declarations.minify(&mut handler, &mut important_handler, &mut logical_properties);
  }

  pub fn to_css(&self, options: PrinterOptions) -> Result<ToCssResult, PrinterError> {
    assert_eq!(options.source_map, false, "Source maps are not supported for style attributes");

    let mut dest = String::new();
    let mut printer = Printer::new("", &mut dest, None, options.minify, options.targets);

    let mut dependencies = if options.analyze_dependencies {
      Some(Vec::new())
    } else {
      None
    };

    printer.dependencies = dependencies.as_mut();

    let len = self.declarations.declarations.len() + self.declarations.important_declarations.len();
    let mut i = 0;

    macro_rules! write {
      ($decls: expr, $important: literal) => {
        for decl in &$decls {
          decl.to_css(&mut printer, $important)?;
          if i != len - 1 {
            printer.write_char(';')?;
            printer.whitespace()?;
          }
          i += 1;
        }
      };
    }

    write!(self.declarations.declarations, false);
    write!(self.declarations.important_declarations, true);

    Ok(ToCssResult {
      code: dest,
      source_map: None,
      exports: None,
      dependencies
    })
  }
}
