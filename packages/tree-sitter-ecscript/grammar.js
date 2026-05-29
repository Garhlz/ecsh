/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

const PREC = {
  pipe: 5,
  range: 10,
  or: 20,
  and: 30,
  compare: 40,
  add: 60,
  multiply: 80,
  unary: 130,
  postfix: 150
};

/**
 * 当前 grammar 目标：
 * - 优先服务高亮、结构导航和后续编辑器集成
 * - 贴近现有 ecscript 语法，但不过度复刻运行时 parser 的每个细节
 * - `cmd{ ... }` 仍作为语法岛处理，不在 Tree-sitter 内完整解析 shell 子语法
 * - 语法岛边界由 external scanner 负责，避免字符串或内部 brace 提前截断
 */
module.exports = grammar({
  name: "ecscript",

  externals: ($) => [
    $.command_body,
  ],

  extras: ($) => [
    /[ \t\r\f]+/,
    $.comment
  ],

  word: ($) => $.identifier,

  conflicts: ($) => [
    [$.lambda_expression, $.parenthesized_expression],
    [$.object, $.statement_block],
    [$.primary_expression, $.variable_identifier],
    [$.expression, $._logical_expression_operand],
    [$.expression, $._comparison_operand],
    [$.expression, $._additive_operand],
    [$.expression, $._multiplicative_operand],
    [$.expression, $._unary_operand],
    [$.expression, $._postfix_operand],
    [$.postfix_expression, $._postfix_operand],
    [$._logical_expression_operand, $._comparison_operand],
    [$._logical_expression_operand, $._additive_operand],
    [$._logical_expression_operand, $._multiplicative_operand],
    [$._logical_expression_operand, $._unary_operand],
    [$._logical_expression_operand, $._postfix_operand],
    [$._comparison_operand, $._additive_operand],
    [$._comparison_operand, $._multiplicative_operand],
    [$._comparison_operand, $._unary_operand],
    [$._comparison_operand, $._postfix_operand],
    [$._additive_operand, $._multiplicative_operand],
    [$._additive_operand, $._unary_operand],
    [$._additive_operand, $._postfix_operand],
    [$._multiplicative_operand, $._unary_operand],
    [$._multiplicative_operand, $._postfix_operand],
    [$._unary_operand, $._postfix_operand]
  ],

  rules: {
    source_file: ($) => repeat(choice($._statement, $._separator)),

    comment: (_) => token(choice(
      seq("//", /.*/),
      seq("/*", /[^*]*\*+([^/*][^*]*\*+)*/, "/")
    )),

    _statement: ($) => choice(
      $.use_statement,
      $.let_statement,
      $.assignment_statement,
      $.compound_assignment_statement,
      $.expression_statement,
      $.statement_block,
      $.if_statement,
      $.while_statement,
      $.for_statement,
      $.function_declaration,
      $.break_statement,
      $.continue_statement,
      $.return_statement
    ),

    use_statement: ($) => seq(
      "use",
      field("path", $.module_path),
      "as",
      field("alias", $.module_identifier)
    ),

    let_statement: ($) => seq(
      optional(field("visibility", "pub")),
      "let",
      field("name", $.variable_identifier),
      "=",
      field("value", $.expression)
    ),

    assignment_statement: ($) => seq(
      field("target", $.assignment_target),
      "=",
      field("value", $.expression)
    ),

    compound_assignment_statement: ($) => seq(
      field("target", $.assignment_target),
      field("operator", choice("+=", "-=", "*=", "/=", "%=")),
      field("value", $.expression)
    ),

    expression_statement: ($) => $.expression,

    statement_block: ($) => seq(
      "{",
      repeat(choice($._statement, $._separator)),
      "}"
    ),

    if_statement: ($) => seq(
      "if",
      field("condition", $.expression),
      field("consequence", $.statement_block),
      optional(seq(
        "else",
        field("alternative", choice($.if_statement, $.statement_block))
      ))
    ),

    while_statement: ($) => seq(
      "while",
      field("condition", $.expression),
      field("body", $.statement_block)
    ),

    for_statement: ($) => seq(
      "for",
      field("name", $.variable_identifier),
      "in",
      field("iterable", choice($.for_range_expression, $.expression)),
      field("body", $.statement_block)
    ),

    function_declaration: ($) => seq(
      optional(field("visibility", "pub")),
      "func",
      field("name", $.variable_identifier),
      field("parameters", $.parameter_list),
      field("body", $.statement_block)
    ),

    break_statement: (_) => "break",
    continue_statement: (_) => "continue",

    return_statement: ($) => prec.right(seq(
      "return",
      optional($.expression)
    )),

    newline: (_) => /\n/,
    _separator: ($) => choice(";", $.newline),

    parameter_list: ($) => seq(
      "(",
      repeat($.newline),
      optional(commaSep1($,$.variable_identifier)),
      repeat($.newline),
      ")"
    ),

    assignment_target: ($) => choice(
      $.identifier,
      $.field_expression,
      $.index_expression
    ),

    module_path: ($) => choice(
      $.path_literal,
      $.string
    ),

    path_literal: (_) => token(/\.?\.?\/?[^\s"'<>;(){}\[\],|]+/),

    expression: ($) => choice(
      $.pipe_expression,
      $.logical_or_expression,
      $.logical_and_expression,
      $.comparison_expression,
      $.additive_expression,
      $.multiplicative_expression,
      $.unary_expression,
      $.postfix_expression,
      $.primary_expression
    ),

    pipe_expression: ($) => prec.left(PREC.pipe, seq(
      field("left", choice($.pipe_expression, $._logical_expression_operand)),
      "|>",
      field("right", $.call_expression)
    )),

    for_range_expression: ($) => prec.left(PREC.range, seq(
      field("left", $._logical_expression_operand),
      field("operator", choice("..", "..=")),
      field("right", $._logical_expression_operand)
    )),

    logical_or_expression: ($) => prec.left(PREC.or, seq(
      field("left", $._logical_expression_operand),
      "||",
      field("right", $._logical_expression_operand)
    )),

    logical_and_expression: ($) => prec.left(PREC.and, seq(
      field("left", $._comparison_operand),
      "&&",
      field("right", $._comparison_operand)
    )),

    comparison_expression: ($) => prec.left(PREC.compare, seq(
      field("left", $._additive_operand),
      field("operator", choice("==", "!=", "<", ">", "<=", ">=")),
      field("right", $._additive_operand)
    )),

    additive_expression: ($) => prec.left(PREC.add, seq(
      field("left", $._multiplicative_operand),
      field("operator", choice("+", "-")),
      field("right", $._multiplicative_operand)
    )),

    multiplicative_expression: ($) => prec.left(PREC.multiply, seq(
      field("left", $._unary_operand),
      field("operator", choice("*", "/", "%")),
      field("right", $._unary_operand)
    )),

    unary_expression: ($) => prec(PREC.unary, seq(
      field("operator", choice("!", "-")),
      field("argument", $._postfix_operand)
    )),

    postfix_expression: ($) => choice(
      $.call_expression,
      $.field_expression,
      $.index_expression
    ),

    call_expression: ($) => prec.left(PREC.postfix, seq(
      field("function", $._postfix_operand),
      field("arguments", $.argument_list)
    )),

    field_expression: ($) => prec.left(PREC.postfix, seq(
      field("object", $._postfix_operand),
      ".",
      field("field", $.property_identifier)
    )),

    index_expression: ($) => prec.left(PREC.postfix, seq(
      field("object", $._postfix_operand),
      "[",
      field("index", $.expression),
      "]"
    )),

    argument_list: ($) => seq(
      "(",
      repeat($.newline),
      optional(commaSep1($,$.expression)),
      repeat($.newline),
      ")"
    ),

    primary_expression: ($) => choice(
      $.identifier,
      $.integer,
      $.float,
      $.string,
      $.raw_string,
      $.true,
      $.false,
      $.nil,
      $.array,
      $.object,
      $.lambda_expression,
      $.command_literal,
      $.parenthesized_expression
    ),

    parenthesized_expression: ($) => seq(
      "(",
      $.expression,
      ")"
    ),

    lambda_expression: ($) => seq(
      $.parameter_list,
      "=>",
      field("body", choice($.statement_block, $.expression))
    ),

    command_literal: ($) => seq(
      "cmd",
      "{",
      optional($.command_body),
      "}"
    ),

    // `cmd{ ... }` 的内容由 external scanner 作为单个 `command_body`
    // token 扫出。scanner 只负责识别语法岛边界，不尝试解析 shell AST。

    array: ($) => seq(
      "[",
      repeat($.newline),
      optional(commaSep1($,$.expression)),
      optional(","),
      repeat($.newline),
      "]"
    ),

    object: ($) => seq(
      "{",
      repeat($.newline),
      optional(commaSep1($,$.object_entry)),
      optional(","),
      repeat($.newline),
      "}"
    ),

    object_entry: ($) => seq(
      field("key", choice($.property_identifier, $.string)),
      ":",
      field("value", $.expression)
    ),

    identifier: (_) => /[A-Za-z_][A-Za-z0-9_]*/,
    variable_identifier: ($) => alias($.identifier, $.variable_identifier),
    property_identifier: ($) => alias($.identifier, $.property_identifier),
    module_identifier: ($) => alias($.identifier, $.module_identifier),
    integer: (_) => /[0-9]+/,
    float: (_) => token(choice(
      /[0-9]+\.[0-9]+/,
      /\.[0-9]+/
    )),
    string: (_) => token(seq(
      "\"",
      repeat(choice(
        /[^"\\]+/,
        /\\./
      )),
      "\""
    )),
    raw_string: (_) => token(seq(
      "r\"",
      repeat(/[^"]/),
      "\""
    )),

    true: (_) => "true",
    false: (_) => "false",
    nil: (_) => "nil",

    _logical_expression_operand: ($) => choice(
      $.logical_and_expression,
      $.comparison_expression,
      $.additive_expression,
      $.multiplicative_expression,
      $.unary_expression,
      $.postfix_expression,
      $.primary_expression
    ),

    _comparison_operand: ($) => choice(
      $.comparison_expression,
      $.additive_expression,
      $.multiplicative_expression,
      $.unary_expression,
      $.postfix_expression,
      $.primary_expression
    ),

    _additive_operand: ($) => choice(
      $.additive_expression,
      $.multiplicative_expression,
      $.unary_expression,
      $.postfix_expression,
      $.primary_expression
    ),

    _multiplicative_operand: ($) => choice(
      $.multiplicative_expression,
      $.unary_expression,
      $.postfix_expression,
      $.primary_expression
    ),

    _unary_operand: ($) => choice(
      $.unary_expression,
      $.postfix_expression,
      $.primary_expression
    ),

    _postfix_operand: ($) => choice(
      $.field_expression,
      $.index_expression,
      $.call_expression,
      $.primary_expression
    )
  }
});

function commaSep1($, rule) {
  return seq(rule, repeat(seq(",", repeat($.newline), rule)));
}
