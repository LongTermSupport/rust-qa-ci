#![crate_type = "lib"]

// A level chosen by the caller: the definition holds `#[$level(...)]`, which no syntax scan can
// read as a lint attribute, and the invocation holds a bare level name.
macro_rules! quiet {
    ($level:ident, $($item:tt)*) => {
        #[$level(clippy::unwrap_used)]
        $($item)*
    };
}

// A lint name chosen by the caller, wrapped around the caller's item.
macro_rules! silence {
    ($lint:path => $item:item) => {
        #[expect($lint, reason = "generated")]
        $item
    };
}

// A level chosen by the caller on a statement inside the caller's function.
macro_rules! quiet_let {
    ($level:ident, $name:ident = $value:expr) => {
        #[$level(unused_variables)]
        let $name = $value;
    };
}

// Fires: generated level over caller-written code.
quiet!(allow, pub fn first(value: Option<u8>) -> u8 { value.unwrap() });

// Fires: generated `expect` with a caller-chosen lint over a caller-written item.
silence!(dead_code => fn second() -> u8 { 2 });

// Fires: generated level over a caller-written statement.
pub fn caller() {
    quiet_let!(allow, unused = 3);
}

// Quiet: the attribute is written literally in source, where the syntactic suppression guard
// sees it and requires a project-record exception.
#[expect(dead_code, reason = "fixture")]
fn literal() {}

// Quiet: written literally inside a macro definition, also visible to the syntactic guard.
macro_rules! literal_in_macro {
    ($($item:tt)*) => {
        #[expect(dead_code, reason = "fixture")]
        $($item)*
    };
}
literal_in_macro!(fn third() {});

// Quiet: the generated attribute covers only generated code; nothing the caller wrote is
// silenced.
macro_rules! helper {
    ($level:ident) => {
        #[$level(dead_code)]
        fn helper() {
            let _ = 1 + 1;
        }
    };
}
helper!(allow);

// Quiet: standard-library macros and desugarings (`matches!`, `?`) emit their own lint attributes
// over caller expressions; like any dependency's lint levels, they are outside the project policy.
pub fn std_macros(value: Option<u8>) -> Option<bool> {
    let byte = value?;
    Some(matches!(byte, 1 | 2))
}

// Quiet: the enclosing item carries an expectation for this lint, which the syntactic guard in
// turn requires to be recorded.
#[cfg_attr(dylint_lib = "rqaci_lints", expect(rqaci_suppression_generated, reason = "fixture"))]
pub mod recorded {
    quiet!(allow, pub fn fourth(value: Option<u8>) -> u8 { value.unwrap() });
}
