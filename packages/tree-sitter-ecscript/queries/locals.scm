; ── Definitions ──────────────────────────
(function_declaration
  name: (variable_identifier) @local.definition.function)

(let_statement
  name: (variable_identifier) @local.definition.var)

(for_statement
  name: (variable_identifier) @local.definition.var)

(parameter_list
  (variable_identifier) @local.definition.parameter)

; ── References ───────────────────────────
; Aliased identifiers used as bindings (let, for, params, func names).
(variable_identifier) @local.reference

; Bare identifiers used as values in expressions (e.g. `obj` in `obj.name`,
; function names in calls). These do NOT match aliased property_identifier
; or module_identifier nodes, so property and namespace references are
; correctly excluded.
(identifier) @local.reference
