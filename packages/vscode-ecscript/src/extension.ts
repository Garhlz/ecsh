import * as fs from "node:fs/promises";
import * as path from "node:path";
import * as vscode from "vscode";
import { Language, Parser, Query, type QueryCapture } from "web-tree-sitter";

type SyntaxNode = NonNullable<ReturnType<Parser["parse"]>>["rootNode"];

const TOKEN_TYPES = [
  "keywordDeclaration",
  "keywordControl",
  "keywordImport",
  "keywordCommand",
  "keyword",
  "modifier",
  "string",
  "number",
  "comment",
  "function",
  "method",
  "namespace",
  "variable",
  "parameter",
  "property",
  "operator"
] as const;

const TOKEN_MODIFIERS = [
  "declaration",
  "documentation",
  "modification",
] as const;

const LEGEND = new vscode.SemanticTokensLegend(
  [...TOKEN_TYPES],
  [...TOKEN_MODIFIERS],
);

type TokenType = typeof TOKEN_TYPES[number];
type TokenModifier = (typeof TOKEN_MODIFIERS)[number];

type TokenCandidate = {
  line: number;
  char: number;
  length: number;
  tokenType: TokenType;
  modifiers: TokenModifier[];
  priority: number;
};

type LoadedRuntime = {
  parser: Parser;
  language: Language;
  query: Query;
  wasmPath: string;
  queryPath: string;
};

class EcscriptTreeSitterRuntime {
  private runtimePromise?: Promise<LoadedRuntime>;

  constructor(private readonly context: vscode.ExtensionContext) {}

  async get(): Promise<LoadedRuntime> {
    this.runtimePromise ??= this.load();
    return this.runtimePromise;
  }

  invalidate(): void {
    this.runtimePromise = undefined;
  }

  private async load(): Promise<LoadedRuntime> {
    const wasmPath = path.join(this.context.extensionPath, "assets", "tree-sitter-ecscript.wasm");
    const queryPath = path.join(this.context.extensionPath, "assets", "queries", "highlights.scm");

    await ensureExists(wasmPath, "Missing ecscript parser wasm. Run `just sync-vscode-assets` from the monorepo root.");
    await ensureExists(queryPath, "Missing ecscript highlight query bundle.");

    await Parser.init({
      locateFile(scriptName: string) {
        return require.resolve(`web-tree-sitter/${scriptName}`);
      }
    });

    const [languageBytes, querySource] = await Promise.all([
      fs.readFile(wasmPath),
      fs.readFile(queryPath, "utf8")
    ]);

    const language = await Language.load(new Uint8Array(languageBytes));
    const parser = new Parser();
    parser.setLanguage(language);
    const query = new Query(language, querySource);

    return { parser, language, query, wasmPath, queryPath };
  }
}

class EcscriptSemanticTokensProvider implements vscode.DocumentSemanticTokensProvider {
  constructor(private readonly runtime: EcscriptTreeSitterRuntime) {}

  async provideDocumentSemanticTokens(
    document: vscode.TextDocument,
    token: vscode.CancellationToken
  ): Promise<vscode.SemanticTokens> {
    const runtime = await this.runtime.get();
    if (token.isCancellationRequested) {
      return new vscode.SemanticTokensBuilder(LEGEND).build();
    }

    const tree = runtime.parser.parse(document.getText());
    try {
      if (!tree) return new vscode.SemanticTokensBuilder(LEGEND).build();
      if (token.isCancellationRequested) {
        return new vscode.SemanticTokensBuilder(LEGEND).build();
      }

      const captures = runtime.query.captures(tree.rootNode);
      if (token.isCancellationRequested) {
        return new vscode.SemanticTokensBuilder(LEGEND).build();
      }

      const accepted = resolveCandidates(captures, document);

      const builder = new vscode.SemanticTokensBuilder(LEGEND);
      for (const candidate of accepted) {
        if (token.isCancellationRequested) {
          return new vscode.SemanticTokensBuilder(LEGEND).build();
        }
        builder.push(
          candidate.line,
          candidate.char,
          candidate.length,
          tokenTypeIndex(candidate.tokenType),
          modifierBitmask(candidate.modifiers),
        );
      }

      return builder.build();
    } finally {
      tree?.delete();
    }
  }
}

// ── Folding ranges ──────────────────────────────────────────────
// Foldable nodes are `statement_block`, `object`, `array`, and
// `command_literal` that span more than one line.

const FOLDABLE_NODE_TYPES = new Set([
  "statement_block",
  "object",
  "array",
  "command_literal",
]);

class EcscriptFoldingRangeProvider implements vscode.FoldingRangeProvider {
  constructor(private readonly runtime: EcscriptTreeSitterRuntime) {}

  async provideFoldingRanges(
    document: vscode.TextDocument,
    token: vscode.CancellationToken,
  ): Promise<vscode.FoldingRange[]> {
    const { parser } = await this.runtime.get();
    if (token.isCancellationRequested) return [];

    const tree = parser.parse(document.getText());
    try {
      if (!tree) return [];
      const ranges: vscode.FoldingRange[] = [];
      collectFoldingNodes(tree.rootNode, ranges);
      return ranges;
    } finally {
      tree?.delete();
    }
  }
}

function collectFoldingNodes(
  node: any,
  ranges: vscode.FoldingRange[],
): void {
  if (
    FOLDABLE_NODE_TYPES.has(node.type) &&
    node.childCount > 0 &&
    node.endPosition.row > node.startPosition.row
  ) {
    ranges.push(
      new vscode.FoldingRange(
        node.startPosition.row,
        node.endPosition.row,
      ),
    );
    return; // don't recurse into foldable children — outermost wins
  }
  for (const child of node.namedChildren) {
    collectFoldingNodes(child, ranges);
  }
}

// ── Document symbols (outline / breadcrumbs) ────────────────────

const SYMBOL_KINDS: Record<string, vscode.SymbolKind> = {
  function_declaration: vscode.SymbolKind.Function,
  let_statement: vscode.SymbolKind.Variable,
};

class EcscriptDocumentSymbolProvider implements vscode.DocumentSymbolProvider {
  constructor(private readonly runtime: EcscriptTreeSitterRuntime) {}

  async provideDocumentSymbols(
    document: vscode.TextDocument,
    token: vscode.CancellationToken,
  ): Promise<vscode.DocumentSymbol[]> {
    const { parser } = await this.runtime.get();
    if (token.isCancellationRequested) return [];

    const tree = parser.parse(document.getText());
    try {
      if (!tree) return [];
      return collectDocumentSymbols(tree.rootNode, document);
    } finally {
      tree?.delete();
    }
  }
}

function collectDocumentSymbols(
  node: any,
  document: vscode.TextDocument,
): vscode.DocumentSymbol[] {
  const symbols: vscode.DocumentSymbol[] = [];

  for (const child of node.namedChildren) {
    const kind = SYMBOL_KINDS[child.type];
    if (kind) {
      const name = childName(child);
      if (name) {
        const startLineText = document.lineAt(child.startPosition.row).text;
        const endLineText = document.lineAt(child.endPosition.row).text;
        const range = new vscode.Range(
          child.startPosition.row,
          byteColumnToUtf16Column(startLineText, child.startPosition.column),
          child.endPosition.row,
          byteColumnToUtf16Column(endLineText, child.endPosition.column),
        );
        const selectionRange = new vscode.Range(
          child.startPosition.row,
          byteColumnToUtf16Column(startLineText, child.startPosition.column),
          child.endPosition.row,
          byteColumnToUtf16Column(endLineText, child.endPosition.column),
        );
        symbols.push(
          new vscode.DocumentSymbol(
            name,
            /* detail */ "",
            kind,
            range,
            selectionRange,
          ),
        );
      }
      continue; // don't recurse into symbols
    }
    symbols.push(...collectDocumentSymbols(child, document));
  }

  return symbols;
}

function childName(node: any): string | undefined {
  // The first identifier-like named child is the declaration name.
  for (const child of node.namedChildren) {
    if (
      child.type === "variable_identifier" ||
      child.type === "identifier"
    ) {
      return child.text;
    }
  }
  return undefined;
}

export function activate(context: vscode.ExtensionContext): void {
  const runtime = new EcscriptTreeSitterRuntime(context);

  const status = vscode.commands.registerCommand("ecscript.showStatus", async () => {
    try {
      const loaded = await runtime.get();
      await vscode.window.showInformationMessage(
        `ecscript Tree-sitter runtime ready. wasm: ${path.basename(loaded.wasmPath)}, query: ${path.basename(loaded.queryPath)}`
      );
    } catch (error) {
      await vscode.window.showWarningMessage(renderError(error));
    }
  });

  const showSyntaxTree = vscode.commands.registerCommand("ecscript.showSyntaxTree", async () => {
    const editor = vscode.window.activeTextEditor;
    if (!editor || editor.document.languageId !== "ecscript") {
      await vscode.window.showWarningMessage("Open an ecscript document first.");
      return;
    }

    try {
      const loaded = await runtime.get();
      const tree = loaded.parser.parse(editor.document.getText());
      try {
        const root = tree?.rootNode?.toString() ?? "<parse failed>";

        const doc = await vscode.workspace.openTextDocument({
          language: "lisp",
          content: root
        });
        await vscode.window.showTextDocument(doc, { preview: true });
      } finally {
        tree?.delete();
      }
    } catch (error) {
      await vscode.window.showWarningMessage(renderError(error));
    }
  });

  const semanticTokens = vscode.languages.registerDocumentSemanticTokensProvider(
    [{ language: "ecscript" }],
    new EcscriptSemanticTokensProvider(runtime),
    LEGEND
  );

  const foldingProvider = vscode.languages.registerFoldingRangeProvider(
    [{ language: "ecscript" }],
    new EcscriptFoldingRangeProvider(runtime),
  );

  const documentSymbolProvider = vscode.languages.registerDocumentSymbolProvider(
    [{ language: "ecscript" }],
    new EcscriptDocumentSymbolProvider(runtime),
  );

  context.subscriptions.push(
    status,
    showSyntaxTree,
    semanticTokens,
    foldingProvider,
    documentSymbolProvider,
  );
}

export function deactivate(): void {}

async function ensureExists(filePath: string, message: string): Promise<void> {
  try {
    await fs.access(filePath);
  } catch {
    throw new Error(message);
  }
}

function renderError(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

function tokenTypeIndex(tokenType: TokenType): number {
  return TOKEN_TYPES.indexOf(tokenType);
}

function byteColumnToUtf16Column(lineText: string, byteColumn: number): number {
  const bytes = Buffer.from(lineText, "utf8");
  const prefix = bytes.subarray(0, byteColumn).toString("utf8");
  return prefix.length;
}

// ── Capture-to-token lookup table ──────────────────────────────
// Each capture name maps to a VS Code token type, optional modifier,
// and priority. Higher priority wins when multiple captures cover the
// same byte range. Entries set to `null` are always skipped.

type TokenMapping = {
  tokenType: TokenType;
  modifiers: TokenModifier[];
  priority: number;
};

const CAPTURE_TOKEN_MAP: Record<string, TokenMapping | null> = {
  "comment":            { tokenType: "comment",    modifiers: [], priority: 10 },

  // Keywords
  "keyword.declaration": { tokenType: "keywordDeclaration", modifiers: ["declaration"], priority: 80 },
  "keyword.modifier":    { tokenType: "modifier",           modifiers: [],              priority: 80 },
  "keyword.control":     { tokenType: "keywordControl",     modifiers: [],              priority: 80 },
  "keyword.import":      { tokenType: "keywordImport",      modifiers: [],              priority: 80 },
  "keyword.command":     { tokenType: "keywordCommand",     modifiers: [],              priority: 90 },

  // Functions
  "function.declaration": { tokenType: "function", modifiers: ["declaration"], priority: 100 },
  "function.call":        { tokenType: "function", modifiers: [],             priority:  90 },
  "function.method":      { tokenType: "method",   modifiers: [],             priority: 100 },

  // Variables
  "parameter":             { tokenType: "parameter", modifiers: [],               priority: 90 },
  "variable.declaration":  { tokenType: "variable",  modifiers: ["declaration"],  priority: 80 },
  "variable":              { tokenType: "variable",  modifiers: [],               priority: 40 },

  // Properties
  "property":  { tokenType: "property",  modifiers: [], priority: 70 },
  "namespace": { tokenType: "namespace", modifiers: [], priority: 90 },

  // Literals
  "string":             { tokenType: "string", modifiers: [],                     priority: 70 },
  "string.special":     { tokenType: "string", modifiers: ["documentation"],      priority: 80 },
  "string.special.path":{ tokenType: "string", modifiers: [],                     priority: 85 },
  "number":             { tokenType: "number", modifiers: [],                     priority: 70 },
  "constant.builtin":   { tokenType: "keyword", modifiers: [],                    priority: 70 },

  // Operators
  "operator":      { tokenType: "operator", modifiers: [],                   priority: 50 },
  "operator.pipe": { tokenType: "operator", modifiers: ["modification"],     priority: 80 },

  // Excluded (structural punctuation)
  "punctuation.delimiter": null,
};

function classifyCapture(name: string): { tokenType: TokenType; modifiers: TokenModifier[]; priority: number } | null {
  const entry = CAPTURE_TOKEN_MAP[name];
  if (!entry) return null;
  return entry;
}

// ── Semantic token conflict resolution ─────────────────────────
// Sorts candidates by priority descending so that higher-priority
// captures are accepted first. A candidate is only kept if it does
// not overlap any already-accepted (higher-priority) candidate.
// The final list is re-sorted by source position for VS Code output.

function resolveCandidates(captures: QueryCapture[], document: vscode.TextDocument): TokenCandidate[] {
  const candidates = captures
    .map((c) => candidateFromCapture(c, document))
    .filter((candidate): candidate is TokenCandidate => candidate !== null);

  // Sort by priority descending first — highest-priority wins
  candidates.sort((a, b) => {
    if (a.priority !== b.priority) return b.priority - a.priority;
    // Stable fallback for equal priority
    if (a.line !== b.line) return a.line - b.line;
    if (a.char !== b.char) return a.char - b.char;
    return b.length - a.length;
  });

  const accepted: TokenCandidate[] = [];

  for (const candidate of candidates) {
    const hasOverlap = accepted.some((existing) =>
      overlaps(existing, candidate)
    );
    if (!hasOverlap) {
      accepted.push(candidate);
    }
  }

  // Re-sort by source position for the builder
  accepted.sort((a, b) => {
    if (a.line !== b.line) return a.line - b.line;
    if (a.char !== b.char) return a.char - b.char;
    return b.length - a.length;
  });

  return accepted;
}

function candidateFromCapture(
  capture: QueryCapture,
  document: vscode.TextDocument,
): TokenCandidate | null {
  const token = classifyCapture(capture.name);
  if (!token) {
    return null;
  }

  const { node } = capture;
  if (node.startPosition.row !== node.endPosition.row) {
    return null;
  }

  const lineText = document.lineAt(node.startPosition.row).text;
  const startChar = byteColumnToUtf16Column(lineText, node.startPosition.column);
  const endChar = byteColumnToUtf16Column(lineText, node.endPosition.column);
  const length = endChar - startChar;
  if (length <= 0) {
    return null;
  }

  return {
    line: node.startPosition.row,
    char: startChar,
    length,
    tokenType: token.tokenType,
    modifiers: token.modifiers,
    priority: token.priority,
  };
}

function modifierBitmask(modifiers: readonly TokenModifier[]): number {
  let mask = 0;
  for (const m of modifiers) {
    const idx = TOKEN_MODIFIERS.indexOf(m);
    if (idx >= 0) mask |= 1 << idx;
  }
  return mask;
}

function overlaps(a: TokenCandidate, b: TokenCandidate): boolean {
  if (a.line !== b.line) {
    return false;
  }

  const aEnd = a.char + a.length;
  const bEnd = b.char + b.length;
  return a.char < bEnd && b.char < aEnd;
}
