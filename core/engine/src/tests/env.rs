use boa_macros::js_str;
use indoc::indoc;

use crate::{JsNativeErrorKind, JsValue, TestAction, run_test_actions};

#[test]
// https://github.com/boa-dev/boa/issues/2317
fn fun_block_eval_2317() {
    run_test_actions([
        TestAction::assert_eq(
            indoc! {r#"
                (function(y){
                    {
                        eval("var x = 'inner';");
                    }
                    return y + x;
                })("arg");
            "#},
            js_str!("arginner"),
        ),
        TestAction::assert_eq(
            indoc! {r#"
                (function(y = "default"){
                    {
                        eval("var x = 'inner';");
                    }
                    return y + x;
                })();
            "#},
            js_str!("defaultinner"),
        ),
    ]);
}

#[test]
// https://github.com/boa-dev/boa/issues/2719
fn with_env_not_panic() {
    run_test_actions([TestAction::assert_native_error(
        indoc! {r#"
            with({ p1:1,  }) {k[oa>>2]=d;}
            {
            let a12345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890 = 1,
                b = "";
            }
        "#},
        JsNativeErrorKind::Reference,
        "k is not defined",
    )]);
}

#[test]
// https://github.com/boa-dev/boa/issues/4350
fn indirect_eval_function_var_binding_4350() {
    run_test_actions([TestAction::assert_eq(
        indoc! {r#"
            var t = [];

            var s1 = `
            function core() { t.push(1) }

            core.prototype.a = function () { t.push(2) }
            core.prototype.b = function () { t.push(3) }
            `;
            var s2 = `
            function core() { t.push(1) }

            core.prototype.a = function () { t.push(2) }
            core.prototype.b = function () { t.push(3) }
            var core = new core();
            `;
            var s3 = `
            function core() { t.push(1) }
            var core = new core();
            `;

            function run_ctx(s) {
                (1,eval)(s);
            }

            function test() {
                run_ctx(s1);
                var core1 = new core();

                run_ctx(s2);
                var core2 = core;

                run_ctx(s3);
                var core3 = core;
                return [core1, core2, core3].toString();
            }

            test();
        "#},
        js_str!("[object Object],[object Object],[object Object]"),
    )]);
}

#[test]
// https://github.com/boa-dev/boa/issues/5333
fn eval_created_bindings_can_be_deleted_5333() {
    run_test_actions([
        TestAction::assert_eq(
            indoc! {r#"
                (function() {
                    var initial = null;
                    var deleted = null;
                    var postDeletion;
                    eval('initial = x; deleted = delete x; postDeletion = function() { x; }; var x;');
                    try {
                        postDeletion();
                        return 'no throw';
                    } catch (e) {
                        return String(initial) + ':' + String(deleted) + ':' + e.name;
                    }
                }());
            "#},
            js_str!("undefined:true:ReferenceError"),
        ),
        TestAction::assert_eq(
            indoc! {r#"
                (function() {
                    var initial;
                    var deleted = null;
                    var postDeletion;
                    eval('initial = f; deleted = delete f; postDeletion = function() { f; }; function f() { return 33; }');
                    try {
                        postDeletion();
                        return 'no throw';
                    } catch (e) {
                        return typeof initial + ':' + String(initial()) + ':' + String(deleted) + ':' + e.name;
                    }
                }());
            "#},
            js_str!("function:33:true:ReferenceError"),
        ),
        TestAction::assert_eq(
            indoc! {r#"
                (function() {
                    delete globalThis.x;
                    eval('delete x; var x = 1;');
                    var result = typeof globalThis.x + ':' + String(globalThis.x);
                    delete globalThis.x;
                    return result;
                }());
            "#},
            js_str!("number:1"),
        ),
        TestAction::assert_eq(
            indoc! {r#"
                (function() {
                    delete globalThis.x;
                    var result = eval('var x = delete x; x;');
                    var global = globalThis.x;
                    delete globalThis.x;
                    return String(result) + ':' + String(global);
                }());
            "#},
            js_str!("true:true"),
        ),
        TestAction::assert_eq(
            indoc! {r#"
                (function() {
                    delete globalThis.x;
                    var x = 'outer';
                    var result = (function() {
                        return eval('var x = delete x; x;');
                    }());
                    var global = globalThis.x;
                    delete globalThis.x;
                    return String(result) + ':' + String(x) + ':' + String(global);
                }());
            "#},
            js_str!("true:true:undefined"),
        ),
        TestAction::assert_eq(
            indoc! {r#"
                (function() {
                    delete globalThis.x;
                    eval('var x; delete x;');
                    eval('var x = 2;');
                    var result = String(x) + ':' + String(globalThis.x);
                    delete globalThis.x;
                    return result;
                }());
            "#},
            js_str!("2:undefined"),
        ),
        TestAction::assert_eq(
            indoc! {r#"
                (function() {
                    delete globalThis.f;
                    eval('function f() {}; delete f;');
                    eval('function f() { return 2; }');
                    var result = String(f()) + ':' + String(globalThis.f);
                    delete globalThis.f;
                    return result;
                }());
            "#},
            js_str!("2:undefined"),
        ),
    ]);
}

#[test]
// https://tc39.es/ecma262/#sec-putvalue
fn strict_assign_to_binding_created_by_rhs_throws() {
    run_test_actions([
        TestAction::assert_native_error(
            indoc! {r#"
                "use strict";
                undeclared = (this.undeclared = 5);
            "#},
            JsNativeErrorKind::Reference,
            "undeclared is not defined",
        ),
        // Sloppy mode still creates the global.
        TestAction::run(indoc! {r#"
            sloppyCreated = (this.sloppyCreated = 5);
        "#}),
        TestAction::assert_eq("sloppyCreated", 5),
    ]);
}

#[test]
// https://tc39.es/ecma262/#sec-ordinaryownpropertykeys
// Found by P3 differential testing (jsshell agreement probe); no Test262
// test covers Object.keys(globalThis) order.
fn global_var_bindings_enumerate_in_source_order() {
    run_test_actions([
        TestAction::run(indoc! {r#"
            var zkord_a = 0; var zkord_b = 0; var zkord_c = 0; var zkord_d = 0; var zkord_e = 0;
        "#}),
        TestAction::assert_eq(
            indoc! {r#"
                Object.keys(globalThis).filter(function (k) {
                    return k.indexOf("zkord_") === 0;
                }).join(",");
            "#},
            js_str!("zkord_a,zkord_b,zkord_c,zkord_d,zkord_e"),
        ),
        // Global eval takes the same instantiation path.
        TestAction::run(indoc! {r#"
            eval("var zkev_a = 0; var zkev_b = 0; var zkev_c = 0;");
        "#}),
        TestAction::assert_eq(
            indoc! {r#"
                Object.keys(globalThis).filter(function (k) {
                    return k.indexOf("zkev_") === 0;
                }).join(",");
            "#},
            js_str!("zkev_a,zkev_b,zkev_c"),
        ),
    ]);
}

#[test]
// https://tc39.es/ecma262/#sec-global-object
// Found by P3 differential testing (state-filter asymmetry against jsshell):
// the engine exposed a `TypedArray` global the spec does not list.
fn no_typedarray_global_binding() {
    run_test_actions([
        TestAction::assert_eq("typeof TypedArray", js_str!("undefined")),
        TestAction::assert_eq("globalThis.TypedArray", JsValue::undefined()),
        // The intrinsic itself stays reachable through its subclasses.
        TestAction::assert_eq(
            "typeof Object.getPrototypeOf(Uint8Array)",
            js_str!("function"),
        ),
        TestAction::assert(
            "Object.getPrototypeOf(Uint8Array) === Object.getPrototypeOf(Float64Array)",
        ),
    ]);
}
