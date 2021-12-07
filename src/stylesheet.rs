use cssparser::{Parser, ParserInput, RuleListParser, ParseError};
use parcel_sourcemap::SourceMap;
use crate::rules::CssRuleList;
use crate::parser::TopLevelRuleParser;
use crate::printer::Printer;
use crate::traits::ToCss;
use crate::targets::Browsers;
use crate::declaration::DeclarationHandler;

pub struct StyleSheet {
  pub filename: String,
  pub rules: CssRuleList
}

impl StyleSheet {
  pub fn empty(filename: String) -> StyleSheet {
    StyleSheet {
      filename,
      rules: CssRuleList(Vec::new())
    }
  }

  pub fn parse<'i>(filename: String, code: &'i str) -> Result<StyleSheet, ParseError<'i, ()>> {
    let mut sheet = StyleSheet::empty(filename);
    sheet.replace(code)?;
    Ok(sheet)
  }

  pub fn replace<'i>(&mut self, code: &'i str) -> Result<(), ParseError<'i, ()>> {
    let mut input = ParserInput::new(&code);
    let mut parser = Parser::new(&mut input);
    let rule_list_parser = RuleListParser::new_for_stylesheet(&mut parser, TopLevelRuleParser {});

    let mut rules = vec![];
    for rule in rule_list_parser {
      let rule = match rule {
        Ok((_, rule)) => rule,
        Err((e, _)) => return Err(e)
      };

      rules.push(rule)
    }

    self.rules = CssRuleList(rules);
    Ok(())
  }

  pub fn minify(&mut self, targets: Option<Browsers>) {
    let mut handler = DeclarationHandler::new(false, targets);
    let mut important_handler = DeclarationHandler::new(true, targets);
    self.rules.minify(targets, &mut handler, &mut important_handler);
  }

  pub fn to_css(&self, minify: bool, source_map: bool, targets: Option<Browsers>) -> Result<(String, Option<SourceMap>), std::fmt::Error> {
    let mut dest = String::new();
    let mut source_map = if source_map {
      let mut sm = SourceMap::new("/");
      sm.add_source(&self.filename);
      Some(sm)
    } else {
      None
    };

    let mut printer = Printer::new(&mut dest, source_map.as_mut(), minify, targets);
    let mut first = true;

    for rule in &self.rules.0 {
      if first {
        first = false;
      } else {
        printer.newline()?;
      }
  
      rule.to_css(&mut printer)?;
      printer.newline()?;
    }

    Ok((dest, source_map))
  }
}
