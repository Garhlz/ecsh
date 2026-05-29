# tree-sitter-ecscript

`ecscript` 的 Tree-sitter grammar 子项目。

## 当前状态

- `grammar.js`：覆盖 ecscript stage 7 全部语法（表达式、语句、控制流、函数/闭包、lambda、复合赋值、`use`/`pub`、`|>` 管道）
- `queries/highlights.scm`：关键字、字面量、函数、变量、操作符的高亮分组
- `queries/locals.scm`：函数/参数/变量的定义与引用导航
- `cmd{ ... }` 作为语法岛处理，内部由 external scanner 识别边界并当整块文本保留
- 已生成 parser 并能通过 `npm run generate` / `npm test`

### cmd{} 语法岛实现方式

`cmd{ ... }` 当前通过 `src/scanner.c` 里的 external scanner 识别边界。

scanner 负责维护最小状态机：

- brace depth
- single quote state
- double quote state
- escape state

因此下面这类输入都能作为完整 `command_literal` 保留下来，而不会在第一个 `}` 处截断：

- `cmd{ echo "}" }`
- `cmd{ awk '{print $1}' file }`
- `cmd{ echo ${HOME} }`
- `cmd{ if true; then echo ok; fi }`

注意：

- scanner 只负责找到语法岛边界
- scanner 不会把内部内容解析成 shell AST
- 内部 shell 语义仍由 `ecsh` / `ecscript` runtime 处理

## 目录

```text
tree-sitter-ecscript/
  grammar.js
  package.json
  tree-sitter.json
  queries/
    highlights.scm
    locals.scm
  test/
    corpus/          -- ( corpus tests to be added incrementally )
  src/               -- ( generated parser )
```

## 使用

```bash
npm install
npm run generate
npm test
```

同步 VS Code 插件需要的 query / wasm 资产：

```bash
just sync-vscode-assets
```

## 编辑器主题自定义（可选）

插件提供的是 semantic tokens，不强制覆盖用户主题颜色。若想在 VS Code 中手动定制高亮，可在 `settings.json` 中添加：

```json
{
  "editor.semanticTokenColorCustomizations": {
    "enabled": true,
    "rules": {
      "keyword.declaration:ecscript": "#C586C0",
      "keyword.control:ecscript":     "#C586C0",
      "keyword.import:ecscript":      "#569CD6",
      "function.declaration:ecscript": "#DCDCAA",
      "function.call:ecscript":       "#DCDCAA",
      "method:ecscript":              "#DCDCAA",
      "parameter:ecscript":           "#9CDCFE",
      "property:ecscript":            "#4FC1FF",
      "namespace:ecscript":           "#C586C0",
      "operator.modification:ecscript": "#D4D4D4"
    }
  }
}
```

以上仅为示例，按个人主题调整。

## 设计基线

语法基线来自：

- [../../docs/ecscript-manual.md](../../docs/ecscript-manual.md)
- [../../src/ecscript/ast.rs](../../src/ecscript/ast.rs)
- [../../src/ecscript/lexer.rs](../../src/ecscript/lexer.rs)
- [../../src/ecscript/parser.rs](../../src/ecscript/parser.rs)
- [../../src/ecscript/pratt.rs](../../src/ecscript/pratt.rs)
