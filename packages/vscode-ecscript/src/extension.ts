import * as fs from "node:fs/promises";
import * as path from "node:path";
import * as vscode from "vscode";
import { Language, Parser, Query, type QueryCapture, type Tree } from "web-tree-sitter";

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
  injectionQuery: Query;
  specs: CallableSpec[];
  // NOTE: injectionQuery is loaded and exposed but not yet consumed by VS
  // Code's language injection pipeline. A future step should use
  // query.matches() to find @injection.content capture ranges and
  // register them via VS Code's setLanguageConfiguration or
  // create virtual documents with the injected language ID.
  wasmPath: string;
  queryPath: string;
  injectionQueryPath: string;
  specsPath: string;
};

type SpecKind = "builtin" | "shell_extension" | "shell_builtin";

type CallableSpec = {
  name: string;
  kind: SpecKind;
  category: string;
  signature: string;
  summary: string;
  details: string;
  examples: string[];
};

class EcscriptTreeSitterRuntime {
  private runtimePromise?: Promise<LoadedRuntime>;
  private treeCache?: { uri: string; version: number; tree: Tree };

  constructor(private readonly context: vscode.ExtensionContext) {}

  async get(): Promise<LoadedRuntime> {
    if (!this.runtimePromise) {
      this.runtimePromise = this.load().catch((err) => {
        this.runtimePromise = undefined;
        throw err;
      });
    }
    return this.runtimePromise;
  }

  async invalidate(): Promise<void> {
    const old = this.runtimePromise;
    this.runtimePromise = undefined;
    if (old) {
      try {
        const runtime = await old;
        runtime.parser.delete();
        runtime.query.delete();
        runtime.injectionQuery.delete();
      } catch {
        // runtime was never successfully loaded; nothing to clean up
      }
    }
    // Tree cache depends on the parser; clear it when the runtime is flushed
    this.clearTreeCache();
  }

  /** Parse (or reuse a cached parse for) an editor document.
   *
   * Three providers (semantic tokens, folding, document symbols) each need
   * the parse tree.  The first one to arrive parses; the others reuse the
   * cached tree for the same document version.
   *
   * We intentionally parse from scratch on version changes.  Incremental
   * parsing requires calling Tree.edit() on the old tree before passing
   * it to parser.parse(), which is not implemented yet.  Passing an
   * un-edited old tree produces incorrect trees after insertions or
   * deletions (especially newline edits).
   *
   * The caller must NOT call tree.delete() — the cache owns the lifecycle. */
  async parseDocument(document: vscode.TextDocument): Promise<Tree | null> {
    const { parser } = await this.get();
    const uri = document.uri.toString();
    const version = document.version;

    // Cache hit: same document at same version
    if (this.treeCache && this.treeCache.uri === uri && this.treeCache.version === version) {
      return this.treeCache.tree;
    }

    // Version changed — fresh parse (no old tree, no incremental parsing).
    const text = document.getText();
    const tree = parser.parse(text) ?? null;

    if (!tree) {
      this.clearTreeCache();
      return null;
    }

    // Replace cache entry.
    if (this.treeCache) {
      this.treeCache.tree.delete();
    }
    this.treeCache = { uri, version, tree };
    return tree;
  }

  private clearTreeCache(): void {
    if (this.treeCache) {
      this.treeCache.tree.delete();
      this.treeCache = undefined;
    }
  }

  private async load(): Promise<LoadedRuntime> {
    const wasmPath = path.join(this.context.extensionPath, "assets", "tree-sitter-ecscript.wasm");
    const queryPath = path.join(this.context.extensionPath, "assets", "queries", "highlights.scm");
    const injectionQueryPath = path.join(this.context.extensionPath, "assets", "queries", "injections.scm");
    const specsPath = path.join(this.context.extensionPath, "assets", "specs.json");

    await Parser.init({
      locateFile(scriptName: string) {
        return require.resolve(`web-tree-sitter/${scriptName}`);
      }
    });

    let languageBytes: Buffer;
    let querySource: string;
    let injectionQuerySource: string;
    let specsSource: string;
    try {
      [languageBytes, querySource, injectionQuerySource, specsSource] = await Promise.all([
        fs.readFile(wasmPath),
        fs.readFile(queryPath, "utf8"),
        fs.readFile(injectionQueryPath, "utf8"),
        fs.readFile(specsPath, "utf8"),
      ]);
    } catch (err: unknown) {
      const code = (err as NodeJS.ErrnoException)?.code;
      if (code === "ENOENT") {
        throw new Error(
          "Missing ecscript extension assets. Run `just sync-vscode-assets` from the monorepo root."
        );
      }
      throw err;
    }

    const language = await Language.load(new Uint8Array(languageBytes));
    const parser = new Parser();
    parser.setLanguage(language);
    const query = new Query(language, querySource);
    const injectionQuery = new Query(language, injectionQuerySource);
    const specs = JSON.parse(specsSource) as CallableSpec[];

    return {
      parser,
      language,
      query,
      injectionQuery,
      specs,
      wasmPath,
      queryPath,
      injectionQueryPath,
      specsPath,
    };
  }
}

class EcscriptSemanticTokensProvider implements vscode.DocumentSemanticTokensProvider {
  constructor(private readonly runtime: EcscriptTreeSitterRuntime) {}

  async provideDocumentSemanticTokens(
    document: vscode.TextDocument,
    token: vscode.CancellationToken
  ): Promise<vscode.SemanticTokens> {
    if (token.isCancellationRequested) {
      return new vscode.SemanticTokensBuilder(LEGEND).build();
    }

    const tree = await this.runtime.parseDocument(document);
    if (!tree) return new vscode.SemanticTokensBuilder(LEGEND).build();
    if (token.isCancellationRequested) {
      return new vscode.SemanticTokensBuilder(LEGEND).build();
    }

    const runtime = await this.runtime.get();
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
    if (token.isCancellationRequested) return [];

    const tree = await this.runtime.parseDocument(document);
    if (!tree) return [];
    const ranges: vscode.FoldingRange[] = [];
    collectFoldingNodes(tree.rootNode, ranges);
    return ranges;
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

const SYMBOL_KINDS: Record<string, { kind: vscode.SymbolKind; detail?: string }> = {
  function_declaration: { kind: vscode.SymbolKind.Function },
  let_statement: { kind: vscode.SymbolKind.Variable },
  use_statement: { kind: vscode.SymbolKind.Namespace, detail: "module import" },
};

class EcscriptDocumentSymbolProvider implements vscode.DocumentSymbolProvider {
  constructor(private readonly runtime: EcscriptTreeSitterRuntime) {}

  async provideDocumentSymbols(
    document: vscode.TextDocument,
    token: vscode.CancellationToken,
  ): Promise<vscode.DocumentSymbol[]> {
    if (token.isCancellationRequested) return [];

    const tree = await this.runtime.parseDocument(document);
    if (!tree) return [];
    return collectDocumentSymbols(tree.rootNode, document);
  }
}

function collectDocumentSymbols(
  node: any,
  document: vscode.TextDocument,
): vscode.DocumentSymbol[] {
  const symbols: vscode.DocumentSymbol[] = [];

  for (const child of node.namedChildren) {
    const info = SYMBOL_KINDS[child.type];
    if (info) {
      // Use grammar field names to locate the name/alias node precisely.
      const nameNode =
        child.childForFieldName?.("name") ??
        child.childForFieldName?.("alias");
      if (nameNode) {
        const name = nameNode.text;
        const startLineText = document.lineAt(child.startPosition.row).text;
        const endLineText = document.lineAt(child.endPosition.row).text;
        const nameLineText = document.lineAt(nameNode.startPosition.row).text;
        const range = new vscode.Range(
          child.startPosition.row,
          byteColumnToUtf16Column(startLineText, child.startPosition.column),
          child.endPosition.row,
          byteColumnToUtf16Column(endLineText, child.endPosition.column),
        );
        const selectionRange = new vscode.Range(
          nameNode.startPosition.row,
          byteColumnToUtf16Column(nameLineText, nameNode.startPosition.column),
          nameNode.endPosition.row,
          byteColumnToUtf16Column(nameLineText, nameNode.endPosition.column),
        );
        symbols.push(
          new vscode.DocumentSymbol(
            name,
            info.detail ?? "",
            info.kind,
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


// ── Diagnostics (syntax errors from tree-sitter ERROR/MISSING nodes) ─

class EcscriptDiagnosticsProvider {
  private readonly diagnostics: vscode.DiagnosticCollection;
  private timer?: ReturnType<typeof setTimeout>;
  private readonly output: vscode.OutputChannel;

  constructor(private readonly runtime: EcscriptTreeSitterRuntime) {
    this.diagnostics = vscode.languages.createDiagnosticCollection("ecscript");
    this.output = vscode.window.createOutputChannel("ecscript");
  }

  /** Lint every already-open .ecs file (called on activation). */
  lintOpenEditors(): void {
    for (const editor of vscode.window.visibleTextEditors) {
      if (editor.document.languageId === "ecscript") {
        this.lint(editor.document);
      }
    }
  }

  onDidChange(document: vscode.TextDocument): void {
    if (document.languageId !== "ecscript") return;
    clearTimeout(this.timer);
    this.timer = setTimeout(() => this.lint(document), 300);
  }

  onDidClose(document: vscode.TextDocument): void {
    this.diagnostics.delete(document.uri);
  }

  dispose(): void {
    clearTimeout(this.timer);
    this.diagnostics.dispose();
    this.output.dispose();
  }

  private lint(document: vscode.TextDocument): void {
    this.runtime.get().then(({ parser }) => {
      const tree = parser.parse(document.getText());
      if (!tree) return;
      try {
        const errors = collectErrorDiagnostics(tree.rootNode, document);
        this.diagnostics.set(document.uri, errors);
      } finally {
        tree.delete();
      }
    }).catch((err: unknown) => {
      this.output.appendLine(
        `[diagnostics] ${document.uri.toString()}: ${renderError(err)}`
      );
    });
  }
}

function collectErrorDiagnostics(
  node: SyntaxNode,
  document: vscode.TextDocument,
): vscode.Diagnostic[] {
  const results: vscode.Diagnostic[] = [];

  function walk(n: SyntaxNode): void {
    // Detect both hard parse errors (ERROR) and missing required tokens
    // (MISSING).  Tree-sitter reports most common user mistakes — e.g.
    //   let x =     (missing value)
    //   let = 1     (missing name)
    // as MISSING nodes, not ERROR nodes.
    if (n.isError || n.isMissing) {
      // Only report the innermost error / missing node — checking
      // `children` (all children, not just named) catches nested
      // ERROR/MISSING and avoids cascading diagnostics.
      const hasNestedIssue = n.children.some(
        (c) => c != null && (c.isError || c.isMissing),
      );
      if (!hasNestedIssue) {
        const range = diagnosticRange(n, document);
        if (range) {
          const message = n.isMissing
            ? `Missing ${readableNodeLabel(n)}`
            : "Syntax error";
          results.push({
            message,
            range,
            severity: vscode.DiagnosticSeverity.Error,
            source: "ecscript",
          });
        }
      }
    }
    for (const child of n.namedChildren) {
      if (child) walk(child);
    }
  }

  walk(node);
  return results;
}

/** Build a VS Code Range for a tree-sitter node, ensuring the result
 *  has non-zero width (MISSING nodes are zero-width). */
function diagnosticRange(
  n: SyntaxNode,
  document: vscode.TextDocument,
): vscode.Range | undefined {
  const sr = n.startPosition.row;
  const sc = byteColumnToUtf16Column(
    document.lineAt(sr).text,
    n.startPosition.column,
  );
  const er = n.endPosition.row;
  let ec = byteColumnToUtf16Column(
    document.lineAt(er).text,
    n.endPosition.column,
  );
  // MISSING nodes are zero-width — extend to cover at least one
  // character so the squiggle is visible.
  if (sr === er && ec <= sc) {
    const lineText = document.lineAt(sr).text;
    ec = Math.min(sc + 1, lineText.length);
    if (ec <= sc) return undefined; // empty line
  }
  return new vscode.Range(sr, sc, er, ec);
}

/** Human-readable label for a MISSING node, derived from its grammar
 *  symbol or field name so the diagnostic message is more helpful. */
function readableNodeLabel(n: SyntaxNode): string {
  // The node type for MISSING nodes in tree-sitter is the grammar
  // rule name (e.g. "expression", "variable_identifier").
  const label = n.type
    .replace(/_/g, " ")
    .replace(/\b\w/g, (c) => c.toUpperCase());
  return label || "token";
}

const IDENTIFIER_NODE_TYPES = new Set([
  "identifier",
  "variable_identifier",
  "property_identifier",
]);

function specKindLabel(kind: SpecKind): string {
  switch (kind) {
    case "builtin":
      return "ecscript builtin";
    case "shell_extension":
      return "shell extension";
    case "shell_builtin":
      return "shell builtin";
  }
}

function nodeRange(document: vscode.TextDocument, node: SyntaxNode): vscode.Range {
  const startLineText = document.lineAt(node.startPosition.row).text;
  const endLineText = document.lineAt(node.endPosition.row).text;
  return new vscode.Range(
    node.startPosition.row,
    byteColumnToUtf16Column(startLineText, node.startPosition.column),
    node.endPosition.row,
    byteColumnToUtf16Column(endLineText, node.endPosition.column),
  );
}

type CallableTarget = {
  name: string;
  range: vscode.Range;
};

function sameNode(a: SyntaxNode | null | undefined, b: SyntaxNode | null | undefined): boolean {
  return !!a
    && !!b
    && a.type === b.type
    && a.startIndex === b.startIndex
    && a.endIndex === b.endIndex;
}

function callableTargetAtNode(
  document: vscode.TextDocument,
  node: SyntaxNode,
): CallableTarget | undefined {
  let identifier: SyntaxNode | null = node;
  while (identifier && !IDENTIFIER_NODE_TYPES.has(identifier.type)) {
    identifier = identifier.parent;
  }
  if (!identifier) {
    return undefined;
  }

  let callExpression: SyntaxNode | null | undefined = identifier.parent;
  let callee: SyntaxNode | null | undefined = undefined;

  if (callExpression?.type === "field_expression") {
    const field = callExpression.childForFieldName("field");
    if (!sameNode(field, identifier)) {
      return undefined;
    }
    const fieldExpression = callExpression;
    callExpression = fieldExpression.parent;
    callee = fieldExpression;
  } else {
    callee = identifier;
  }

  if (callExpression?.type !== "call_expression") {
    return undefined;
  }
  if (!sameNode(callExpression.childForFieldName("function"), callee)) {
    return undefined;
  }

  const range = nodeRange(document, identifier);
  return {
    name: document.getText(range),
    range,
  };
}

function formatSpecHover(specs: CallableSpec[]): vscode.MarkdownString {
  const md = new vscode.MarkdownString(undefined, true);
  md.isTrusted = false;
  for (const [index, spec] of specs.entries()) {
    if (index > 0) {
      md.appendMarkdown("\n---\n\n");
    }
    md.appendMarkdown(`**${spec.name}** (${specKindLabel(spec.kind)})\n\n`);
    md.appendCodeblock(spec.signature, "ecscript");
    md.appendMarkdown(`${spec.summary}\n\n`);
    md.appendMarkdown(`${spec.details}\n\n`);
    if (spec.examples.length > 0) {
      md.appendMarkdown("Examples:\n");
      for (const example of spec.examples.slice(0, 2)) {
        md.appendCodeblock(example, "ecscript");
      }
    }
  }
  return md;
}

// ── Hover (spec docs or node type under cursor) ───────────────────

class EcscriptHoverProvider implements vscode.HoverProvider {
  constructor(private readonly runtime: EcscriptTreeSitterRuntime) {}

  async provideHover(
    document: vscode.TextDocument,
    position: vscode.Position,
    _token: vscode.CancellationToken,
  ): Promise<vscode.Hover | undefined> {
    const tree = await this.runtime.parseDocument(document);
    if (!tree) return undefined;
    const runtime = await this.runtime.get();
    const byteCol = byteColumnFromUtf16Column(
      document.lineAt(position.line).text,
      position.character,
    );
    // Use descendantForPosition (returns any node, including anonymous
    // keywords/punctuation) and walk up to the nearest named ancestor.
    let node = tree.rootNode.descendantForPosition({
      row: position.line,
      column: byteCol,
    });
    while (node && node.type === "") {
      node = node.parent;
    }
    if (!node || node === tree.rootNode) return undefined;

    const target = callableTargetAtNode(document, node);
    if (target) {
      const specs = runtime.specs.filter((spec) => spec.name === target.name);
      if (specs.length > 0) {
        return new vscode.Hover(formatSpecHover(specs), target.range);
      }
    }

    return new vscode.Hover(
      new vscode.MarkdownString().appendCodeblock(node.type, "ecscript"),
      nodeRange(document, node),
    );
  }
}

function byteColumnFromUtf16Column(
  lineText: string,
  utf16Column: number,
): number {
  // Convert UTF-16 column back to byte offset by encoding the prefix
  // up to that column and measuring its UTF-8 byte length.
  const prefix = lineText.substring(0, utf16Column);
  return Buffer.byteLength(prefix, "utf8");
}

let _runtime: EcscriptTreeSitterRuntime | undefined;

export function activate(context: vscode.ExtensionContext): void {
  const runtime = new EcscriptTreeSitterRuntime(context);
  // Stash for deactivate() to clean up.
  _runtime = runtime;

  const status = vscode.commands.registerCommand("ecscript.showStatus", async () => {
    try {
      const loaded = await runtime.get();
      await vscode.window.showInformationMessage(
        `ecscript Tree-sitter runtime ready. wasm: ${path.basename(loaded.wasmPath)}, query: ${path.basename(loaded.queryPath)}, injection: ${path.basename(loaded.injectionQueryPath)}, specs: ${path.basename(loaded.specsPath)}`
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

  const diagnostics = new EcscriptDiagnosticsProvider(runtime);
  const onDidChange = vscode.workspace.onDidChangeTextDocument((e) =>
    diagnostics.onDidChange(e.document),
  );
  const onDidOpen = vscode.workspace.onDidOpenTextDocument((doc) =>
    diagnostics.onDidChange(doc),
  );
  const onDidClose = vscode.workspace.onDidCloseTextDocument((doc) =>
    diagnostics.onDidClose(doc),
  );
  // Lint .ecs files that were already open before activation.
  diagnostics.lintOpenEditors();

  const hoverProvider = vscode.languages.registerHoverProvider(
    [{ language: "ecscript" }],
    new EcscriptHoverProvider(runtime),
  );

  context.subscriptions.push(
    status,
    showSyntaxTree,
    semanticTokens,
    foldingProvider,
    documentSymbolProvider,
    diagnostics,
    onDidChange,
    onDidOpen,
    onDidClose,
    hoverProvider,
  );
}

export function deactivate(): void {
  // Tree-sitter parser/language/query objects use WASM heap that will be
  // freed when the module is unloaded. No explicit cleanup needed for
  // web-tree-sitter v0.25.x — the WASM module releases its memory on unload.
  _runtime?.invalidate();
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
  const candidates = captures.flatMap((c) => candidateFromCapture(c, document));

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
): TokenCandidate[] {
  const token = classifyCapture(capture.name);
  if (!token) {
    return [];
  }

  const { node } = capture;
  const startRow = node.startPosition.row;
  const endRow = node.endPosition.row;

  if (startRow === endRow) {
    // Single-line case: emit a single token
    return emitLineToken(
      document, startRow,
      node.startPosition.column, node.endPosition.column,
      token.tokenType, token.modifiers, token.priority,
    );
  }

  // Multi-line case: emit one token per line the node spans
  const results: TokenCandidate[] = [];

  // First line: from startPosition.column to end of line
  results.push(...emitLineToken(
    document, startRow,
    node.startPosition.column, null, // null → to end of line
    token.tokenType, token.modifiers, token.priority,
  ));

  // Middle lines: full lines
  for (let line = startRow + 1; line < endRow; line++) {
    results.push(...emitLineToken(
      document, line,
      0, null,
      token.tokenType, token.modifiers, token.priority,
    ));
  }

  // Last line: from 0 to endPosition.column
  results.push(...emitLineToken(
    document, endRow,
    0, node.endPosition.column,
    token.tokenType, token.modifiers, token.priority,
  ));

  return results;
}

/** Emit zero or one safe, bounds-checked semantic token for a span on a
 *  single line.  Returns `[]` when the span is empty, out-of-bounds, or
 *  the line does not exist. */
function emitLineToken(
  document: vscode.TextDocument,
  line: number,
  startByteCol: number,
  endByteCol: number | null, // null → rest of the line
  tokenType: TokenType,
  modifiers: TokenModifier[],
  priority: number,
): TokenCandidate[] {
  if (line < 0 || line >= document.lineCount) return [];

  const lineText = document.lineAt(line).text;
  if (lineText.length === 0) return [];

  // Clamp byte columns into [0, byteLength(lineText)] before converting.
  // Tree-sitter byte offsets may include the trailing newline byte that
  // VS Code's lineAt().text does NOT include, so a column pointing at
  // '\n' would overrun the line text and produce an out-of-bounds
  // semantic token — which VS Code rejects wholesale, breaking ALL
  // highlighting for the document.
  const lineBytes = Buffer.byteLength(lineText, "utf8");
  const safeStartByte = Math.max(0, Math.min(startByteCol, lineBytes));
  const safeEndByte =
    endByteCol != null
      ? Math.max(0, Math.min(endByteCol, lineBytes))
      : lineBytes;

  const startChar = byteColumnToUtf16Column(lineText, safeStartByte);
  const endChar =
    endByteCol != null
      ? byteColumnToUtf16Column(lineText, safeEndByte)
      : lineText.length;

  const length = endChar - startChar;
  if (length <= 0) return [];

  // Final safety: the token must not overrun the line.
  if (startChar + length > lineText.length) return [];

  return [{
    line,
    char: startChar,
    length,
    tokenType,
    modifiers,
    priority,
  }];
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
