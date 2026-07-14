//! Golden-file parser tests (§13.2, §25 testing plan): every `.ulx` file
//! under `examples/` at the repo root must parse successfully end-to-end
//! (lexer -> parser -> full `Program`, with `end()` enforcing that the
//! entire file was consumed, not just a prefix).

use std::path::Path;

fn examples_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

#[test]
fn all_examples_parse() {
    let dir = examples_dir();
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).expect("examples/ directory must exist") {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ulx") {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        match ulx_syntax::parse_source(&src) {
            Ok(program) => {
                checked += 1;
                assert!(
                    !program.decls.is_empty(),
                    "{} parsed but declared nothing",
                    path.display()
                );
            }
            Err(errors) => {
                panic!("{} failed to parse: {:#?}", path.display(), errors);
            }
        }
    }
    assert!(
        checked >= 8,
        "expected at least 8 example files, found {checked}"
    );
}
