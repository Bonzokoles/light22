use crate::values::{
  size::Size2D,
  length::LengthPercentageOrAuto,
  rect::Rect
};
use crate::properties::{Property, PropertyId};
use crate::declaration::DeclarationList;
use crate::traits::PropertyHandler;
use crate::logical::PropertyCategory;
use crate::context::PropertyHandlerContext;
use crate::compat::Feature;

macro_rules! side_handler {
  ($name: ident, $top: ident, $bottom: ident, $left: ident, $right: ident, $block_start: ident, $block_end: ident, $inline_start: ident, $inline_end: ident, $shorthand: ident, $block_shorthand: ident, $inline_shorthand: ident, $logical_shorthand: literal $(, $feature: ident)?) => {
    #[derive(Debug, Default)]
    pub(crate) struct $name {
      top: Option<LengthPercentageOrAuto>,
      bottom: Option<LengthPercentageOrAuto>,
      left: Option<LengthPercentageOrAuto>,
      right: Option<LengthPercentageOrAuto>,
      block_start: Option<LengthPercentageOrAuto>,
      block_end: Option<LengthPercentageOrAuto>,
      inline_start: Option<LengthPercentageOrAuto>,
      inline_end: Option<LengthPercentageOrAuto>,
      has_any: bool,
      category: PropertyCategory
    }

    impl<'i> PropertyHandler<'i> for $name {
      fn handle_property(&mut self, property: &Property<'i>, dest: &mut DeclarationList<'i>, context: &mut PropertyHandlerContext<'i>) -> bool {
        use Property::*;

        macro_rules! property {
          ($key: ident, $val: ident, $category: ident) => {{
            if PropertyCategory::$category != self.category {
              self.flush(dest, context);
            }
            self.$key = Some($val.clone());
            self.category = PropertyCategory::$category;
            self.has_any = true;
          }};
        }

        macro_rules! set_shorthand {
          ($start: ident, $end: ident, $val: ident) => {{
            if self.category != PropertyCategory::Logical {
              self.flush(dest, context);
            }
            self.$start = Some($val.0.clone());
            self.$end = Some($val.1.clone());
            self.category = PropertyCategory::Logical;
            self.has_any = true;
          }};
        }

        match &property {
          $top(val) => property!(top, val, Physical),
          $bottom(val) => property!(bottom, val, Physical),
          $left(val) => property!(left, val, Physical),
          $right(val) => property!(right, val, Physical),
          $block_start(val) => property!(block_start, val, Logical),
          $block_end(val) => property!(block_end, val, Logical),
          $inline_start(val) => property!(inline_start, val, Logical),
          $inline_end(val) => property!(inline_end, val, Logical),
          $block_shorthand(val) => set_shorthand!(block_start, block_end, val),
          $inline_shorthand(val) => set_shorthand!(inline_start, inline_end, val),
          $shorthand(val) => {
            // dest.clear();
            self.top = Some(val.0.clone());
            self.right = Some(val.1.clone());
            self.bottom = Some(val.2.clone());
            self.left = Some(val.3.clone());
            self.block_start = None;
            self.block_end = None;
            self.inline_start = None;
            self.inline_end = None;
            self.has_any = true;
          }
          Unparsed(val) if matches!(val.property_id, PropertyId::$top | PropertyId::$bottom | PropertyId::$left | PropertyId::$right | PropertyId::$block_start | PropertyId::$block_end | PropertyId::$inline_start | PropertyId::$inline_end | PropertyId::$block_shorthand | PropertyId::$inline_shorthand | PropertyId::$shorthand) => {
            self.flush(dest, context);
            dest.push(property.clone());
          }
          _ => return false
        }

        true
      }

      fn finalize(&mut self, dest: &mut DeclarationList<'i>, context: &mut PropertyHandlerContext<'i>) {
        self.flush(dest, context);
      }
    }

    impl<'i> $name {
      fn flush(&mut self, dest: &mut DeclarationList<'i>, context: &mut PropertyHandlerContext<'i>) {
        use Property::*;

        if !self.has_any {
          return
        }

        self.has_any = false;

        let top = std::mem::take(&mut self.top);
        let bottom = std::mem::take(&mut self.bottom);
        let left = std::mem::take(&mut self.left);
        let right = std::mem::take(&mut self.right);
        let logical_supported = true $(&& context.is_supported(Feature::$feature))?;

        if (!$logical_shorthand || logical_supported) && top.is_some() && bottom.is_some() && left.is_some() && right.is_some() {
          let rect = Rect::new(top.unwrap(), right.unwrap(), bottom.unwrap(), left.unwrap());
          dest.push($shorthand(rect));
        } else {
          if let Some(val) = top {
            dest.push($top(val));
          }

          if let Some(val) = bottom {
            dest.push($bottom(val));
          }

          if let Some(val) = left {
            dest.push($left(val));
          }

          if let Some(val) = right {
            dest.push($right(val));
          }
        }

        let block_start = std::mem::take(&mut self.block_start);
        let block_end = std::mem::take(&mut self.block_end);
        let inline_start = std::mem::take(&mut self.inline_start);
        let inline_end = std::mem::take(&mut self.inline_end);

        macro_rules! logical_side {
          ($start: ident, $end: ident, $shorthand_prop: ident, $start_prop: ident, $end_prop: ident) => {
            if ($start.is_some() && $end.is_some()) {
              let size = Size2D($start.unwrap(), $end.unwrap());
              dest.push($shorthand_prop(size));
            } else {
              if let Some(val) = $start {
                dest.push($start_prop(val));
              }
    
              if let Some(val) = $end {
                dest.push($end_prop(val));
              }
            }
          };
        }

        if logical_supported {
          logical_side!(block_start, block_end, $block_shorthand, $block_start, $block_end);
        } else {
          if let Some(val) = block_start {
            dest.push($top(val));
          }

          if let Some(val) = block_end {
            dest.push($bottom(val));
          }
        }

        if logical_supported {
          logical_side!(inline_start, inline_end, $inline_shorthand, $inline_start, $inline_end);
        } else if inline_start.is_some() || inline_end.is_some() {
          if inline_start == inline_end {
            dest.push($left(inline_start.unwrap()));
            dest.push($right(inline_end.unwrap()));
          } else {
            context.add_inline_logical_properties(
              dest,
              PropertyId::$left,
              PropertyId::$right,
              inline_start.map(|v| Property::$inline_start(v)),
              inline_end.map(|v| Property::$inline_end(v)),
            );
          }
        }
      }
    }
  };
}

side_handler!(
  MarginHandler,
  MarginTop,
  MarginBottom,
  MarginLeft,
  MarginRight,
  MarginBlockStart,
  MarginBlockEnd,
  MarginInlineStart,
  MarginInlineEnd,
  Margin,
  MarginBlock,
  MarginInline,
  false,
  LogicalMargin
);

side_handler!(
  PaddingHandler,
  PaddingTop,
  PaddingBottom,
  PaddingLeft,
  PaddingRight,
  PaddingBlockStart,
  PaddingBlockEnd,
  PaddingInlineStart,
  PaddingInlineEnd,
  Padding,
  PaddingBlock,
  PaddingInline,
  false,
  LogicalPadding
);

side_handler!(
  ScrollMarginHandler,
  ScrollMarginTop,
  ScrollMarginBottom,
  ScrollMarginLeft,
  ScrollMarginRight,
  ScrollMarginBlockStart,
  ScrollMarginBlockEnd,
  ScrollMarginInlineStart,
  ScrollMarginInlineEnd,
  ScrollMargin,
  ScrollMarginBlock,
  ScrollMarginInline,
  false
);

side_handler!(
  ScrollPaddingHandler,
  ScrollPaddingTop,
  ScrollPaddingBottom,
  ScrollPaddingLeft,
  ScrollPaddingRight,
  ScrollPaddingBlockStart,
  ScrollPaddingBlockEnd,
  ScrollPaddingInlineStart,
  ScrollPaddingInlineEnd,
  ScrollPadding,
  ScrollPaddingBlock,
  ScrollPaddingInline,
  false
);

side_handler!(
  InsetHandler,
  Top,
  Bottom,
  Left,
  Right,
  InsetBlockStart,
  InsetBlockEnd,
  InsetInlineStart,
  InsetInlineEnd,
  Inset,
  InsetBlock,
  InsetInline,
  true,
  LogicalInset
);
