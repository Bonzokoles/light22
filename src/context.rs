use crate::logical::LogicalProperty;
use crate::properties::custom::UnparsedProperty;
use crate::rules::Location;
use crate::rules::supports::{SupportsRule, SupportsCondition};
use crate::rules::{CssRule, CssRuleList, style::StyleRule};
use parcel_selectors::SelectorList;
use crate::selector::{SelectorIdent, SelectorString};
use crate::declaration::{DeclarationBlock, DeclarationList};
use crate::vendor_prefix::VendorPrefix;
use crate::compat::Feature;
use crate::targets::Browsers;
use parcel_selectors::{
  parser::{Selector, Component},
  attr::{AttrSelectorOperator, ParsedCaseSensitivity}
};
use crate::properties::{
  Property,
  PropertyId,
  custom::{CustomProperty, TokenList, Token},
};

#[derive(Debug)]
pub(crate) struct SupportsEntry<'i> {
  pub condition: SupportsCondition<'i>,
  pub declarations: Vec<Property<'i>>,
  pub important_declarations: Vec<Property<'i>>
}

#[derive(Debug, PartialEq)]
pub(crate) enum DeclarationContext {
  None,
  StyleRule,
  Keyframes,
  StyleAttribute
}

#[derive(Debug)]
pub(crate) struct PropertyHandlerContext<'i> {
  targets: Option<Browsers>,
  pub used_logical: bool,
  pub is_important: bool,
  supports: Vec<SupportsEntry<'i>>,
  pub context: DeclarationContext
}

impl<'i> PropertyHandlerContext<'i> {
  pub fn new(targets: Option<Browsers>) -> Self {
    PropertyHandlerContext {
      targets,
      used_logical: false,
      is_important: false,
      supports: Vec::new(),
      context: DeclarationContext::None
    }
  }

  pub fn is_supported(&self, feature: Feature) -> bool {
    // Don't convert logical properties in style attributes because
    // our fallbacks rely on extra rules to define --ltr and --rtl.
    if self.context == DeclarationContext::StyleAttribute {
      return true
    }

    if let Some(targets) = self.targets {
      feature.is_compatible(targets)
    } else {
      true
    }
  }

  pub fn add_logical_property(&mut self, dest: &mut DeclarationList<'i>, property_id: PropertyId<'i>, ltr: Property<'i>, rtl: Property<'i>) {
    self.used_logical = true;
    dest.push(Property::Logical(LogicalProperty {
      property_id,
      ltr: Some(Box::new(ltr)),
      rtl: Some(Box::new(rtl))
    }));
  }

  pub fn add_inline_logical_properties(&mut self, dest: &mut DeclarationList<'i>, left: PropertyId<'i>, right: PropertyId<'i>, start: Option<Property<'i>>, end: Option<Property<'i>>) {
    self.used_logical = true;
    dest.push(Property::Logical(LogicalProperty {
      property_id: left,
      ltr: start.clone().map(|v| Box::new(v)),
      rtl: end.clone().map(|v| Box::new(v)),
    }));

    dest.push(Property::Logical(LogicalProperty {
      property_id: right,
      ltr: end.map(|v| Box::new(v)),
      rtl: start.map(|v| Box::new(v)),
    }));
  }

  pub fn add_logical_rules(&mut self, dest: &mut CssRuleList) {
    // Generate rules for [dir="ltr"] and [dir="rtl"] to define --ltr and --rtl vars.
    macro_rules! style_rule {
      ($dir: ident, $ltr: expr, $rtl: expr) => {
        dest.0.push(CssRule::Style(StyleRule {
          selectors: SelectorList(smallvec::smallvec![
            Selector::from_vec2(vec![
              Component::AttributeInNoNamespace {
                local_name: SelectorIdent("dir".into()),
                operator: AttrSelectorOperator::Equal,
                value: SelectorString(stringify!($dir).into()),
                case_sensitivity: ParsedCaseSensitivity::CaseSensitive,
                never_matches: false
              }
            ])
          ]),
          rules: CssRuleList(vec![]),
          vendor_prefix: VendorPrefix::empty(),
          declarations: DeclarationBlock {
            important_declarations: vec![],
            declarations: vec![
              Property::Custom(CustomProperty {
                name: "--ltr".into(),
                value: TokenList(vec![$ltr.into()])
              }),
              Property::Custom(CustomProperty {
                name: "--rtl".into(),
                value: TokenList(vec![$rtl.into()])
              })
            ]
          },
          loc: Location {
            source_index: 0,
            line: 0,
            column: 0
          }
        }));
      };
    }

    if self.used_logical {
      style_rule!(ltr, Token::Ident("initial".into()), Token::WhiteSpace(" "));
      style_rule!(rtl, Token::WhiteSpace(" "), Token::Ident("initial".into()));
    }
  }

  pub fn add_conditional_property(&mut self, condition: SupportsCondition<'i>, property: Property<'i>) {
    if self.context != DeclarationContext::StyleRule {
      return
    }

    if let Some(entry) = self.supports.iter_mut().find(|supports| condition == supports.condition) {
      if self.is_important {
        entry.important_declarations.push(property);
      } else {
        entry.declarations.push(property);
      }
    } else {
      let mut important_declarations = Vec::new();
      let mut declarations = Vec::new();
      if self.is_important {
        important_declarations.push(property);
      } else {
        declarations.push(property);
      }
      self.supports.push(SupportsEntry {
        condition,
        important_declarations,
        declarations,
      });
    }
  }

  pub fn add_unparsed_fallbacks(&mut self, unparsed: &mut UnparsedProperty<'i>) {
    if self.context != DeclarationContext::StyleRule && self.context != DeclarationContext::StyleAttribute {
      return
    }
    
    if let Some(targets) = self.targets {
      let fallbacks = unparsed.value.get_fallbacks(targets);
      for (condition, fallback) in fallbacks {
        self.add_conditional_property(
          condition,
          Property::Unparsed(UnparsedProperty {
            property_id: unparsed.property_id.clone(),
            value: fallback
          })
        );
      }
    }
  }

  pub fn get_supports_rules(&mut self, style_rule: &StyleRule<'i>) -> Vec<CssRule<'i>> {
    if self.supports.is_empty() {
      return Vec::new()
    }

    let mut dest = Vec::new();
    let supports = std::mem::take(&mut self.supports);
    for entry in supports {
      dest.push(CssRule::Supports(SupportsRule {
        condition: entry.condition,
        rules: CssRuleList(vec![
          CssRule::Style(StyleRule {
            selectors: style_rule.selectors.clone(),
            vendor_prefix: VendorPrefix::None,
            declarations: DeclarationBlock {
              declarations: entry.declarations,
              important_declarations: entry.important_declarations
            },
            rules: CssRuleList(vec![]),
            loc: style_rule.loc.clone()
          })
        ]),
        loc: style_rule.loc.clone()
      }));
    }

    dest
  }
}
