use super::JsValueStore;
use boa_engine::value::TryIntoJs;
use boa_engine::{Context, Source, js_string};

/// Round-trips `source` through a `JsValueStore` into a fresh context and
/// evaluates `check` there with the rebuilt value bound to `actual`.
fn assert_round_trip(source: &[u8], check: &[u8]) {
    let mut first = Context::default();
    let value = first.eval(Source::from_bytes(source)).expect("eval source");
    let store = JsValueStore::try_from_js(&value, &mut first, Vec::new()).expect("store");
    drop(first);
    let mut second = Context::default();
    let rebuilt = store.try_into_js(&mut second).expect("rebuild");
    second
        .global_object()
        .create_data_property_or_throw(js_string!("actual"), rebuilt, &mut second)
        .expect("bind actual");
    second.eval(Source::from_bytes(check)).expect("eval check");
}

const ASSERT: &str = "function assert(c, m) { if (!c) throw new Error('assert: ' + m); }\n";

#[test]
fn primitives_round_trip() {
    assert_round_trip(
        b"[1, -2.5, 'two', true, false, null, undefined]",
        format!("{ASSERT}assert(JSON.stringify(actual) === '[1,-2.5,\"two\",true,false,null,null]', 'json');")
            .as_bytes(),
    );
}

#[test]
fn bigint_round_trips() {
    assert_round_trip(
        b"10n ** 30n",
        format!("{ASSERT}assert(actual === 10n ** 30n, 'bigint');").as_bytes(),
    );
}

#[test]
fn nested_objects_round_trip() {
    assert_round_trip(
        b"({a: [1, {b: 'x'}], c: {d: [true, null]}})",
        format!(
            "{ASSERT}assert(JSON.stringify(actual) === '{{\"a\":[1,{{\"b\":\"x\"}}],\"c\":{{\"d\":[true,null]}}}}', 'json');"
        )
        .as_bytes(),
    );
}

#[test]
fn shared_references_stay_shared() {
    assert_round_trip(
        b"var a = [1]; [a, a];",
        format!("{ASSERT}assert(actual[0] === actual[1], 'shared');").as_bytes(),
    );
}

#[test]
fn recursive_values_round_trip() {
    assert_round_trip(
        b"var o = {a: 1}; o.self = o; o;",
        format!("{ASSERT}assert(actual.a === 1 && actual.self === actual, 'recursive');")
            .as_bytes(),
    );
}

#[test]
fn maps_sets_dates_regexps_round_trip() {
    assert_round_trip(
        b"({m: new Map([['k', [1, 2]]]), s: new Set([1, 2, 2]), d: new Date(123456789), r: /ab+c/gi})",
        format!(
            "{ASSERT}\
             assert(actual.m instanceof Map && JSON.stringify(actual.m.get('k')) === '[1,2]', 'map');\
             assert(actual.s instanceof Set && actual.s.size === 2 && actual.s.has(1), 'set');\
             assert(actual.d instanceof Date && actual.d.getTime() === 123456789, 'date');\
             assert(actual.r.source === 'ab+c' && actual.r.flags === 'gi', 'regexp');"
        )
        .as_bytes(),
    );
}

#[test]
fn typed_arrays_round_trip() {
    assert_round_trip(
        b"new Uint8Array([1, 2, 3])",
        format!("{ASSERT}assert(actual.join(',') === '1,2,3', 'bytes');").as_bytes(),
    );
}

#[test]
fn functions_are_rejected() {
    let mut context = Context::default();
    let value = context
        .eval(Source::from_bytes("function f() {} f;"))
        .expect("eval source");
    let err = JsValueStore::try_from_js(&value, &mut context, Vec::new()).expect_err("must reject");
    assert!(err.to_string().contains("DataCloneError"), "{err:?}");
}

#[test]
fn transfer_moves_array_buffers() {
    let mut first = Context::default();
    let value = first
        .eval(Source::from_bytes("new Uint8Array([4, 5, 6]).buffer"))
        .expect("eval source");
    first
        .global_object()
        .create_data_property_or_throw(js_string!("src"), value.clone(), &mut first)
        .expect("bind src");
    let store =
        JsValueStore::try_from_js(&value, &mut first, vec![value.clone()]).expect("transfer");
    // The source is detached by the transfer.
    first
        .eval(Source::from_bytes(
            "if (src.byteLength !== 0) throw new Error('source not detached');",
        ))
        .expect("source detached");
    drop(first);
    let mut second = Context::default();
    let rebuilt = store.try_into_js(&mut second).expect("rebuild");
    second
        .global_object()
        .create_data_property_or_throw(js_string!("actual"), rebuilt, &mut second)
        .expect("bind actual");
    second
        .eval(Source::from_bytes(
            format!("{ASSERT}assert(new Uint8Array(actual).join(',') === '4,5,6', 'bytes');")
                .as_bytes(),
        ))
        .expect("eval check");
}
