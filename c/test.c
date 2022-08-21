#include <stdio.h>
#include <string.h>
#include "parcel_css.h"

int print_error(CssError *error);

int main()
{
  char *source =
      ".foo {"
      "  color: lch(50.998% 135.363 338);"
      "}"
      ".bar {"
      "  color: yellow;"
      "  composes: foo from './bar.css';"
      "}"
      ".baz:hover {"
      "  color: var(--foo from './baz.css');"
      "}";

  ParseOptions parse_opts = {
      .filename = "test.css",
      .css_modules = true,
      .css_modules_pattern = "yo_[name]_[local]",
      .css_modules_dashed_idents = true};

  CssError *error = NULL;
  StyleSheet *stylesheet = stylesheet_parse(source, strlen(source), parse_opts, &error);
  if (!stylesheet)
    goto cleanup;

  char *unused_symbols[1] = {"bar"};
  TransformOptions transform_opts = {
      .unused_symbols = unused_symbols,
      .unused_symbols_len = 0};

  if (!browserslist_to_targets("last 2 versions, not IE <= 11", &transform_opts.targets, &error))
    goto cleanup;

  if (!stylesheet_transform(stylesheet, transform_opts, &error))
    goto cleanup;

  ToCssOptions to_css_opts = {
      .minify = true,
      .source_map = true,
      .pseudo_classes = {
          .hover = "is-hovered"}};

  ToCssResult result = stylesheet_to_css(stylesheet, to_css_opts, &error);
  if (error)
    goto cleanup;

  fwrite(result.code.text, sizeof(char), result.code.len, stdout);
  printf("\n");
  fwrite(result.map.text, sizeof(char), result.map.len, stdout);
  printf("\n");

  for (int i = 0; i < result.exports_len; i++)
  {
    printf("%.*s -> %.*s\n", (int)result.exports[i].exported.len, result.exports[i].exported.text, (int)result.exports[i].local.len, result.exports[i].local.text);
    for (int j = 0; j < result.exports[i].composes_len; j++)
    {
      const CssModuleReference *ref = &result.exports[i].composes[j];
      switch (ref->tag)
      {
      case CssModuleReference_Local:
        printf("  composes local: %.*s\n", (int)ref->local.name.len, ref->local.name.text);
        break;
      case CssModuleReference_Global:
        printf("  composes global: %.*s\n", (int)ref->global.name.len, ref->global.name.text);
        break;
      case CssModuleReference_Dependency:
        printf("  composes dependency: %.*s from %.*s\n", (int)ref->dependency.name.len, ref->dependency.name.text, (int)ref->dependency.specifier.len, ref->dependency.specifier.text);
        break;
      }
    }
  }

  for (int i = 0; i < result.references_len; i++)
  {
    printf("placeholder: %.*s\n", (int)result.references[i].placeholder.len, result.references[i].placeholder.text);
  }

cleanup:
  stylesheet_free(stylesheet);
  to_css_result_free(result);

  if (error)
  {
    printf("error: %s\n", error_message(error));
    error_free(error);
    return 1;
  }
}
