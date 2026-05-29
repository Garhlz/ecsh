; ── Keywords: declaration ─────────────
[
  "let"
  "func"
  "pub"
] @keyword.declaration

; ── Keywords: control flow ────────────
[
  "if"
  "else"
  "while"
  "for"
  "in"
  "return"
] @keyword.control

; ── Keywords: import ─────────────────
[
  "use"
  "as"
] @keyword.import

; ── Keywords: command ────────────────
(command_literal
  "cmd" @keyword.command)

; ── Constants ────────────────────────
[
  (true)
  (false)
] @constant.builtin

(nil) @constant.builtin

; ── Numbers ──────────────────────────
[
  (integer)
  (float)
] @number

; ── Strings ──────────────────────────
(string) @string
(raw_string) @string
(path_literal) @string.special.path

; ── Comments ────────────────────────
(comment) @comment

; ── Functions ────────────────────────
(function_declaration
  name: (variable_identifier) @function.declaration)

(call_expression
  function: (primary_expression
    (identifier) @function.call))

(call_expression
  function: (field_expression
    field: (property_identifier) @function.method))

; ── Parameters ───────────────────────
(parameter_list
  (variable_identifier) @parameter)

; ── Variables ────────────────────────
(let_statement
  name: (variable_identifier) @variable.declaration)

(for_statement
  name: (variable_identifier) @variable.declaration)

; ── Properties ───────────────────────
(field_expression
  field: (property_identifier) @property)

(object_entry
  key: (property_identifier) @property)

(object_entry
  key: (string) @property)

; ── Modules ──────────────────────────
(use_statement
  alias: (module_identifier) @namespace)

; ── Embedded command ─────────────────
(command_body) @string.special

; ── Operators ────────────────────────
"|>" @operator.pipe

[
  "+"
  "-"
  "*"
  "/"
  "%"
  "=="
  "!="
  "<"
  ">"
  "<="
  ">="
  "&&"
  "||"
  "!"
  "="
  "+="
  "-="
  "*="
  "/="
  "%="
  ".."
  "..="
] @operator

; ── Punctuation ──────────────────────
[
  "."
  ","
  ":"
  ";"
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.delimiter
