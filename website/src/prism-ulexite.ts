// A Prism grammar for `.ulx` source, ported from the same token categories
// as the VS Code extension's TextMate grammar
// (tooling/vscode-ulx/syntaxes/ulx.tmLanguage.json) — kept in sync by
// hand, not generated, since Prism and TextMate grammars use different
// pattern-composition rules (Prism resolves overlapping matches by grammar
// key order; TextMate by its `patterns` array order), so a literal
// automatic port isn't possible, but the same token boundaries are.
//
// Token names deliberately reuse Prism's own standard vocabulary
// (`comment`, `string`, `keyword`, `function`, `number`, `operator`,
// `punctuation`, `class-name`, `property`, `builtin`) rather than
// inventing custom ones, so the theme's existing prism-react-renderer
// themes (github/dracula, configured in docusaurus.config.ts) style
// everything correctly with no extra CSS.
import type * as PrismNamespace from 'prismjs';

export default function ulexiteLanguage(Prism: typeof PrismNamespace): void {
  Prism.languages.ulexite = {
    comment: [
      {
        pattern: /\/\/\/.*/,
        alias: 'doc-comment',
      },
      /\/\/.*/,
      {
        pattern: /\/\*[\s\S]*?\*\//,
      },
    ],
    'triple-string': {
      // """...""" text blocks, with {interpolation} spans tokenized as
      // embedded code — mirrors the TextMate grammar's `text-block` rule.
      pattern: /"""[\s\S]*?"""/,
      greedy: true,
      alias: 'string',
      inside: {
        interpolation: {
          pattern: /\{[^{}]*\}/,
          inside: {
            punctuation: /^\{|\}$/,
            variable: /[A-Za-z_][A-Za-z0-9_]*/,
          },
        },
      },
    },
    string: {
      pattern: /"(?:\\.|[^"\\])*"/,
      greedy: true,
    },
    keyword:
      /\b(?:conversation|judge|validator|dataset|type|provider|benchmark|import|with|ask|match|retry|else|escalate|for|in|while|break|from|as|satisfies|threshold|expect|assert|snapshot|run|and|or|not|if)\b/,
    builtin:
      /\b(?:system|user|assistant)\b(?=\s*(?::|->))|\b(?:text|markdown|image|audio|video|pdf|json|xml|html|csv|embedding|vector|tool_output)\b/,
    property: {
      // A record/provider/rubric field name or named-arg label —
      // `vendor: "..."`, `ask chat(temperature: 0.7)` — any lowercase
      // identifier immediately followed by `:` (not `::`).
      pattern: /\b[a-z_][A-Za-z0-9_]*\b(?=\s*:(?!:))/,
    },
    function: {
      pattern: /\b[a-z_][A-Za-z0-9_]*\b(?=\s*\()/,
    },
    'class-name': {
      // Capitalized identifiers: type names and closed-union variants
      // (Verdict, Pass, Fail, Score, Escalate, Draft, ...).
      pattern: /\b[A-Z][A-Za-z0-9_]*\b/,
    },
    number: /\b\d+(?:\.\d+)?\b/,
    variable: /\$/,
    operator: /->|=>|==|!=|<=|>=|[=<>+\-*/|.]/,
    punctuation: /[{}()[\],:]/,
  };
}
