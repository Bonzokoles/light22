use crate::values::string::CowArcStr;
use cssparser::*;
use crate::traits::{Parse, ToCss, PropertyHandler};
use super::{Property, PropertyId};
use crate::values::{image::Image, ident::CustomIdent};
use crate::declaration::DeclarationList;
use crate::macros::{enum_property, shorthand_property, shorthand_handler};
use crate::printer::Printer;
use crate::error::{ParserError, PrinterError};
use crate::logical::LogicalProperties;

/// https://www.w3.org/TR/2020/WD-css-lists-3-20201117/#text-markers
#[derive(Debug, Clone, PartialEq)]
pub enum ListStyleType<'i> {
  None,
  CounterStyle(CounterStyle<'i>),
  String(CowArcStr<'i>)
}

impl Default for ListStyleType<'_> {
  fn default() -> Self {
    ListStyleType::CounterStyle(CounterStyle::Name(CustomIdent("disc".into())))
  }
}

impl<'i> Parse<'i> for ListStyleType<'i> {
  fn parse<'t>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    if input.try_parse(|input| input.expect_ident_matching("none")).is_ok() {
      return Ok(ListStyleType::None)
    }

    if let Ok(val) = input.try_parse(CounterStyle::parse) {
      return Ok(ListStyleType::CounterStyle(val))
    }

    let s = input.expect_string_cloned()?;
    Ok(ListStyleType::String(s.into()))
  }
}

impl ToCss for ListStyleType<'_> {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError> where W: std::fmt::Write {
    match self {
      ListStyleType::None => dest.write_str("none"),
      ListStyleType::CounterStyle(style) => style.to_css(dest),
      ListStyleType::String(s) => {
        serialize_string(&s, dest)?;
        Ok(())
      }
    }
  }
}

/// https://www.w3.org/TR/css-counter-styles-3/#typedef-counter-style
#[derive(Debug, Clone, PartialEq)]
pub enum CounterStyle<'i> {
  Name(CustomIdent<'i>),
  Symbols(SymbolsType, Vec<Symbol<'i>>)
}

impl<'i> Parse<'i> for CounterStyle<'i> {
  fn parse<'t>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    if input.try_parse(|input| input.expect_function_matching("symbols")).is_ok() {
      return input.parse_nested_block(|input| {
        let t = input.try_parse(SymbolsType::parse).unwrap_or(SymbolsType::Symbolic);

        let mut symbols = Vec::new();
        while let Ok(s) = input.try_parse(Symbol::parse) {
          symbols.push(s);
        }

        Ok(CounterStyle::Symbols(t, symbols))
      })
    }

    let name = CustomIdent::parse(input)?;
    Ok(CounterStyle::Name(name))
  }
}

impl ToCss for CounterStyle<'_> {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError> where W: std::fmt::Write {
    match self {
      CounterStyle::Name(name) => {
        if let Some(css_module) = &mut dest.css_module {
          css_module.reference(&name.0)
        }
        name.to_css(dest)
      },
      CounterStyle::Symbols(t, symbols) => {
        dest.write_str("symbols(")?;
        let mut needs_space = false;
        if *t != SymbolsType::Symbolic {
          t.to_css(dest)?;
          needs_space = true;
        }
        
        for symbol in symbols {
          if needs_space {
            dest.write_char(' ')?;
          }
          symbol.to_css(dest)?;
          needs_space = true;
        }
        dest.write_char(')')
      }
    }
  }
}

enum_property! {
  pub enum SymbolsType {
    Cyclic,
    Numeric,
    Alphabetic,
    Symbolic,
    Fixed,
  }
}

/// https://www.w3.org/TR/css-counter-styles-3/#funcdef-symbols
#[derive(Debug, Clone, PartialEq)]
pub enum Symbol<'i> {
  String(CowArcStr<'i>),
  Image(Image<'i>)
}

impl<'i> Parse<'i> for Symbol<'i> {
  fn parse<'t>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    if let Ok(img) = input.try_parse(Image::parse) {
      return Ok(Symbol::Image(img))
    }

    let s = input.expect_string_cloned()?;
    Ok(Symbol::String(s.into()))
  }
}

impl<'i> ToCss for Symbol<'i> {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError> where W: std::fmt::Write {
    match self {
      Symbol::String(s) => {
        serialize_string(&s, dest)?;
        Ok(())
      },
      Symbol::Image(img) => img.to_css(dest)
    }
  }
}

enum_property! {
  /// https://www.w3.org/TR/2020/WD-css-lists-3-20201117/#list-style-position-property
  pub enum ListStylePosition {
    Inside,
    Outside,
  }
}

impl Default for ListStylePosition {
  fn default() -> ListStylePosition {
    ListStylePosition::Outside
  }
}

enum_property! {
  /// https://www.w3.org/TR/2020/WD-css-lists-3-20201117/#marker-side
  pub enum MarkerSide {
    "match-self": MatchSelf,
    "match-parent": MatchParent,
  }
}

// https://www.w3.org/TR/2020/WD-css-lists-3-20201117/#list-style-property
shorthand_property!(ListStyle<'i> {
  list_style_type: ListStyleType<'i>,
  image: Image<'i>,
  position: ListStylePosition,
});

shorthand_handler!(ListStyleHandler -> ListStyle {
  list_style_type: ListStyleType(ListStyleType<'i>),
  image: ListStyleImage(Image<'i>),
  position: ListStylePosition(ListStylePosition),
});
