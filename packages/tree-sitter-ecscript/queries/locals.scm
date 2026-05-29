; Definitions
(function_declaration
  name: (variable_identifier) @local.definition.function)

(let_statement
  name: (variable_identifier) @local.definition.var)

(for_statement
  name: (variable_identifier) @local.definition.var)

(parameter_list
  (variable_identifier) @local.definition.parameter)

; References — only variable_identifier, never property_identifier
(variable_identifier) @local.reference
