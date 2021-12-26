use cssparser::SourceLocation;
use crate::media_query::MediaList;
use crate::traits::ToCss;
use crate::printer::Printer;
use super::CssRuleList;
use crate::declaration::DeclarationHandler;
use crate::targets::Browsers;
use crate::rules::{ToCssWithContext, StyleContext};

#[derive(Debug, PartialEq)]
pub struct MediaRule {
  pub query: MediaList,
  pub rules: CssRuleList,
  pub loc: SourceLocation
}

impl MediaRule {
  pub(crate) fn minify(&mut self, targets: Option<Browsers>, handler: &mut DeclarationHandler, important_handler: &mut DeclarationHandler) {
    self.rules.minify(targets, handler, important_handler)
  }
}

impl ToCssWithContext for MediaRule {
  fn to_css_with_context<W>(&self, dest: &mut Printer<W>, context: Option<&StyleContext>) -> std::fmt::Result where W: std::fmt::Write {
    dest.add_mapping(self.loc);
    dest.write_str("@media ")?;
    self.query.to_css(dest)?;
    dest.whitespace()?;
    dest.write_char('{')?;
    dest.indent();
    dest.newline()?;
    self.rules.to_css_with_context(dest, context)?;
    dest.dedent();
    dest.newline()?;
    dest.write_char('}')
  }
}
