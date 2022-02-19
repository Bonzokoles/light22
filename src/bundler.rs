use parcel_sourcemap::SourceMap;
use crate::{rules::{Location, layer::LayerBlockRule}, error::ErrorLocation};
use std::{fs, path::{Path, PathBuf}, sync::Mutex};
use rayon::prelude::*;
use dashmap::DashMap;
use crate::{
  stylesheet::{StyleSheet, ParserOptions},
  rules::{CssRule, CssRuleList,
    media::MediaRule,
    supports::{SupportsRule, SupportsCondition},
    import::ImportRule
  },
  media_query::MediaList,
  error::{Error, ParserError}
};

pub struct Bundler<'a, 's, P> {
  source_map: Option<Mutex<&'s mut SourceMap>>,
  sources: Mutex<Vec<String>>,
  fs: &'a P,
  loaded: DashMap<PathBuf, ImportRule<'a>>,
  stylesheets: DashMap<PathBuf, StyleSheet<'a>>,
  options: ParserOptions
}

pub trait SourceProvider: Send + Sync {
  fn read<'a>(&'a self, file: &Path) -> std::io::Result<&'a str>;
}

pub struct FileProvider {
  inputs: Mutex<Vec<*mut String>>
}

impl FileProvider {
  pub fn new() -> FileProvider {
    FileProvider {
      inputs: Mutex::new(Vec::new()),
    }
  }
}

unsafe impl Sync for FileProvider {}
unsafe impl Send for FileProvider {}

impl SourceProvider for FileProvider {
  fn read<'a>(&'a self, file: &Path) -> std::io::Result<&'a str> {
    let source = fs::read_to_string(file)?;
    let ptr = Box::into_raw(Box::new(source));
    self.inputs.lock().unwrap().push(ptr);
    // SAFETY: this is safe because the pointer is not dropped
    // until the FileProvider is, and we never remove from the
    // list of pointers stored in the vector.
    Ok(unsafe { &*ptr })
  }
}

impl Drop for FileProvider {
  fn drop(&mut self) {
    for ptr in self.inputs.lock().unwrap().iter() {
      std::mem::drop(unsafe { Box::from_raw(*ptr) })
    }
  }
}

#[derive(Debug)]
pub enum BundleErrorKind<'i> {
  IOError(std::io::Error),
  ParserError(ParserError<'i>),
  UnsupportedImportCondition,
  UnsupportedMediaBooleanLogic,
  UnsupportedLayerCombination
}

impl<'i> From<Error<ParserError<'i>>> for Error<BundleErrorKind<'i>> {
  fn from(err: Error<ParserError<'i>>) -> Self {
    Error {
      kind: BundleErrorKind::ParserError(err.kind),
      loc: err.loc
    }
  }
}

impl<'i> BundleErrorKind<'i> {
  pub fn reason(&self) -> String {
    match self {
      BundleErrorKind::IOError(e) => e.to_string(),
      BundleErrorKind::ParserError(e) => e.reason(),
      BundleErrorKind::UnsupportedImportCondition => "Unsupported import condition".into(),
      BundleErrorKind::UnsupportedMediaBooleanLogic => "Unsupported boolean logic in @import media query".into(),
      BundleErrorKind::UnsupportedLayerCombination => "Unsupported layer combination in @import".into()
    }
  }
}

impl<'a, 's, P: SourceProvider> Bundler<'a, 's, P> {
  pub fn new(fs: &'a P, source_map: Option<&'s mut SourceMap>, options: ParserOptions) -> Self {
    Bundler {
      sources: Mutex::new(Vec::new()),
      source_map: source_map.map(Mutex::new),
      fs,
      loaded: DashMap::new(),
      stylesheets: DashMap::new(),
      options
    }
  }

  pub fn bundle<'e>(&mut self, entry: &'e Path) -> Result<StyleSheet<'a>, Error<BundleErrorKind<'a>>> {
    // Phase 1: load and parse all files.
    self.load_file(&entry, ImportRule {
      url: "".into(),
      layer: None,
      supports: None,
      media: MediaList::new(),
      loc: Location {
        source_index: 0,
        line: 1,
        column: 0
      }
    })?;

    // Phase 2: concatenate rules in the right order.
    let mut rules: Vec<CssRule<'a>> = Vec::new();
    self.inline(&entry, &mut rules);
    Ok(StyleSheet::new(
      std::mem::take(self.sources.get_mut().unwrap()),
      CssRuleList(rules), 
      self.options.clone()
    ))
  }

  fn load_file(&self, file: &Path, rule: ImportRule<'a>) -> Result<(), Error<BundleErrorKind<'a>>> {
    use dashmap::mapref::entry::Entry;

    // Check if we already loaded this file. This is stored in a separate
    // map from the stylesheet itself so we don't hold a lock while parsing.
    match self.loaded.entry(file.to_owned()) {
      Entry::Occupied(mut entry) => {
        // If we already loaded this file, combine the media queries and supports conditions
        // from this import rule with the existing ones using a logical or operator.
        let entry = entry.get_mut();

        // We cannot combine a media query and a supports query from different @import rules.
        // e.g. @import "a.css" print; @import "a.css" supports(color: red);
        // This would require duplicating the actual rules in the file.
        if (!rule.media.media_queries.is_empty() && !entry.supports.is_none()) || 
          (!entry.media.media_queries.is_empty() && !rule.supports.is_none()) {
          return Err(Error {
            kind: BundleErrorKind::UnsupportedImportCondition,
            loc: Some(ErrorLocation::from(
              rule.loc, 
              self.sources.lock().unwrap()[rule.loc.source_index as usize].clone()
            ))
          })
        }

        if rule.media.media_queries.is_empty() {
          entry.media.media_queries.clear();
        } else if !entry.media.media_queries.is_empty() {
          entry.media.or(&rule.media);
        }

        if let Some(supports) = rule.supports {
          if let Some(existing_supports) = &mut entry.supports {
            existing_supports.or(&supports)
          }
        } else {
          entry.supports = None;
        }

        if let Some(layer) = &rule.layer {
          if let Some(existing_layer) = &entry.layer {
            // We can't OR layer names without duplicating all of the nested rules, so error for now.
            if layer != existing_layer || (layer.is_none() && existing_layer.is_none()) {
              return Err(Error {
                kind: BundleErrorKind::UnsupportedLayerCombination,
                loc: Some(ErrorLocation::from(
                  rule.loc, 
                  self.sources.lock().unwrap()[rule.loc.source_index as usize].clone()
                ))
              })
            }
          } else {
            entry.layer = rule.layer;
          }
        }
        
        return Ok(());
      }
      Entry::Vacant(entry) => {
        entry.insert(rule.clone());
      }
    }
    
    let filename = file.to_str().unwrap();
    let code = self.fs.read(file).map_err(|e| Error {
      kind: BundleErrorKind::IOError(e),
      loc: Some(ErrorLocation::from(
        rule.loc,
        self.sources.lock().unwrap()[rule.loc.source_index as usize].clone()
      ))
    })?;

    let mut opts = self.options.clone();

    {
      let mut sources = self.sources.lock().unwrap();
      opts.source_index = sources.len() as u32;
      sources.push(filename.into());
    }

    if let Some(source_map) = &self.source_map {
      let mut source_map = source_map.lock().unwrap();
      let source_index = source_map.add_source(filename);
      let _ = source_map.set_source_content(source_index as usize, code);
    }

    let mut stylesheet = StyleSheet::parse(
      filename.into(),
      code,
      opts,
    )?;

    // Collect and load dependencies for this stylesheet in parallel.
    stylesheet.rules.0.par_iter_mut()
      .try_for_each(|r| {
        // Prepend parent layer name to @layer statements.
        if let CssRule::LayerStatement(layer) = r {
          if let Some(Some(parent_layer)) = &rule.layer {
            for name in &mut layer.names {
              name.0.insert_many(0, parent_layer.0.iter().cloned())
            }
          }
        }

        if let CssRule::Import(import) = r {
          let path = file.with_file_name(&*import.url);

          // Combine media queries and supports conditions from parent 
          // stylesheet with @import rule using a logical and operator.
          let mut media = rule.media.clone();
          media.and(&import.media).map_err(|_| Error {
            kind: BundleErrorKind::UnsupportedMediaBooleanLogic,
            loc: Some(ErrorLocation::from(
              import.loc,
              self.sources.lock().unwrap()[import.loc.source_index as usize].clone()
            ))
          })?;

          let layer = if (rule.layer == Some(None) && import.layer.is_some()) || (import.layer == Some(None) && rule.layer.is_some()) {
            // Cannot combine anonymous layers
            return Err(Error {
              kind: BundleErrorKind::UnsupportedLayerCombination,
              loc: Some(ErrorLocation::from(
                import.loc, 
                self.sources.lock().unwrap()[import.loc.source_index as usize].clone()
              ))
            })
          } else if let Some(Some(a)) = &rule.layer {
            if let Some(Some(b)) = &import.layer {
              let mut name = a.clone();
              name.0.extend(b.0.iter().cloned());
              Some(Some(name))
            } else {
              Some(Some(a.clone()))
            }
          } else {
            import.layer.clone()
          };
          
          self.load_file(&path, ImportRule {
            layer,
            media,
            supports: combine_supports(rule.supports.clone(), &import.supports),
            url: "".into(),
            loc: import.loc
          })?
        }
        Ok(())
      })?;

    self.stylesheets.insert(file.to_owned(), stylesheet);
    Ok(())
  }

  fn inline(&self, file: &Path, dest: &mut Vec<CssRule<'a>>) {
    // Retrieve the stylesheet for this file from the map and remove it.
    // If it doesn't exist, then we already inlined it (e.g. circular dep).
    let stylesheet = match self.stylesheets.remove(file) {
      Some((_, s)) => s,
      None => return
    };

    // Wrap rules in the appropriate @media and @supports rules.
    let mut rules: Vec<CssRule<'a>> = stylesheet.rules.0;

    for rule in &mut rules {
      match rule {
        CssRule::Import(import) => {
          let path = file.with_file_name(&*import.url);
          self.inline(&path, dest);
          *rule = CssRule::Ignored;
        },
        CssRule::LayerStatement(_) => {
          // @layer rules are the only rules that may appear before an @import.
          // We must preserve this order to ensure correctness.
          let layer = std::mem::replace(rule, CssRule::Ignored);
          dest.push(layer);
        },
        _ => break
      }
    }

    let (_, loaded) = self.loaded.remove(file).unwrap();
    if !loaded.media.media_queries.is_empty() {
      rules = vec![
        CssRule::Media(MediaRule {
          query: loaded.media,
          rules: CssRuleList(rules),
          loc: loaded.loc
        })
      ]
    }

    if let Some(supports) = loaded.supports {
      rules = vec![
        CssRule::Supports(SupportsRule {
          condition: supports,
          rules: CssRuleList(rules),
          loc: loaded.loc
        })
      ]
    }

    if let Some(layer) = loaded.layer {
      rules = vec![
        CssRule::LayerBlock(LayerBlockRule {
          name: layer,
          rules: CssRuleList(rules),
          loc: loaded.loc
        })
      ]
    }

    dest.extend(rules);
  }
}

fn combine_supports<'a>(a: Option<SupportsCondition<'a>>, b: &Option<SupportsCondition<'a>>) -> Option<SupportsCondition<'a>> {
  if let Some(mut a) = a {
    if let Some(b) = b {
      a.and(b)
    }
    Some(a)
  } else {
    b.clone()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{stylesheet::{PrinterOptions, MinifyOptions}, targets::Browsers};
  use indoc::indoc;
  use std::collections::HashMap;

  struct TestProvider {
    map: HashMap<PathBuf, String>
  }

  impl SourceProvider for TestProvider {
    fn read<'a>(&'a self, file: &Path) -> std::io::Result<&'a str> {
      Ok(self.map.get(file).unwrap())
    }
  }

  macro_rules! fs(
    { $($key:literal: $value:expr),* } => {
      {
        #[allow(unused_mut)]
        let mut m = HashMap::new();
        $(
          m.insert(PathBuf::from($key), $value.to_owned());
        )*
        TestProvider {
          map: m
        }
      }
    };
  );

  fn bundle(fs: TestProvider, entry: &str) -> String {
    let mut bundler = Bundler::new(&fs, None, ParserOptions::default());
    let stylesheet = bundler.bundle(Path::new(entry)).unwrap();
    stylesheet.to_css(PrinterOptions::default()).unwrap().code
  }

  fn bundle_css_module(fs: TestProvider, entry: &str) -> String {
    let mut bundler = Bundler::new(&fs, None, ParserOptions { css_modules: true, ..ParserOptions::default() });
    let stylesheet = bundler.bundle(Path::new(entry)).unwrap();
    stylesheet.to_css(PrinterOptions::default()).unwrap().code
  }

  fn bundle_custom_media(fs: TestProvider, entry: &str) -> String {
    let mut bundler = Bundler::new(&fs, None, ParserOptions { custom_media: true, ..ParserOptions::default() });
    let mut stylesheet = bundler.bundle(Path::new(entry)).unwrap();
    let targets = Some(Browsers { safari: Some(13 << 16 ), ..Browsers::default() });
    stylesheet.minify(MinifyOptions { targets, ..MinifyOptions::default() }).unwrap();
    stylesheet.to_css(PrinterOptions { targets, ..PrinterOptions::default() }).unwrap().code
  }

  fn error_test(fs: TestProvider, entry: &str) {
    let mut bundler = Bundler::new(&fs, None, ParserOptions::default());
    let res = bundler.bundle(Path::new(entry));
    match res {
      Ok(_) => unreachable!(),
      Err(e) => assert!(matches!(e.kind, BundleErrorKind::UnsupportedLayerCombination))
    }
  }

  #[test]
  fn test_bundle() {
    let res = bundle(fs! {
      "/a.css": r#"
        @import "b.css";
        .a { color: red }
      "#,
      "/b.css": r#"
        .b { color: green }
      "#
    }, "/a.css");
    assert_eq!(res, indoc! { r#"
      .b {
        color: green;
      }
      
      .a {
        color: red;
      }
    "#});

    let res = bundle(fs! {
      "/a.css": r#"
        @import "b.css" print;
        .a { color: red }
      "#,
      "/b.css": r#"
        .b { color: green }
      "#
    }, "/a.css");
    assert_eq!(res, indoc! { r#"
      @media print {
        .b {
          color: green;
        }
      }
      
      .a {
        color: red;
      }
    "#});

    let res = bundle(fs! {
      "/a.css": r#"
        @import "b.css" supports(color: green);
        .a { color: red }
      "#,
      "/b.css": r#"
        .b { color: green }
      "#
    }, "/a.css");
    assert_eq!(res, indoc! { r#"
      @supports (color: green) {
        .b {
          color: green;
        }
      }
      
      .a {
        color: red;
      }
    "#});

    let res = bundle(fs! {
      "/a.css": r#"
        @import "b.css" supports(color: green) print;
        .a { color: red }
      "#,
      "/b.css": r#"
        .b { color: green }
      "#
    }, "/a.css");
    assert_eq!(res, indoc! { r#"
      @supports (color: green) {
        @media print {
          .b {
            color: green;
          }
        }
      }
      
      .a {
        color: red;
      }
    "#});

    let res = bundle(fs! {
      "/a.css": r#"
        @import "b.css" print;
        @import "b.css" screen;
        .a { color: red }
      "#,
      "/b.css": r#"
        .b { color: green }
      "#
    }, "/a.css");
    assert_eq!(res, indoc! { r#"
      @media print, screen {
        .b {
          color: green;
        }
      }
      
      .a {
        color: red;
      }
    "#});

    let res = bundle(fs! {
      "/a.css": r#"
        @import "b.css" supports(color: red);
        @import "b.css" supports(foo: bar);
        .a { color: red }
      "#,
      "/b.css": r#"
        .b { color: green }
      "#
    }, "/a.css");
    assert_eq!(res, indoc! { r#"
      @supports ((color: red) or (foo: bar)) {
        .b {
          color: green;
        }
      }
      
      .a {
        color: red;
      }
    "#});

    let res = bundle(fs! {
      "/a.css": r#"
        @import "b.css" print;
        .a { color: red }
      "#,
      "/b.css": r#"
        @import "c.css" (color);
        .b { color: yellow }
      "#,
      "/c.css": r#"
        .c { color: green }
      "#
    }, "/a.css");
    assert_eq!(res, indoc! { r#"
      @media print and (color) {
        .c {
          color: green;
        }
      }
      
      @media print {
        .b {
          color: #ff0;
        }
      }

      .a {
        color: red;
      }
    "#});

    let res = bundle(fs! {
      "/a.css": r#"
        @import "b.css";
        .a { color: red }
      "#,
      "/b.css": r#"
        @import "c.css";
      "#,
      "/c.css": r#"
        @import "a.css";
        .c { color: green }
      "#
    }, "/a.css");
    assert_eq!(res, indoc! { r#"
      .c {
        color: green;
      }

      .a {
        color: red;
      }
    "#});

    let res = bundle(fs! {
      "/a.css": r#"
        @import "b/c.css";
        .a { color: red }
      "#,
      "/b/c.css": r#"
        .b { color: green }
      "#
    }, "/a.css");
    assert_eq!(res, indoc! { r#"
      .b {
        color: green;
      }
      
      .a {
        color: red;
      }
    "#});

    let res = bundle(fs! {
      "/a.css": r#"
        @import "./b/c.css";
        .a { color: red }
      "#,
      "/b/c.css": r#"
        .b { color: green }
      "#
    }, "/a.css");
    assert_eq!(res, indoc! { r#"
      .b {
        color: green;
      }
      
      .a {
        color: red;
      }
    "#});

    let res = bundle_css_module(fs! {
      "/a.css": r#"
        @import "b.css";
        .a { color: red }
      "#,
      "/b.css": r#"
        .a { color: green }
      "#
    }, "/a.css");
    assert_eq!(res, indoc! { r#"
      .a_6lixEq_1 {
        color: green;
      }

      .a_6lixEq {
        color: red;
      }
    "#});

    let res = bundle_custom_media(fs! {
      "/a.css": r#"
        @import "media.css";
        @import "b.css";
        .a { color: red }
      "#,
      "/media.css": r#"
        @custom-media --foo print;
      "#,
      "/b.css": r#"
        @media (--foo) {
          .a { color: green }
        }
      "#
    }, "/a.css");
    assert_eq!(res, indoc! { r#"
      @media print {
        .a {
          color: green;
        }
      }

      .a {
        color: red;
      }
    "#});

    let res = bundle(fs! {
      "/a.css": r#"
        @import "b.css" layer(foo);
        .a { color: red }
      "#,
      "/b.css": r#"
        .b { color: green }
      "#
    }, "/a.css");
    assert_eq!(res, indoc! { r#"
      @layer foo {
        .b {
          color: green;
        }
      }
      
      .a {
        color: red;
      }
    "#});

    let res = bundle(fs! {
      "/a.css": r#"
        @import "b.css" layer;
        .a { color: red }
      "#,
      "/b.css": r#"
        .b { color: green }
      "#
    }, "/a.css");
    assert_eq!(res, indoc! { r#"
      @layer {
        .b {
          color: green;
        }
      }
      
      .a {
        color: red;
      }
    "#});

    let res = bundle(fs! {
      "/a.css": r#"
        @import "b.css" layer(foo);
        .a { color: red }
      "#,
      "/b.css": r#"
        @import "c.css" layer(bar);
        .b { color: green }
      "#,
      "/c.css": r#"
        .c { color: green }
      "#
    }, "/a.css");
    assert_eq!(res, indoc! { r#"
      @layer foo.bar {
        .c {
          color: green;
        }
      }

      @layer foo {
        .b {
          color: green;
        }
      }
      
      .a {
        color: red;
      }
    "#});

    let res = bundle(fs! {
      "/a.css": r#"
        @import "b.css" layer(foo);
        @import "b.css" layer(foo);
      "#,
      "/b.css": r#"
        .b { color: green }
      "#
    }, "/a.css");
    assert_eq!(res, indoc! { r#"
      @layer foo {
        .b {
          color: green;
        }
      }
    "#});

    let res = bundle(fs! {
      "/a.css": r#"
        @layer bar, foo;
        @import "b.css" layer(foo);
        
        @layer bar {
          div {
            background: red;
          }
        }
      "#,
      "/b.css": r#"
        @layer qux, baz;
        @import "c.css" layer(baz);
        
        @layer qux {
          div {
            background: green;
          }
        }
      "#,
      "/c.css": r#"
        div {
          background: yellow;
        }      
      "#
    }, "/a.css");
    assert_eq!(res, indoc! { r#"
      @layer bar, foo;
      @layer foo.qux, foo.baz;

      @layer foo.baz {
        div {
          background: #ff0;
        }
      }

      @layer foo {
        @layer qux {
          div {
            background: green;
          }
        }
      }
      
      @layer bar {
        div {
          background: red;
        }
      }
    "#});

    error_test(fs! {
      "/a.css": r#"
        @import "b.css" layer(foo);
        @import "b.css" layer(bar);
      "#,
      "/b.css": r#"
        .b { color: red }
      "#
    }, "/a.css");

    error_test(fs! {
      "/a.css": r#"
        @import "b.css" layer;
        @import "b.css" layer;
      "#,
      "/b.css": r#"
        .b { color: red }
      "#
    }, "/a.css");
    
    error_test(fs! {
      "/a.css": r#"
        @import "b.css" layer;
        .a { color: red }
      "#,
      "/b.css": r#"
        @import "c.css" layer;
        .b { color: green }
      "#,
      "/c.css": r#"
        .c { color: green }
      "#
    }, "/a.css");

    error_test(fs! {
      "/a.css": r#"
        @import "b.css" layer;
        .a { color: red }
      "#,
      "/b.css": r#"
        @import "c.css" layer(foo);
        .b { color: green }
      "#,
      "/c.css": r#"
        .c { color: green }
      "#
    }, "/a.css");

    // let res = bundle(fs! {
    //   "/a.css": r#"
    //     @import "b.css" supports(color: red) (color);
    //     @import "b.css" supports(foo: bar) (orientation: horizontal);
    //     .a { color: red }
    //   "#,
    //   "/b.css": r#"
    //     .b { color: green }
    //   "#
    // }, "/a.css");

    // let res = bundle(fs! {
    //   "/a.css": r#"
    //     @import "b.css" not print;
    //     .a { color: red }
    //   "#,
    //   "/b.css": r#"
    //     @import "c.css" not screen;
    //     .b { color: green }
    //   "#,
    //   "/c.css": r#"
    //     .c { color: yellow }
    //   "#
    // }, "/a.css");
  }
}
