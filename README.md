# oas2client

An OpenAPI client generator written in Rust. It replaces `openapi-generator-cli` 7.8.0
for the generators it supports and produces byte-identical output, including
`.openapi-generator/FILES`. It is 8–25 times faster and needs no JVM. Templates can be
written in Mustache or Jinja, and both can be mixed in one template set.

```sh
oas2client -g typescript-fetch -i openapi.json -o client-ts
oas2client -g java -i openapi.json -o client-java
```

| Option | Meaning |
|---|---|
| `-i <file>` | OpenAPI 3.0/3.1 spec (JSON) |
| `-o <dir>` | output directory (files are overwritten, nothing is deleted) |
| `-g <set>` | template set: built-in name or a directory with a `manifest.toml` (default `typescript-fetch`) |
| `-t <dir>` | templates that override the set's templates by name |
| `-p key=value` | language option, overrides the manifest's `[options]` |

## Installation

Download a binary from the [releases](https://github.com/dhufnagel/oas2client/releases)
(Linux x86_64/aarch64 statically linked, macOS x86_64/arm64, Windows x86_64), or build it
yourself:

```sh
cargo install --git https://github.com/dhufnagel/oas2client
```

## Built-in template sets

| Set | Generates | Compared with openapi-generator 7.8.0 |
|---|---|---|
| `typescript-fetch` | `-g typescript-fetch -p fileNaming=kebab-case` | identical |
| `java` | `-g java` (library okhttp-gson) | identical except `api/openapi.yaml` (see below) |

Custom templates written for openapi-generator keep working: pass their directory with
`-t`, and they override the built-in templates of the same name.

## Architecture

```
spec ──► core ──► lang ──► template ──► files
         (neutral   (type mapping,   (Mustache / Jinja
          model)     naming, contexts) + manifest)
```

- **core** (`src/core`) loads and validates the spec. It rejects duplicate keys,
  unresolvable `$ref`, repeated `operationId` and undeclared path parameters. It then
  resolves the spec into a language-neutral model of types, models, operations,
  parameters and responses.
- **lang** (`src/lang`) has one module per language. Each module maps core types to the
  language (e.g. `List<UUID>` or `Array<string>`), derives names and imports, and builds
  the template contexts. The contexts have the shape of openapi-generator's
  (`CodegenModel`, `CodegenOperation`, ...), so its templates work unchanged.
- **template** (`src/template`) handles template sets. A set is a directory with a
  `manifest.toml` and templates, and it can be stacked from several layers. The file
  extension selects the engine:
  - `name.jinja`: MiniJinja
  - `name.mustache`: a jmustache-compatible Mustache engine
  - plain `name`: copied as is

  A `.jinja` file passed with `-t` replaces the `.mustache` template of the same name,
  and vice versa. This applies to templates rendered by the manifest, not to Mustache
  partials. See `examples/java-jinja-override`, which replaces one Java template with
  Jinja and leaves the output identical.

### Manifest

```toml
language = "java"
layers = [".", "libraries/okhttp-gson"]   # later layers win

[options]                                  # passed to the language layer
invokerPackage = "org.openapitools.client"

[[files]]
each = "model"                             # model | api | once
template = "model"                         # model.jinja, model.mustache or model
path = "{modelFolder}/{model.classname}.java"
when = "model.isEnum"                      # optional; `unless` is the negation
```

What each kind of file sees as its context:

- **model**: the global context plus `models: [{model, importPath}]`, `model`, `imports`
  and `package`.
- **api**: the global context plus `operations: {classname, operation: [...]}` and
  `imports`.
- **once**: the global context plus `models` (all models) and `apiInfo.apis` (all APIs).

Two tools help when writing templates:

- `OAS2CLIENT_DUMP_CONTEXTS=<dir>` writes every context as JSON.
- `oas2client render-context <set> <template> <ctx.json>` renders a single template.

### Adding a language

1. Add `src/lang/<lang>.rs`: implement `Language::contexts` (type mapping, naming,
   imports) and register it in `lang::by_name`.
2. Add `templates/<set>/manifest.toml` and the templates. These can be the Mustache
   templates of openapi-generator or Jinja templates of your own.

## Verification

During development, the output was compared byte for byte with openapi-generator 7.8.0:

- a production spec with about 380 schemas and 240 operations
- more than 130 historical revisions of that spec
- `examples/petstore.json`

The project's tests and CI generate the example clients.

| Spec with 380 schemas | openapi-generator | oas2client |
|---|---|---|
| TypeScript (450 files) | ~2.9 s | ~0.12 s |
| Java (1366 files) | ~4.0 s | ~0.46 s |

## Limitations

The following are not supported yet:

- `allOf` inheritance with own properties
- `oneOf`/`anyOf` with several variants
- authentication other than HTTP bearer for TypeScript
- `readOnly` properties
- other Java libraries than okhttp-gson
- YAML specs (only JSON is read)

For Java, `api/openapi.yaml` is the only file that differs from openapi-generator.
openapi-generator writes its normalized spec there, with generated examples. oas2client
writes the input spec as YAML.

## License

Apache-2.0. The templates in `templates/` come from
[OpenAPI Generator](https://github.com/OpenAPITools/openapi-generator) (Apache-2.0); see
`NOTICE`.
