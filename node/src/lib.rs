#[macro_use]
extern crate napi_derive;

#[cfg(target_os = "macos")]
#[global_allocator]
static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;

#[cfg(not(target_arch = "wasm32"))]
mod stylesheet;
mod rule_list;
mod rule;
mod style_rule;

mod error;
use error::CompileError;

use serde::{Serialize, Deserialize};
use parcel_css::stylesheet::StyleSheet;
use parcel_css::targets::Browsers;

// ---------------------------------------------

#[cfg(target_arch = "wasm32")]
use serde_wasm_bindgen::{from_value, Serializer};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn transform(config_val: JsValue) -> Result<JsValue, JsValue> {
  let config: Config = from_value(config_val).map_err(JsValue::from)?;
  let code = unsafe { std::str::from_utf8_unchecked(&config.code) };  
  let res = compile(code, &config)?;
  let serializer = Serializer::new().serialize_maps_as_objects(true);
  res.serialize(&serializer).map_err(JsValue::from)
}

// ---------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
use napi_derive::{js_function, module_exports};
#[cfg(not(target_arch = "wasm32"))]
use napi::{CallContext, JsObject, JsUnknown, Env};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceMapJson<'a> {
  version: u8,
  mappings: String,
  sources: &'a Vec<String>,
  sources_content: &'a Vec<String>,
  names: &'a Vec<String>
}

#[derive(Serialize)]
struct TransformResult {
  #[serde(with = "serde_bytes")]
  code: Vec<u8>,
  #[serde(with = "serde_bytes")]
  map: Option<Vec<u8>>
}

#[cfg(not(target_arch = "wasm32"))]
#[js_function(1)]
fn transform(ctx: CallContext) -> napi::Result<JsUnknown> {
  let opts = ctx.get::<JsObject>(0)?;
  let config: Config = ctx.env.from_js_value(opts)?;
  let code = unsafe { std::str::from_utf8_unchecked(&config.code) }; 
  let res = compile(code, &config);

  match res {
    Ok(res) => ctx.env.to_js_value(&res),
    Err(err) => {
      match &err {
        CompileError::ParseError(e) => {
          // Generate an error with location information.
          let syntax_error = ctx.env.get_global()?
            .get_named_property::<napi::JsFunction>("SyntaxError")?;
          let reason = ctx.env.create_string_from_std(err.reason())?;
          let line = ctx.env.create_int32((e.location.line + 1) as i32)?;
          let col = ctx.env.create_int32(e.location.column as i32)?;
          let mut obj = syntax_error.new(&[reason])?;
          let filename = ctx.env.create_string_from_std(config.filename)?;
          obj.set_named_property("fileName", filename)?;
          let source = ctx.env.create_string(code)?;
          obj.set_named_property("source", source)?;
          let mut loc = ctx.env.create_object()?;
          loc.set_named_property("line", line)?;
          loc.set_named_property("column", col)?;
          obj.set_named_property("loc", loc)?;
          ctx.env.throw(obj)?;
          Ok(ctx.env.get_undefined()?.into_unknown())
        }
        _ => Err(err.into())
      }
    }
  }
}

#[cfg(not(target_arch = "wasm32"))]
#[module_exports]
fn init(mut exports: JsObject, env: Env) -> napi::Result<()> {
  exports.create_named_method("transform", transform)?;
  stylesheet::init(&mut exports, env)?;
  rule_list::init(&mut exports, env)?;
  rule::init(&mut exports, env)?;
  style_rule::init(&mut exports, env)?;

  Ok(())
}

// ---------------------------------------------

#[derive(Serialize, Debug, Deserialize)]
struct Config {
  pub filename: String,
  #[serde(with = "serde_bytes")]
  pub code: Vec<u8>,
  pub targets: Option<Browsers>,
  pub minify: Option<bool>,
  pub source_map: Option<bool>
}

fn compile<'i>(code: &'i str, config: &Config) -> Result<TransformResult, CompileError<'i>> {
  let mut stylesheet = StyleSheet::parse(config.filename.clone(), &code)?;
  stylesheet.minify(config.targets); // TODO: should this be conditional?
  let (res, source_map) = stylesheet.to_css(
    config.minify.unwrap_or(false),
    config.source_map.unwrap_or(false),
    config.targets
  )?;

  let map = if let Some(mut source_map) = source_map {
    source_map.set_source_content(0, code)?;
    let mut vlq_output: Vec<u8> = Vec::new();
    source_map.write_vlq(&mut vlq_output)?;

    let sm = SourceMapJson {
      version: 3,
      mappings: unsafe { String::from_utf8_unchecked(vlq_output) },
      sources: source_map.get_sources(),
      sources_content: source_map.get_sources_content(),
      names: source_map.get_names()
    };

    serde_json::to_vec(&sm).ok()
  } else {
    None
  };

  Ok(TransformResult {
    code: res.into_bytes(),
    map
  })
}
