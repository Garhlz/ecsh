; `cmd{ ... }` 目前在 grammar 里仍然是语法岛。
; 第一版先把 command_body 注入为 shell，编辑器侧可以复用已有的 shell 高亮。
(
  (command_literal
    (command_body) @injection.content)
  (#set! injection.language "bash")
)
