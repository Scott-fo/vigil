; Vendored from nvim-treesitter (Apache-2.0). Local change: `#lua-match?` is
; rewritten as `#match?` because tree-sitter only evaluates regex predicates.
(comment) @comment @spell

(key) @property

(value) @string

(value
  (escape) @string.escape)

((value) @boolean
  (#any-of? @boolean "true" "false"))

((value) @number
  (#match? @number "^\\d+$"))

((index) @number
  (#match? @number "^\\d+$"))

((substitution
  (key) @constant)
  (#match? @constant "^[A-Z_][A-Z0-9_]*$"))

(substitution
  (key) @function
  "::" @punctuation.special
  (secret) @constant.macro)

(property
  [
    "="
    ":"
  ] @operator)

[
  "${"
  "}"
] @punctuation.special

(substitution
  ":" @punctuation.special)

[
  "["
  "]"
] @punctuation.bracket

[
  "."
  "\\"
] @punctuation.delimiter
