//! XPILE-RUSTDICTKEY-001 (xpile#2165, epic #2126): a dict subscript with a
//! literal key that cannot match the dict key type was accepted at exit 0 and
//! emitted a lookup rustc rejects (E0277). Refuse at transpile time.

use std::process::Command;

fn run(name: &str, src: &str) -> (bool, String, String) {
    let dir = std::env::temp_dir().join(format!("xpile-dictkey-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let py = dir.join(format!("{name}.py"));
    std::fs::write(&py, src).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_xpile"))
        .args(["transpile", py.to_str().unwrap(), "--target", "rust"])
        .output()
        .unwrap();
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

fn refused(name: &str, src: &str) {
    let (ok, out, err) = run(name, src);
    assert!(!ok, "{name}: accepted; emitted:\n{out}");
    assert!(err.contains("2165"), "{name}: wrong refusal: {err}");
}

#[test]
fn int_key_into_str_keyed_dict_is_refused() {
    refused(
        "k_int_str",
        "def g() -> int:\n    d = {\"x\": 1}\n    return d[42]\n",
    );
}

#[test]
fn str_key_into_int_keyed_dict_is_refused() {
    refused(
        "k_str_int",
        "def g() -> int:\n    d = {1: 1}\n    return d[\"x\"]\n",
    );
}

#[test]
fn tuple_arity_mismatch_is_refused() {
    refused(
        "k_arity",
        "def g() -> int:\n    d = {(1, 2): 1}\n    return d[(1, 2, 3)]\n",
    );
}

#[test]
fn nested_tuple_element_mismatch_is_refused() {
    refused(
        "k_nested",
        "def g() -> int:\n    d = {(1, 2): 1}\n    return d[(1, \"a\")]\n",
    );
}

#[test]
fn tuple_key_into_int_keyed_dict_is_refused() {
    refused(
        "k_tup_int",
        "def g() -> int:\n    d = {1: 1}\n    return d[(1, 2)]\n",
    );
}

#[test]
fn control_matching_keys_still_transpile() {
    for (n, s) in [
        (
            "c_str",
            "def g() -> int:\n    d = {\"x\": 1}\n    return d[\"x\"]\n",
        ),
        (
            "c_int",
            "def g() -> int:\n    d = {1: 1}\n    return d[1]\n",
        ),
        (
            "c_bool",
            "def g() -> int:\n    d = {1: 1}\n    return d[True]\n",
        ),
        (
            "c_tup",
            "def g() -> int:\n    d = {(1, 2): 1}\n    return d[(1, 2)]\n",
        ),
    ] {
        let (ok, _, err) = run(n, s);
        assert!(ok, "{n}: control refused: {err}");
    }
}
