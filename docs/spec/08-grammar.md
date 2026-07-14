# 8. Grammar

Formal grammar for the syntax fixed in §7, in EBNF. This is the grammar a parser-combinator or LR/PEG generator (§13.2) implements; it is deliberately independent of any runtime or provider concept — nothing below names a vendor, a model, or an SDK.

```ebnf
(* ---------- Lexical ---------- *)
letter        = "a".."z" | "A".."Z" | "_" ;
digit         = "0".."9" ;
ident         = letter , { letter | digit } ;
int_lit       = digit , { digit } ;
float_lit     = digit , { digit } , "." , digit , { digit } ;
string_lit    = '"' , { string_char } , '"' ;
text_block    = '"""' , { any_char_or_interp } , '"""' ;
interp        = "{" , expr , "}" ;
line_comment  = "//" , { any_char - newline } ;
block_comment = "/*" , { any_char } , "*/" ;

(* ---------- Program ---------- *)
program       = { import_decl | top_decl } ;

import_decl   = "import" , kind , ident , "from" , string_lit ;
kind          = "conversation" | "judge" | "validator" | "dataset" | "type" ;

top_decl      = conversation_decl
              | judge_decl
              | validator_decl
              | dataset_decl
              | type_decl ;

(* ---------- Conversation ---------- *)
conversation_decl
              = { doc_comment } , "conversation" , ident , param_list ,
                [ "->" , type_expr ] , block ;

param_list    = "(" , [ param , { "," , param } ] , ")" ;
param         = ident , ":" , type_expr ;

block         = "{" , { stmt } , [ expr ] , "}" ;

stmt          = message_stmt
              | with_block
              | ask_stmt
              | binding_stmt
              | match_stmt
              | retry_stmt
              | escalate_stmt
              | for_stmt
              | while_stmt
              | break_stmt
              | expr_stmt ;

(* ordinary imperative loops, §21.6 — deliberately outside any with_block,
   since a loop body is sequential-by-nature and therefore belongs to the
   imperative region, §10.2 *)
for_stmt      = "for" , ident , "in" , expr , block ;
while_stmt    = "while" , expr , block ;
break_stmt    = "break" , [ expr ] ;

(* message literals: read like the transcript they produce, §7.3 *)
message_stmt  = role_literal , ":" , text_block
              | "assistant" , "->" , ident , [ ":" , type_expr ] ;
role_literal  = "system" | "user" ;

(* explicit multimodal / capability-pinned call, §7.5 *)
ask_stmt      = "ask" , ident , "(" , [ arg_list ] , ")" , block ,
                "->" , ident , [ ":" , type_expr ] ;
arg_list      = arg , { "," , arg } ;
arg           = [ ident , ":" ] , expr ;

(* declarative independent sub-block, §7.4 *)
with_block    = "with" , "{" , { binding_stmt } , "}" ;
binding_stmt  = ident , "=" , ( judge_call | validator_call | ask_expr | expr ) ;

judge_call    = "judge" , ident , "(" , [ arg_list ] , ")" ;
validator_call= "validator" , ident , "(" , [ arg_list ] , ")" ;
ask_expr      = "ask" , ident , "(" , [ arg_list ] , ")" , block ;

(* exhaustive match over Verdict or any closed union, §7.3, §9.4 *)
match_stmt    = "match" , expr , "{" , { match_arm } , "}" ;
match_arm     = pattern , "=>" , ( expr | block ) ;
pattern       = ident , [ "(" , [ pattern_binding , { "," , pattern_binding } ] , ")" ]
              | "_" ;
pattern_binding = ident ;

retry_stmt    = "retry" , "(" , int_lit , ")" , block , [ "else" , expr ] ;
escalate_stmt = "escalate" , "(" , ident , [ "," , named_arg_list ] , ")" ;
named_arg_list= named_arg , { "," , named_arg } ;
named_arg     = ident , ":" , expr ;

expr_stmt     = expr ;

(* ---------- Judges, validators, datasets, types ---------- *)
judge_decl    = { doc_comment } , "judge" , ident , param_list , "->" , type_expr ,
                "{" , { field_assign } , "}" ;
validator_decl= { doc_comment } , "validator" , ident , param_list , "->" , type_expr ,
                "{" , { field_assign } , "}" ;
field_assign  = ident , ":" , expr ;

dataset_decl  = { doc_comment } , "dataset" , ident , ":" , type_expr ,
                "{" , ( "from" , string_lit | dataset_rows ) , "}" ;
dataset_rows  = "[" , [ record_lit , { "," , record_lit } ] , "]" ;

type_decl     = "type" , ident , "=" , type_expr ;

(* ---------- Types, §9 ---------- *)
type_expr     = artifact_type
              | record_type
              | union_type
              | generic_type
              | ident ;                       (* named/alias reference *)

artifact_type = "text" | "markdown" | "image" | "audio" | "video" | "pdf"
              | "json" | "xml" | "html" | "csv" | "embedding" | "vector"
              | "tool_output" ;

record_type   = "{" , [ field_type , { "," , field_type } ] , "}" ;
field_type    = ident , ":" , type_expr ;

union_type    = variant , { "|" , variant } ;
variant       = ident , [ "(" , type_expr , ")" ] ;   (* e.g. Fail(text) *)

generic_type  = ident , "<" , type_expr , ">" ;        (* e.g. Draft<text>, dataset<Row> *)

(* ---------- Testing / evaluation, §7.6, §16 ---------- *)
benchmark_decl= { doc_comment } , "benchmark" , ident , "{" , { benchmark_stmt } , "}" ;
benchmark_stmt= "dataset" , ":" , ident
              | "run" , ":" , expr , "->" , ident
              | "expect" , expr , "satisfies" , expr , [ "with" , "threshold" , "(" , float_lit , ")" ]
              | "assert" , expr
              | "snapshot" , expr , "as" , expr ;

(* ---------- Expressions ---------- *)
expr          = or_expr ;
or_expr       = and_expr , { "or" , and_expr } ;
and_expr      = cmp_expr , { "and" , cmp_expr } ;
cmp_expr      = add_expr , [ ( "==" | "!=" | "<" | "<=" | ">" | ">=" ) , add_expr ] ;
add_expr      = mul_expr , { ( "+" | "-" ) , mul_expr } ;
mul_expr      = unary_expr , { ( "*" | "/" ) , unary_expr } ;
unary_expr    = [ "not" | "-" ] , postfix_expr ;
postfix_expr  = primary_expr , { field_access | call | index } ;
field_access  = "." , ident ;
call          = "(" , [ arg_list ] , ")" ;
index         = "[" , expr , "]" ;
primary_expr  = int_lit | float_lit | string_lit | text_block
              | ident | record_lit | "(" , expr , ")" ;
record_lit    = "{" , [ field_assign , { "," , field_assign } ] , "}" ;

doc_comment   = "///" , { any_char - newline } ;
```

## 8.1 Notes on design choices reflected in the grammar

- **No vendor token anywhere.** `capability_ident` (the `ident` following `ask`) resolves against the stdlib's capability registry (§15.1) at semantic-analysis time, not the grammar — consistent with §4.3.
- **`with_block` bindings are syntactically restricted to a flat list with no forward or sibling reference production** — the grammar itself cannot express `with { a = ...; b = f(a) }`; that dependency must be written as a second, sequential statement outside the block. This is what makes §7.4's "provably independent" claim a parser-enforced guarantee rather than a convention (contrast Pulumi, §2.4, §3.4).
- **`match_stmt` requires no default arm** in the grammar — exhaustiveness is enforced in semantic analysis (§9.4, §13.3) against the closed variant set of the scrutinee's type, the same way Rust's grammar permits a non-exhaustive `match` syntactically but rejects it in a later compiler pass.
- **`retry_stmt`'s `else` clause is grammatically mandatory only when the retry's body's block type isn't provably total** — full rule in §9.3; a `retry` whose body cannot fail (rare, e.g. a pure validator with no model call) may omit `else`.
- **Message literals (`message_stmt`) and the explicit `ask_stmt` are two productions, not one**, deliberately: the terse `system:`/`user:`/`assistant ->` form covers the common single-capability, text-first turn (§7.3); `ask` is required the moment a step needs multimodal input, an explicit capability, or a provider policy override (§7.5) — this mirrors SQL's separate terse-`SELECT` vs. explicit-`JOIN` forms rather than collapsing everything into one maximally general but noisier production.
