# tree-sitter-ecscript

`ecscript` 的 Tree-sitter grammar 子项目。

## 当前状态

- `grammar.js`：覆盖 ecscript 阶段 9 语法（表达式、语句、控制流、函数/闭包、lambda、复合赋值、`use`/`pub`、`|>` 管道、`for` 专用 range）
- `queries/highlights.scm`：关键字（按声明/控制/导入/命令/可见性分类）、字面量、函数、变量、操作符的高亮分组
- `queries/locals.scm`：函数/参数/变量的定义与引用导航
- `cmd{ ... }` 作为语法岛处理，内部由 external scanner 识别边界并当整块文本保留
- 已生成 parser 并能通过 `npm run generate` / `npm test`

当前 `|>` 语法与运行时解释器保持一致：

- 右侧必须是调用表达式
- `|>` 后允许一个或多个换行
- 多行值流链在 grammar 与 runtime 中应得到相同的解析结果

注意：`1..10` / `1..=10` 只在 `for` 语句中合法；需要值时使用 `range(start, end)`。

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

插件注册了四种自定义 semantic token 类型来区分不同关键字类别
（`keywordDeclaration`、`keywordControl`、`keywordImport`、`keywordCommand`）。

若想在 VS Code 中手动定制高亮，可在 `settings.json` 中添加：

```json
{
  "editor.semanticTokenColorCustomizations": {
    "enabled": true,
    "rules": {
      "keywordDeclaration:ecscript":   "#C586C0",
      "keywordDeclaration.declaration:ecscript": "#C586C0",
      "keywordControl:ecscript":       "#569CD6",
      "keywordImport:ecscript":        "#4EC9B0",
      "keywordCommand:ecscript":       "#D7BA7D",
      "keyword.modifier:ecscript":     "#DCDCAA",
      "function.declaration:ecscript": "#DCDCAA",
      "function.call:ecscript":        "#DCDCAA",
      "method:ecscript":               "#DCDCAA",
      "parameter:ecscript":            "#9CDCFE",
      "property:ecscript":             "#4FC1FF",
      "namespace:ecscript":            "#C586C0",
      "operator.modification:ecscript":"#D4D4D4"
    }
  }
}
```

### 自定义 token 类型说明

| Token 类型 | capture | 对应关键字 |
|-----------|---------|-----------|
| `keywordDeclaration` | `keyword.declaration` | `let`, `func` |
| `keywordControl` | `keyword.control` | `if`, `else`, `while`, `for`, `in`, `return` |
| `keywordImport` | `keyword.import` | `use`, `as` |
| `keywordCommand` | `keyword.command` | `cmd` |
| `modifier` | `keyword.modifier` | `pub` |

以上仅为示例，按个人主题调整。如果主题未定义对应规则，将回退到
`package.json` 中声明的 TextMate scope（`storage.type.ecscript` 等）。

## 设计基线

语法基线来自：

- [../../docs/ecscript-manual.md](../../docs/ecscript-manual.md)
- [../../src/ecscript/ast.rs](../../src/ecscript/ast.rs)
- [../../src/ecscript/lexer.rs](../../src/ecscript/lexer.rs)
- [../../src/ecscript/parser.rs](../../src/ecscript/parser.rs)
- [../../src/ecscript/pratt.rs](../../src/ecscript/pratt.rs)
