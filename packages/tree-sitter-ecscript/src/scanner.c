#include <tree_sitter/parser.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>

enum {
  TOKEN_COMMAND_BODY,
};

/// Track parse state across the command-body scan loop.
typedef struct {
  int brace_depth;
  bool in_single_quote;
  bool in_double_quote;
  bool escaped;
} ScannerState;

void *tree_sitter_ecscript_external_scanner_create() {
  ScannerState *state = calloc(1, sizeof(ScannerState));
  return state;
}

void tree_sitter_ecscript_external_scanner_destroy(void *payload) {
  free(payload);
}

void tree_sitter_ecscript_external_scanner_reset(void *payload) {
  ScannerState *state = (ScannerState *)payload;
  memset(state, 0, sizeof(ScannerState));
}

unsigned tree_sitter_ecscript_external_scanner_serialize(
    void *payload, char *buffer) {
  // No cross-token state needed for command body scanning.
  return 0;
}

void tree_sitter_ecscript_external_scanner_deserialize(
    void *payload, const char *buffer, unsigned length) {
  // No cross-token state needed.
}

static inline void advance(TSLexer *lexer) {
  lexer->advance(lexer, false);
}

/// Scan the command body inside cmd{ ... }.
/// Stops BEFORE the closing `}` that balances brace_depth back to 0,
/// so the grammar's `}` rule can consume it.
static bool scan_command_body(TSLexer *lexer, ScannerState *s) {
  bool consumed = false;

  // `cmd{}` and `cmd{ }` should be handled by the grammar's optional body.
  // Do not emit empty external tokens.
  if (lexer->eof(lexer) || lexer->lookahead == '}') {
    return false;
  }

  while (true) {
    if (lexer->eof(lexer)) {
      // Unterminated command body: if we already consumed text, still produce
      // a token so tree-sitter can recover with a missing outer `}`.
      if (consumed) {
        lexer->result_symbol = TOKEN_COMMAND_BODY;
        return true;
      }
      return false;
    }

    int32_t ch = lexer->lookahead;

    // ── escape handling ──────────────────────────────────────
    if (s->escaped) {
      s->escaped = false;
      consumed = true;
      advance(lexer);
      continue;
    }

    // ── backslash ────────────────────────────────────────────
    if (ch == '\\' && !s->in_single_quote) {
      s->escaped = true;
      consumed = true;
      advance(lexer);
      continue;
    }

    // ── quote toggling ───────────────────────────────────────
    if (ch == '\'' && !s->in_double_quote) {
      s->in_single_quote = !s->in_single_quote;
      consumed = true;
      advance(lexer);
      continue;
    }

    if (ch == '"' && !s->in_single_quote) {
      s->in_double_quote = !s->in_double_quote;
      consumed = true;
      advance(lexer);
      continue;
    }

    // ── brace depth (outside quotes) ─────────────────────────
    if (!s->in_single_quote && !s->in_double_quote) {
      if (ch == '{') {
        s->brace_depth++;
        consumed = true;
        advance(lexer);
        continue;
      }

      if (ch == '}') {
        if (s->brace_depth > 0) {
          s->brace_depth--;
          consumed = true;
          advance(lexer);
          continue;
        }
        // brace_depth == 0: this is the outer closing `}`.
        // Stop WITHOUT consuming it — let the grammar do that.
        if (consumed) {
          lexer->result_symbol = TOKEN_COMMAND_BODY;
          return true;
        }
        return false;
      }
    }

    // ── ordinary character ───────────────────────────────────
    consumed = true;
    advance(lexer);
  }
}

bool tree_sitter_ecscript_external_scanner_scan(
    void *payload,
    TSLexer *lexer,
    const bool *valid_symbols) {

  ScannerState *s = (ScannerState *)payload;

  if (valid_symbols[TOKEN_COMMAND_BODY]) {
    // Reset scanner state before every command-body scan. The scanner only
    // tracks transient state while searching for the outer closing `}`.
    s->brace_depth = 0;
    s->in_single_quote = false;
    s->in_double_quote = false;
    s->escaped = false;
    return scan_command_body(lexer, s);
  }

  return false;
}
