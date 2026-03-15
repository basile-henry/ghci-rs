use ghci::{FromHaskell, ToHaskell};

// ── Named struct ─────────────────────────────────────────────────────

#[derive(Debug, PartialEq, ToHaskell, FromHaskell)]
struct Point {
    x: u32,
    y: u32,
}

#[test]
fn named_struct_to_haskell() {
    let p = Point { x: 1, y: 2 };
    assert_eq!(p.to_haskell(), "Point {x = 1, y = 2}");
}

#[test]
fn named_struct_from_haskell() {
    let p = Point::from_haskell("Point {x = 1, y = 2}").unwrap();
    assert_eq!(p, Point { x: 1, y: 2 });
}

#[test]
fn named_struct_roundtrip() {
    let p = Point { x: 42, y: 99 };
    assert_eq!(Point::from_haskell(&p.to_haskell()).unwrap(), p);
}

// ── Tuple struct ─────────────────────────────────────────────────────

#[derive(Debug, PartialEq, ToHaskell, FromHaskell)]
struct Pair(u32, bool);

#[test]
fn tuple_struct_to_haskell() {
    assert_eq!(Pair(1, true).to_haskell(), "(Pair 1 True)");
}

#[test]
fn tuple_struct_from_haskell() {
    assert_eq!(Pair::from_haskell("(Pair 1 True)").unwrap(), Pair(1, true));
}

#[test]
fn tuple_struct_roundtrip() {
    let p = Pair(7, false);
    assert_eq!(Pair::from_haskell(&p.to_haskell()).unwrap(), p);
}

// ── Unit struct ──────────────────────────────────────────────────────

#[derive(Debug, PartialEq, ToHaskell, FromHaskell)]
struct Unit;

#[test]
fn unit_struct_to_haskell() {
    assert_eq!(Unit.to_haskell(), "Unit");
}

#[test]
fn unit_struct_from_haskell() {
    assert_eq!(Unit::from_haskell("Unit").unwrap(), Unit);
}

// ── Transparent newtype ──────────────────────────────────────────────

#[derive(Debug, PartialEq, ToHaskell, FromHaskell)]
#[haskell(transparent)]
struct Meters(f64);

#[test]
fn transparent_to_haskell() {
    assert_eq!(Meters(3.14).to_haskell(), 3.14f64.to_haskell());
}

#[test]
fn transparent_from_haskell() {
    let m = Meters::from_haskell("3.14").unwrap();
    assert_eq!(m, Meters(3.14));
}

// ── Custom Haskell names ─────────────────────────────────────────────

#[derive(Debug, PartialEq, ToHaskell, FromHaskell)]
#[haskell(name = "Vec2")]
struct Vec2 {
    #[haskell(name = "vecX")]
    x: f64,
    #[haskell(name = "vecY")]
    y: f64,
}

#[test]
fn custom_name_to_haskell() {
    let v = Vec2 { x: 1.0, y: 2.0 };
    assert_eq!(v.to_haskell(), "Vec2 {vecX = 1.0, vecY = 2.0}");
}

#[test]
fn custom_name_from_haskell() {
    let v = Vec2::from_haskell("Vec2 {vecX = 1.0, vecY = 2.0}").unwrap();
    assert_eq!(v, Vec2 { x: 1.0, y: 2.0 });
}

// ── Enum ─────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq, ToHaskell, FromHaskell)]
enum Shape {
    Circle { radius: f64 },
    Rect(f64, f64),
    Empty,
}

#[test]
fn enum_named_variant_to_haskell() {
    assert_eq!(
        Shape::Circle { radius: 2.5 }.to_haskell(),
        "Circle {radius = 2.5}"
    );
}

#[test]
fn enum_named_variant_from_haskell() {
    assert_eq!(
        Shape::from_haskell("Circle {radius = 2.5}").unwrap(),
        Shape::Circle { radius: 2.5 }
    );
}

#[test]
fn enum_tuple_variant_to_haskell() {
    assert_eq!(Shape::Rect(3.0, 4.0).to_haskell(), "(Rect 3.0 4.0)");
}

#[test]
fn enum_tuple_variant_from_haskell() {
    assert_eq!(
        Shape::from_haskell("(Rect 3.0 4.0)").unwrap(),
        Shape::Rect(3.0, 4.0)
    );
}

#[test]
fn enum_unit_variant_to_haskell() {
    assert_eq!(Shape::Empty.to_haskell(), "Empty");
}

#[test]
fn enum_unit_variant_from_haskell() {
    assert_eq!(Shape::from_haskell("Empty").unwrap(), Shape::Empty);
}

#[test]
fn enum_roundtrips() {
    for shape in [
        Shape::Circle { radius: 1.0 },
        Shape::Rect(2.0, 3.0),
        Shape::Empty,
    ] {
        let s = shape.to_haskell();
        assert_eq!(Shape::from_haskell(&s).unwrap(), shape);
    }
}

// ── Generic struct ───────────────────────────────────────────────────

#[derive(Debug, PartialEq, ToHaskell, FromHaskell)]
struct Wrapper<T> {
    value: T,
}

#[test]
fn generic_struct_roundtrip() {
    let w = Wrapper { value: 42u32 };
    assert_eq!(Wrapper::<u32>::from_haskell(&w.to_haskell()).unwrap(), w);
}

// ── Enum with custom variant names ───────────────────────────────────

#[derive(Debug, PartialEq, ToHaskell, FromHaskell)]
enum Color {
    #[haskell(name = "Red")]
    Red,
    #[haskell(name = "Green")]
    Green,
    #[haskell(name = "Blue")]
    Blue,
}

#[test]
fn enum_custom_variant_names() {
    assert_eq!(Color::Red.to_haskell(), "Red");
    assert_eq!(Color::from_haskell("Green").unwrap(), Color::Green);
}

// ── style = "app" on named struct ───────────────────────────────────

#[derive(Debug, PartialEq, ToHaskell, FromHaskell)]
#[haskell(style = "app")]
struct Tagged {
    label: usize,
    active: bool,
}

#[test]
fn app_style_named_struct_to_haskell() {
    let t = Tagged {
        label: 42,
        active: true,
    };
    assert_eq!(t.to_haskell(), "(Tagged 42 True)");
}

#[test]
fn app_style_named_struct_from_haskell() {
    assert_eq!(
        Tagged::from_haskell("(Tagged 42 True)").unwrap(),
        Tagged {
            label: 42,
            active: true
        }
    );
}

#[test]
fn app_style_named_struct_roundtrip() {
    let t = Tagged {
        label: 7,
        active: false,
    };
    assert_eq!(Tagged::from_haskell(&t.to_haskell()).unwrap(), t);
}

// ── style = "app" on enum variants (with lowercase renamed constructors) ─

#[derive(Debug, PartialEq, ToHaskell, FromHaskell)]
enum Strategy {
    #[haskell(name = "greedyStrategy", style = "app")]
    Greedy { limit: usize },
    #[haskell(name = "lazyStrategy", style = "app")]
    Lazy { threshold: usize },
}

#[test]
fn app_style_enum_variant_to_haskell() {
    assert_eq!(
        Strategy::Greedy { limit: 4 }.to_haskell(),
        "(greedyStrategy 4)"
    );
    assert_eq!(
        Strategy::Lazy { threshold: 10 }.to_haskell(),
        "(lazyStrategy 10)"
    );
}

#[test]
fn app_style_enum_variant_from_haskell() {
    assert_eq!(
        Strategy::from_haskell("(greedyStrategy 4)").unwrap(),
        Strategy::Greedy { limit: 4 }
    );
    assert_eq!(
        Strategy::from_haskell("(lazyStrategy 10)").unwrap(),
        Strategy::Lazy { threshold: 10 }
    );
}

#[test]
fn app_style_enum_variant_roundtrip() {
    for s in [
        Strategy::Greedy { limit: 0 },
        Strategy::Lazy { threshold: 99 },
    ] {
        assert_eq!(Strategy::from_haskell(&s.to_haskell()).unwrap(), s);
    }
}

// ── Container-level style = "app" on enum ───────────────────────────

#[derive(Debug, PartialEq, ToHaskell, FromHaskell)]
#[haskell(style = "app")]
enum Move {
    Forward { steps: u32 },
    Backward { steps: u32 },
}

#[test]
fn container_app_style_enum() {
    assert_eq!(Move::Forward { steps: 5 }.to_haskell(), "(Forward 5)");
    assert_eq!(
        Move::from_haskell("(Backward 3)").unwrap(),
        Move::Backward { steps: 3 }
    );
}

// ── skip in app style ───────────────────────────────────────────────

#[derive(Debug, PartialEq, ToHaskell, FromHaskell)]
#[haskell(style = "app")]
struct Config {
    enabled: bool,
    level: usize,
    verbose: bool,
    #[haskell(skip)]
    cached: usize,
}

#[test]
fn skip_field_app_to_haskell() {
    let c = Config {
        enabled: true,
        level: 3,
        verbose: false,
        cached: 999,
    };
    assert_eq!(c.to_haskell(), "(Config True 3 False)");
}

#[test]
fn skip_field_app_from_haskell() {
    let c = Config::from_haskell("(Config True 3 False)").unwrap();
    assert!(c.enabled);
    assert_eq!(c.level, 3);
    assert!(!c.verbose);
    assert_eq!(c.cached, 0); // Default::default()
}

// ── skip in app style roundtrip ──────────────────────────────────────

#[test]
fn skip_field_app_roundtrip() {
    let c = Config {
        enabled: false,
        level: 5,
        verbose: true,
        cached: 123,
    };
    let parsed = Config::from_haskell(&c.to_haskell()).unwrap();
    assert_eq!(parsed.enabled, c.enabled);
    assert_eq!(parsed.level, c.level);
    assert_eq!(parsed.verbose, c.verbose);
    assert_eq!(parsed.cached, 0); // skipped, so default
}

// ── skip in record style ────────────────────────────────────────────

#[derive(Debug, PartialEq, ToHaskell, FromHaskell)]
struct Settings {
    visible: u32,
    #[haskell(skip)]
    internal: u32,
}

#[test]
fn skip_field_record_to_haskell() {
    let s = Settings {
        visible: 42,
        internal: 999,
    };
    assert_eq!(s.to_haskell(), "Settings {visible = 42}");
}

#[test]
fn skip_field_record_from_haskell() {
    let s = Settings::from_haskell("Settings {visible = 42}").unwrap();
    assert_eq!(s.visible, 42);
    assert_eq!(s.internal, 0);
}

#[test]
fn skip_field_record_roundtrip() {
    let s = Settings {
        visible: 7,
        internal: 888,
    };
    let parsed = Settings::from_haskell(&s.to_haskell()).unwrap();
    assert_eq!(parsed.visible, 7);
    assert_eq!(parsed.internal, 0);
}

// ── skip on enum variant fields ─────────────────────────────────────

#[derive(Debug, PartialEq, ToHaskell, FromHaskell)]
enum Action {
    #[haskell(style = "app")]
    Run {
        speed: u32,
        #[haskell(skip)]
        debug_id: u32,
    },
    Idle,
}

#[test]
fn skip_field_enum_variant_to_haskell() {
    assert_eq!(
        Action::Run {
            speed: 10,
            debug_id: 999
        }
        .to_haskell(),
        "(Run 10)"
    );
}

#[test]
fn skip_field_enum_variant_from_haskell() {
    let a = Action::from_haskell("(Run 10)").unwrap();
    assert_eq!(
        a,
        Action::Run {
            speed: 10,
            debug_id: 0
        }
    );
}

#[test]
fn skip_field_enum_variant_roundtrip() {
    let a = Action::Run {
        speed: 5,
        debug_id: 42,
    };
    let parsed = Action::from_haskell(&a.to_haskell()).unwrap();
    assert_eq!(
        parsed,
        Action::Run {
            speed: 5,
            debug_id: 0
        }
    );
}

// ── explicit style = "record" ───────────────────────────────────────

#[derive(Debug, PartialEq, ToHaskell, FromHaskell)]
#[haskell(style = "record")]
struct Explicit {
    x: u32,
    y: u32,
}

#[test]
fn explicit_record_style() {
    let e = Explicit { x: 1, y: 2 };
    assert_eq!(e.to_haskell(), "Explicit {x = 1, y = 2}");
    assert_eq!(Explicit::from_haskell(&e.to_haskell()).unwrap(), e);
}

// ── bound ───────────────────────────────────────────────────────────

// A type that only implements ToHaskell/FromHaskell for a specific monomorphization.
#[derive(Debug, PartialEq)]
struct Tagged2<T> {
    inner: T,
}

impl ToHaskell for Tagged2<usize> {
    fn write_haskell(&self, buf: &mut impl std::fmt::Write) -> std::fmt::Result {
        ghci::haskell::app(buf, "Tagged2").arg(&self.inner).finish()
    }
}

impl FromHaskell for Tagged2<usize> {
    fn parse_haskell(input: &str) -> Result<(Self, &str), ghci::HaskellParseError> {
        let mut p = ghci::haskell::parse_app("Tagged2", input)?;
        let inner = p.arg()?;
        let rest = p.finish()?;
        Ok((Tagged2 { inner }, rest))
    }
}

#[derive(Debug, PartialEq, ToHaskell, FromHaskell)]
#[haskell(bound(
    ToHaskell = "A: ::ghci::ToHaskell, Tagged2<A>: ::ghci::ToHaskell",
    FromHaskell = "A: ::ghci::FromHaskell, Tagged2<A>: ::ghci::FromHaskell",
))]
struct Bundle<A> {
    plain: A,
    tagged: Tagged2<A>,
}

#[test]
fn custom_bound_to_haskell() {
    let b = Bundle {
        plain: 5usize,
        tagged: Tagged2 { inner: 10 },
    };
    assert_eq!(b.to_haskell(), "Bundle {plain = 5, tagged = (Tagged2 10)}");
}

#[test]
fn custom_bound_from_haskell() {
    let b = Bundle::<usize>::from_haskell("Bundle {plain = 5, tagged = (Tagged2 10)}").unwrap();
    assert_eq!(b.plain, 5);
    assert_eq!(b.tagged, Tagged2 { inner: 10 });
}

#[test]
fn custom_bound_roundtrip() {
    let b = Bundle {
        plain: 42usize,
        tagged: Tagged2 { inner: 99 },
    };
    assert_eq!(Bundle::<usize>::from_haskell(&b.to_haskell()).unwrap(), b);
}
