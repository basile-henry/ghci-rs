use ghci::{ghci, Ghci, SharedGhci};

static GHCI: SharedGhci = SharedGhci::new(|| {
    let mut ghci = Ghci::new()?;
    ghci.import(&["Data.Char", "Data.List"])?;
    Ok(ghci)
});

#[test]
fn simple_arithmetic() -> ghci::Result<()> {
    let mut ghci = GHCI.lock();
    let out: i32 = ghci!(ghci, { 1 + 2 })?;
    assert_eq!(out, 3);
    Ok(())
}

#[test]
fn string_concat_with_bindings() -> ghci::Result<()> {
    let mut ghci = GHCI.lock();
    let x = "hello".to_string();
    let y = "world".to_string();
    let out: String = ghci!(ghci, [x, y] { x ++ " " ++ y })?;
    assert_eq!(out, "hello world");
    Ok(())
}

#[test]
fn haskell_dollar_operator() -> ghci::Result<()> {
    let mut ghci = GHCI.lock();
    let out: i32 = ghci!(ghci, { succ $ 2 })?;
    assert_eq!(out, 3);
    Ok(())
}

#[test]
fn haskell_dot_operator() -> ghci::Result<()> {
    let mut ghci = GHCI.lock();
    let out: i32 = ghci!(ghci, { (succ . succ) 1 })?;
    assert_eq!(out, 3);
    Ok(())
}

#[test]
fn list_operations_with_bindings() -> ghci::Result<()> {
    let mut ghci = GHCI.lock();
    let xs: Vec<i32> = vec![1, 2, 3];
    let out: Vec<i32> = ghci!(ghci, [xs] { map (* 2) xs })?;
    assert_eq!(out, vec![2, 4, 6]);
    Ok(())
}

#[test]
fn expression_binding() -> ghci::Result<()> {
    let mut ghci = GHCI.lock();
    let out: i32 = ghci!(ghci, [z = (1i32 + 2)] { z * 10 })?;
    assert_eq!(out, 30);
    Ok(())
}

#[test]
fn bool_result() -> ghci::Result<()> {
    let mut ghci = GHCI.lock();
    let out: bool = ghci!(ghci, { True && False })?;
    assert!(!out);
    Ok(())
}

#[test]
fn single_binding() -> ghci::Result<()> {
    let mut ghci = GHCI.lock();
    let n: i32 = 42;
    let out: i32 = ghci!(ghci, [n] { n + 1 })?;
    assert_eq!(out, 43);
    Ok(())
}

#[test]
fn binding_with_method_call() -> ghci::Result<()> {
    let mut ghci = GHCI.lock();
    let items = [10i32, 20, 30];
    let n = items.len() as i32;
    let out: String = ghci!(ghci, [n] { show n })?;
    assert_eq!(out, "3");
    Ok(())
}

#[test]
fn explicit_mut_ref() -> ghci::Result<()> {
    let mut ghci = GHCI.lock();
    // &mut ghci and &mut *ghci are also accepted
    let out: i32 = ghci!(&mut *ghci, { 7 })?;
    assert_eq!(out, 7);
    Ok(())
}
