# JSON messages

`quire build --message-format json` and `quire watch --message-format json`
print one JSON object per line on standard output, like
`cargo --message-format=json`. Status lines stay on standard error. With `-v`,
latexmk's output also goes to standard error, so standard output is always
valid JSON.

Every object has a `reason`.

## `compiler-message`

An error or warning from the TeX log:

```json
{
  "reason": "compiler-message",
  "document": "thesis",
  "profile": "dev",
  "message": {
    "level": "error",
    "message": "Undefined control sequence.",
    "file": "/home/me/thesis/src/ch1.tex",
    "line": 42,
    "context": ["l.42 \\foo", "        bar"],
    "rendered": "/home/me/thesis/src/ch1.tex:42: Undefined control sequence.\n  l.42 \\foo\n          bar"
  }
}
```

| Field | |
|---|---|
| `level` | `"error"` or `"warning"`. |
| `message` | The message text. |
| `file` | Absolute path with forward slashes, or `null` if TeX didn't name a file. |
| `line` | Line number, or `null`. For warnings, taken from "on input line N". |
| `context` | The `l.NN …` lines TeX printed to show where it stopped. |
| `rendered` | Human-readable form, as quire prints it. |

## `document-artifact`

The build produced a PDF:

```json
{"reason": "document-artifact", "document": "thesis", "profile": "dev",
 "pdf": "/home/me/thesis/target/dev/thesis.pdf", "log": "/home/me/thesis/target/dev/thesis.log"}
```

## `build-finished`

Always the last message of a build:

```json
{"reason": "build-finished", "success": true}
```

In watch mode, each rebuild ends with its own `build-finished`.
