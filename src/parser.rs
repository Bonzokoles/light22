use cssparser::*;
use parcel_selectors::{SelectorList, parser::NestingRequirement};
use crate::media_query::*;
use crate::rules::viewport::ViewportRule;
use crate::traits::Parse;
use crate::selector::{Selectors, SelectorParser};
use crate::rules::{
  CssRule,
  CssRuleList,
  keyframes::{KeyframeListParser, KeyframesRule},
  font_face::{FontFaceRule, FontFaceDeclarationParser},
  page::{PageSelector, PageRule},
  supports::{SupportsCondition, SupportsRule},
  counter_style::CounterStyleRule,
  namespace::NamespaceRule,
  import::ImportRule,
  media::MediaRule,
  style::StyleRule,
  document::MozDocumentRule,
  nesting::NestingRule,
  custom_media::CustomMediaRule
};
use crate::values::ident::CustomIdent;
use crate::declaration::{DeclarationBlock, DeclarationList, parse_declaration};
use crate::vendor_prefix::VendorPrefix;
use std::collections::HashMap;
use crate::error::ParserError;

#[derive(Default)]
pub struct ParserOptions {
  pub nesting: bool,
  pub custom_media: bool,
  pub css_modules: bool
}

/// The parser for the top-level rules in a stylesheet.
pub struct TopLevelRuleParser<'a> {
  default_namespace: Option<String>,
  namespace_prefixes: HashMap<String, String>,
  options: &'a ParserOptions
}

impl<'a, 'b> TopLevelRuleParser<'a> {
  pub fn new(options: &'a ParserOptions) -> TopLevelRuleParser<'a> {
    TopLevelRuleParser {
      default_namespace: None,
      namespace_prefixes: HashMap::new(),
      options
    }
  }

  fn nested<'x: 'b>(&'x mut self) -> NestedRuleParser {
      NestedRuleParser {
        default_namespace: &mut self.default_namespace,
        namespace_prefixes: &mut self.namespace_prefixes,
        options: &self.options
      }
  }
}

/// A rule prelude for at-rule with block.
#[derive(Debug)]
#[allow(dead_code)]
pub enum AtRulePrelude {
  /// A @font-face rule prelude.
  FontFace,
  /// A @font-feature-values rule prelude, with its FamilyName list.
  FontFeatureValues,//(Vec<FamilyName>),
  /// A @counter-style rule prelude, with its counter style name.
  CounterStyle(CustomIdent),
  /// A @media rule prelude, with its media queries.
  Media(MediaList),
  /// A @custom-media rule prelude.
  CustomMedia(String, MediaList),
  /// An @supports rule, with its conditional
  Supports(SupportsCondition),
  /// A @viewport rule prelude.
  Viewport(VendorPrefix),
  /// A @keyframes rule, with its animation name and vendor prefix if exists.
  Keyframes(CustomIdent, VendorPrefix),
  /// A @page rule prelude.
  Page(Vec<PageSelector>),
  /// A @-moz-document rule.
  MozDocument,
  /// A @import rule prelude.
  Import(String, MediaList, Option<SupportsCondition>),
  /// A @namespace rule prelude.
  Namespace(Option<String>, String),
  /// A @charset rule prelude.
  Charset,
  /// A @nest prelude.
  Nest(SelectorList<Selectors>)
}

impl<'a, 'i> AtRuleParser<'i> for TopLevelRuleParser<'a> {
  type Prelude = AtRulePrelude;
  type AtRule = (SourcePosition, CssRule);
  type Error = ParserError<'i>;

  fn parse_prelude<'t>(
      &mut self,
      name: CowRcStr<'i>,
      input: &mut Parser<'i, 't>,
  ) -> Result<AtRulePrelude, ParseError<'i, Self::Error>> {
      match_ignore_ascii_case! { &*name,
        "import" => {
          let url_string = input.expect_url_or_string()?.as_ref().to_owned();
          let supports = if input.try_parse(|input| input.expect_function_matching("supports")).is_ok() {
            Some(input.parse_nested_block(|input| {
              input.try_parse(SupportsCondition::parse).or_else(|_| SupportsCondition::parse_declaration(input))
            })?)
          } else {
            None
          };
          let media = MediaList::parse(input);
          return Ok(AtRulePrelude::Import(url_string, media, supports));
        },
        "namespace" => {
          let prefix = input.try_parse(|input| input.expect_ident_cloned()).map(|v| v.as_ref().to_owned()).ok();
          let namespace = input.expect_url_or_string()?.as_ref().to_owned();
          let prelude = AtRulePrelude::Namespace(prefix, namespace);
          return Ok(prelude);
        },
        "charset" => {
          // @charset is removed by rust-cssparser if it’s the first rule in the stylesheet.
          // Anything left is technically invalid, however, users often concatenate CSS files
          // together, so we are more lenient and simply ignore @charset rules in the middle of a file.
          input.expect_string()?;
          return Ok(AtRulePrelude::Charset)
        },
        "custom-media" if self.options.custom_media => {
          let name = input.expect_ident()?.as_ref().to_owned();
          if !name.starts_with("--") {
            return Err(input.new_unexpected_token_error(Token::Ident(name.into())));
          }
          let media = MediaList::parse(input);
          return Ok(AtRulePrelude::CustomMedia(name, media))
        },
        _ => {}
      }

      AtRuleParser::parse_prelude(&mut self.nested(), name, input)
  }

  #[inline]
  fn parse_block<'t>(
      &mut self,
      prelude: AtRulePrelude,
      start: &ParserState,
      input: &mut Parser<'i, 't>,
  ) -> Result<Self::AtRule, ParseError<'i, Self::Error>> {
    let rule = AtRuleParser::parse_block(&mut self.nested(), prelude, start, input)?;
    Ok((start.position(), rule))
  }

  #[inline]
  fn rule_without_block(
      &mut self,
      prelude: AtRulePrelude,
      start: &ParserState,
  ) -> Result<Self::AtRule, ()> {
      let loc = start.source_location();
      let rule = match prelude {
        AtRulePrelude::Import(url, media, supports) => {
          CssRule::Import(ImportRule {
            url,
            supports,
            media,
            loc
          })
        },
        AtRulePrelude::Namespace(prefix, url) => {
          if let Some(prefix) = &prefix {
            self.namespace_prefixes.insert(prefix.clone(), url.clone());
          } else {
            self.default_namespace = Some(url.clone());
          }

          CssRule::Namespace(NamespaceRule {
            prefix,
            url,
            loc
          })
        },
        AtRulePrelude::CustomMedia(name, query) => {
          CssRule::CustomMedia(CustomMediaRule {
            name,
            query,
            loc
          })
        },
        AtRulePrelude::Charset => CssRule::Ignored,
        _ => return Err(())
      };

      Ok((start.position(), rule))
  }
}

impl<'a, 'i> QualifiedRuleParser<'i> for TopLevelRuleParser<'a> {
  type Prelude = SelectorList<Selectors>;
  type QualifiedRule = (SourcePosition, CssRule);
  type Error = ParserError<'i>;

  #[inline]
  fn parse_prelude<'t>(
      &mut self,
      input: &mut Parser<'i, 't>,
  ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
    QualifiedRuleParser::parse_prelude(&mut self.nested(), input)
  }

  #[inline]
  fn parse_block<'t>(
      &mut self,
      prelude: Self::Prelude,
      start: &ParserState,
      input: &mut Parser<'i, 't>,
  ) -> Result<Self::QualifiedRule, ParseError<'i, Self::Error>> {
    let rule = QualifiedRuleParser::parse_block(&mut self.nested(), prelude, start, input)?;
    Ok((start.position(), rule))
  }
}

#[derive(Clone)]
struct NestedRuleParser<'a> {
  default_namespace: &'a Option<String>,
  namespace_prefixes: &'a HashMap<String, String>,
  options: &'a ParserOptions
}

impl<'a, 'b> NestedRuleParser<'a> {
  fn parse_nested_rules(&mut self, input: &mut Parser) -> CssRuleList {
    let nested_parser = NestedRuleParser {
      default_namespace: self.default_namespace,
      namespace_prefixes: self.namespace_prefixes,
      options: self.options
    };

    let mut iter = RuleListParser::new_for_nested_rule(input, nested_parser);
    let mut rules = Vec::new();
    while let Some(result) = iter.next() {
      match result {
        Ok(CssRule::Ignored) => {},
        Ok(rule) => rules.push(rule),
        Err(_) => {
          // TODO
        },
      }
    }

    CssRuleList(rules)
  }
}

impl<'a, 'b, 'i> AtRuleParser<'i> for NestedRuleParser<'a> {
  type Prelude = AtRulePrelude;
  type AtRule = CssRule;
  type Error = ParserError<'i>;

  fn parse_prelude<'t>(
      &mut self,
      name: CowRcStr<'i>,
      input: &mut Parser<'i, 't>,
  ) -> Result<AtRulePrelude, ParseError<'i, Self::Error>> {
    match_ignore_ascii_case! { &*name,
      "media" => {
        let media = MediaList::parse(input);
        Ok(AtRulePrelude::Media(media))
      },
      "supports" => {
        let cond = SupportsCondition::parse(input)?;
        Ok(AtRulePrelude::Supports(cond))
      },
      "font-face" => {
        Ok(AtRulePrelude::FontFace)
      },
      // "font-feature-values" => {
      //     if !cfg!(feature = "gecko") {
      //         // Support for this rule is not fully implemented in Servo yet.
      //         return Err(input.new_custom_error(StyleParseErrorKind::UnsupportedAtRule(name.clone())))
      //     }
      //     let family_names = parse_family_name_list(self.context, input)?;
      //     Ok(AtRuleType::WithBlock(AtRuleBlockPrelude::FontFeatureValues(family_names)))
      // },
      "counter-style" => {
        let name = CustomIdent::parse(input)?;
        Ok(AtRulePrelude::CounterStyle(name))
      },
      "viewport" | "-ms-viewport" => {
        let prefix = if starts_with_ignore_ascii_case(&*name, "-ms") {
          VendorPrefix::Ms
        } else {
          VendorPrefix::None
        };
        Ok(AtRulePrelude::Viewport(prefix))
      },
      "keyframes" | "-webkit-keyframes" | "-moz-keyframes" | "-o-keyframes" | "-ms-keyframes" => {
        let prefix = if starts_with_ignore_ascii_case(&*name, "-webkit-") {
          VendorPrefix::WebKit
        } else if starts_with_ignore_ascii_case(&*name, "-moz-") {
          VendorPrefix::Moz
        } else if starts_with_ignore_ascii_case(&*name, "-o-") {
          VendorPrefix::O
        } else if starts_with_ignore_ascii_case(&*name, "-ms-") {
          VendorPrefix::Ms
        } else {
          VendorPrefix::None
        };

        let location = input.current_source_location();
        let name = match *input.next()? {
          Token::Ident(ref s) => s.as_ref(),
          Token::QuotedString(ref s) => s.as_ref(),
          ref t => return Err(location.new_unexpected_token_error(t.clone())),
        };

        Ok(AtRulePrelude::Keyframes(CustomIdent(name.into()), prefix))
      },
      "page" => {
        let selectors = input.try_parse(|input| input.parse_comma_separated(PageSelector::parse)).unwrap_or_default();
        Ok(AtRulePrelude::Page(selectors))
      },
      "-moz-document" => {
        // Firefox only supports the url-prefix() function with no arguments as a legacy CSS hack.
        // See https://css-tricks.com/snippets/css/css-hacks-targeting-firefox/
        input.expect_function_matching("url-prefix")?;
        input.parse_nested_block(|input| input.expect_exhausted().map_err(|e| e.into()))?;

        Ok(AtRulePrelude::MozDocument)
      },
      _ => Err(input.new_error(BasicParseErrorKind::AtRuleInvalid(name)))
    }
  }

  fn parse_block<'t>(
    &mut self,
    prelude: AtRulePrelude,
    start: &ParserState,
    input: &mut Parser<'i, 't>,
  ) -> Result<CssRule, ParseError<'i, Self::Error>> {
    let loc = start.source_location();
    match prelude {
      AtRulePrelude::FontFace => {
        let mut parser = DeclarationListParser::new(input, FontFaceDeclarationParser);
        let mut properties = vec![];
        while let Some(decl) = parser.next() {
          if let Ok(decl) = decl {
            properties.push(decl);
          }
        }
        Ok(CssRule::FontFace(FontFaceRule {
          properties,
          loc
        }))
      },
      // AtRuleBlockPrelude::FontFeatureValues(family_names) => {
      //     let context = ParserContext::new_with_rule_type(
      //         self.context,
      //         CssRuleType::FontFeatureValues,
      //         self.namespaces,
      //     );

      //     Ok(CssRule::FontFeatureValues(Arc::new(self.shared_lock.wrap(
      //         FontFeatureValuesRule::parse(
      //             &context,
      //             input,
      //             family_names,
      //             start.source_location(),
      //         ),
      //     ))))
      // },
      AtRulePrelude::CounterStyle(name) => {
        Ok(CssRule::CounterStyle(CounterStyleRule {
          name,
          declarations: DeclarationBlock::parse(input, self.options)?,
          loc
        }))
      },
      AtRulePrelude::Media(query) => {
        Ok(CssRule::Media(MediaRule {
          query,
          rules: self.parse_nested_rules(input),
          loc
        }))
      },
      AtRulePrelude::Supports(condition) => {
        Ok(CssRule::Supports(SupportsRule {
          condition,
          rules: self.parse_nested_rules(input),
          loc
        }))
      },
      AtRulePrelude::Viewport(vendor_prefix) => {
        Ok(CssRule::Viewport(ViewportRule {
          vendor_prefix,
          // TODO: parse viewport descriptors rather than properties
          // https://drafts.csswg.org/css-device-adapt/#viewport-desc
          declarations: DeclarationBlock::parse(input, self.options)?,
          loc
        }))
      },
      AtRulePrelude::Keyframes(name, vendor_prefix) => {
        let iter = RuleListParser::new_for_nested_rule(input, KeyframeListParser);
        Ok(CssRule::Keyframes(KeyframesRule {
          name,
          keyframes: iter.filter_map(Result::ok).collect(),
          vendor_prefix,
          loc
        }))
      },
      AtRulePrelude::Page(selectors) => {
        Ok(CssRule::Page(PageRule {
          selectors,
          declarations: DeclarationBlock::parse(input, self.options)?,
          loc
        }))
      },
      AtRulePrelude::MozDocument => {
        Ok(CssRule::MozDocument(MozDocumentRule {
          rules: self.parse_nested_rules(input),
          loc
        }))
      },
      // _ => Ok()
      _ => {
        println!("{:?}", prelude);
        unreachable!()
      }
    }
  }
}

impl<'a, 'b, 'i> QualifiedRuleParser<'i> for NestedRuleParser<'a> {
  type Prelude = SelectorList<Selectors>;
  type QualifiedRule = CssRule;
  type Error = ParserError<'i>;

  fn parse_prelude<'t>(
    &mut self,
    input: &mut Parser<'i, 't>,
  ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
    let selector_parser = SelectorParser {
      default_namespace: self.default_namespace,
      namespace_prefixes: self.namespace_prefixes,
      is_nesting_allowed: false,
      css_modules: self.options.css_modules
    };
    SelectorList::parse(&selector_parser, input, NestingRequirement::None)
  }

  fn parse_block<'t>(
    &mut self,
    selectors: Self::Prelude,
    start: &ParserState,
    input: &mut Parser<'i, 't>,
  ) -> Result<CssRule, ParseError<'i, Self::Error>> {
    let loc = start.source_location();
    let (declarations, rules) = if self.options.nesting {
      parse_declarations_and_nested_rules(input, self.default_namespace, self.namespace_prefixes, self.options)?
    } else {
      (DeclarationBlock::parse(input, self.options)?, CssRuleList(vec![]))
    };
    Ok(CssRule::Style(StyleRule {
      selectors,
      vendor_prefix: VendorPrefix::empty(),
      declarations,
      rules,
      loc
    }))
  }
}

fn parse_declarations_and_nested_rules<'a, 'i, 't>(
  input: &mut Parser<'i, 't>,
  default_namespace: &'a Option<String>,
  namespace_prefixes: &'a HashMap<String, String>,
  options: &'a ParserOptions
) -> Result<(DeclarationBlock, CssRuleList), ParseError<'i, ParserError<'i>>> {
  let mut important_declarations = DeclarationList::new();
  let mut declarations = DeclarationList::new();
  let mut rules = CssRuleList(vec![]);
  let parser = StyleRuleParser {
    default_namespace,
    namespace_prefixes,
    options,
    declarations: &mut declarations,
    important_declarations: &mut important_declarations,
    rules: &mut rules
  };

  let mut declaration_parser = DeclarationListParser::new(input, parser);
  let mut last = declaration_parser.input.state();
  while let Some(decl) = declaration_parser.next() {
    match decl {
      Ok(_) => {}
      _ => {
        declaration_parser.input.reset(&last);
        break
      }
    }

    last = declaration_parser.input.state();
  }

  let mut iter = RuleListParser::new_for_nested_rule(declaration_parser.input, declaration_parser.parser);
  while let Some(result) = iter.next() {
    if let Err((err, _)) = result {
      return Err(err)
    }
  }

  Ok((DeclarationBlock { declarations, important_declarations }, rules))
}

pub struct StyleRuleParser<'a> {
  default_namespace: &'a Option<String>,
  namespace_prefixes: &'a HashMap<String, String>,
  options: &'a ParserOptions,
  declarations: &'a mut DeclarationList,
  important_declarations: &'a mut DeclarationList,
  rules: &'a mut CssRuleList
}

/// Parse a declaration within {} block: `color: blue`
impl<'a, 'i> cssparser::DeclarationParser<'i> for StyleRuleParser<'a> {
  type Declaration = ();
  type Error = ParserError<'i>;

  fn parse_value<'t>(
    &mut self,
    name: CowRcStr<'i>,
    input: &mut cssparser::Parser<'i, 't>,
  ) -> Result<Self::Declaration, cssparser::ParseError<'i, Self::Error>> {
    if !self.rules.0.is_empty() {
      // Declarations cannot come after nested rules.
      return Err(input.new_custom_error(ParserError::InvalidNesting))
    }
    parse_declaration(name, input, &mut self.declarations, &mut self.important_declarations, &self.options)
  }
}

impl<'a, 'i> AtRuleParser<'i> for StyleRuleParser<'a> {
  type Prelude = AtRulePrelude;
  type AtRule = ();
  type Error = ParserError<'i>;

  fn parse_prelude<'t>(
    &mut self,
    name: CowRcStr<'i>,
    input: &mut Parser<'i, 't>,
  ) -> Result<AtRulePrelude, ParseError<'i, Self::Error>> {
    match_ignore_ascii_case! { &*name,
      "media" => {
        let media = MediaList::parse(input);
        Ok(AtRulePrelude::Media(media))
      },
      "supports" => {
        let cond = SupportsCondition::parse(input)?;
        Ok(AtRulePrelude::Supports(cond))
      },
      "nest" => {
        let selector_parser = SelectorParser {
          default_namespace: self.default_namespace,
          namespace_prefixes: self.namespace_prefixes,
          is_nesting_allowed: true,
          css_modules: self.options.css_modules
        };
        let selectors = SelectorList::parse(&selector_parser, input, NestingRequirement::Contained)?;
        Ok(AtRulePrelude::Nest(selectors))
      },
      _ => Err(input.new_error(BasicParseErrorKind::AtRuleInvalid(name)))
    }
  }

  fn parse_block<'t>(
    &mut self,
    prelude: AtRulePrelude,
    start: &ParserState,
    input: &mut Parser<'i, 't>,
  ) -> Result<(), ParseError<'i, Self::Error>> {
    let loc = start.source_location();
    match prelude {
      AtRulePrelude::Media(query) => {
        self.rules.0.push(CssRule::Media(MediaRule {
          query,
          rules: parse_nested_at_rule(input, self.default_namespace, self.namespace_prefixes, self.options)?,
          loc
        }));
        Ok(())
      },
      AtRulePrelude::Supports(condition) => {
        self.rules.0.push(CssRule::Supports(SupportsRule {
          condition,
          rules: parse_nested_at_rule(input, self.default_namespace, self.namespace_prefixes, self.options)?,
          loc
        }));
        Ok(())
      },
      AtRulePrelude::Nest(selectors) => {
        let (declarations, rules) = parse_declarations_and_nested_rules(input, self.default_namespace, self.namespace_prefixes, self.options)?;
        self.rules.0.push(CssRule::Nesting(NestingRule {
          style: StyleRule {
            selectors,
            declarations,
            vendor_prefix: VendorPrefix::empty(),
            rules,
            loc
          },
          loc
        }));
        Ok(())
      },
      _ => {
        println!("{:?}", prelude);
        unreachable!()
      }
    }
  }
}

#[inline]
fn parse_nested_at_rule<'a, 'i, 't>(
  input: &mut Parser<'i, 't>,
  default_namespace: &'a Option<String>,
  namespace_prefixes: &'a HashMap<String, String>,
  options: &'a ParserOptions
) -> Result<CssRuleList, ParseError<'i, ParserError<'i>>> {
  let loc = input.current_source_location();

  // Declarations can be immediately within @media and @supports blocks that are nested within a parent style rule.
  // These act the same way as if they were nested within a `& { ... }` block.
  let (declarations, mut rules) = parse_declarations_and_nested_rules(input, default_namespace, namespace_prefixes, options)?;

  if declarations.declarations.len() > 0 {
    rules.0.insert(0, CssRule::Style(StyleRule {
      selectors: SelectorList(smallvec::smallvec![parcel_selectors::parser::Selector::from_vec2(vec![parcel_selectors::parser::Component::Nesting])]),
      declarations,
      vendor_prefix: VendorPrefix::empty(),
      rules: CssRuleList(vec![]),
      loc
    }))
  }

  Ok(rules)
}

impl<'a, 'b, 'i> QualifiedRuleParser<'i> for StyleRuleParser<'a> {
  type Prelude = SelectorList<Selectors>;
  type QualifiedRule = ();
  type Error = ParserError<'i>;

  fn parse_prelude<'t>(
    &mut self,
    input: &mut Parser<'i, 't>,
  ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
    let selector_parser = SelectorParser {
      default_namespace: self.default_namespace,
      namespace_prefixes: self.namespace_prefixes,
      is_nesting_allowed: true,
      css_modules: self.options.css_modules
    };
    SelectorList::parse(&selector_parser, input, NestingRequirement::Prefixed)
  }

  fn parse_block<'t>(
    &mut self,
    selectors: Self::Prelude,
    start: &ParserState,
    input: &mut Parser<'i, 't>,
  ) -> Result<(), ParseError<'i, Self::Error>> {
    let loc = start.source_location();
    let (declarations, rules) = parse_declarations_and_nested_rules(input, self.default_namespace, self.namespace_prefixes, self.options)?;
    self.rules.0.push(CssRule::Style(StyleRule {
      selectors,
      vendor_prefix: VendorPrefix::empty(),
      declarations,
      rules,
      loc
    }));
    Ok(())
  }
}

fn starts_with_ignore_ascii_case(string: &str, prefix: &str) -> bool {
  string.len() >= prefix.len() && string.as_bytes()[0..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}
