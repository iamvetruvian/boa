mod miri {
    use crate::{JsNativeErrorKind, TestAction, run_test_actions};
    use indoc::indoc;

    #[test]
    fn ordinary_has_instance_nonobject_prototype() {
        run_test_actions([TestAction::assert_native_error(
            indoc! {r#"
                function C() {}
                C.prototype = 1
                String instanceof C
            "#},
            JsNativeErrorKind::Type,
            "function has non-object prototype in instanceof check",
        )]);
    }

    #[test]
    fn object_properties_return_order() {
        run_test_actions([
            TestAction::run_harness(),
            TestAction::run(indoc! {r#"
                    var o = {
                        p1: 'v1',
                        p2: 'v2',
                        p3: 'v3',
                    };
                    o.p4 = 'v4';
                    o[2] = 'iv2';
                    o[0] = 'iv0';
                    o[1] = 'iv1';
                    delete o.p1;
                    delete o.p3;
                    o.p1 = 'v1';
                "#}),
            TestAction::assert(
                r#"arrayEquals(Object.keys(o), [ "0", "1", "2", "p2", "p4", "p1" ])"#,
            ),
            TestAction::assert(
                r#"arrayEquals(Object.values(o), [ "iv0", "iv1", "iv2", "v2", "v4", "v1" ])"#,
            ),
        ]);
    }

    /// Bug #13: a proxy-mediated prototype cycle must throw a catchable
    /// `RangeError` (V8-identical shape and message) instead of aborting the
    /// process (recursive get/set/has) or hanging it (`instanceof`,
    /// `isPrototypeOf`, legacy lookup). The walk is infinite, so tripping the
    /// guard is certain on every stack and profile.
    ///
    /// MIRI-IGNORE: each of the 7 probes walks an infinite proxy-mediated
    /// chain until the stack guard trips; under Miri a single probe exceeds
    /// 4GB RSS (proven OOM 2026-10-04) because every interpreter step of the
    /// deep walk carries interpreter metadata. The property under test is
    /// termination (catchable throw, no abort/hang), not memory safety, and
    /// the unsafe surface it touches (Gc walks, proxy dispatch) is covered
    /// by other `mod miri` tests. Still runs in the normal suite.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn cyclic_prototype_walk_throws() {
        let setup = indoc! {r#"
            var root = {};
            var intermediary = new Proxy(Object.create(root), {});
            var leaf = Object.create(intermediary);
            root.__proto__ = leaf;
        "#};
        for probe in [
            "leaf.toString;",
            "leaf.x = 1;",
            "'toString' in leaf;",
            "leaf instanceof Object;",
            "Object.prototype.isPrototypeOf.call({}, leaf);",
            "leaf.__lookupGetter__('toString');",
            "leaf.__lookupSetter__('toString');",
        ] {
            run_test_actions([TestAction::assert_native_error(
                format!("{setup}\n{probe}"),
                JsNativeErrorKind::Range,
                "Maximum call stack size exceeded",
            )]);
        }
    }

    /// Exercises the `GcRefCell` borrow panic path (`Writing` re-borrow).
    #[test]
    #[should_panic(expected = "already borrowed")]
    fn object_borrow_mut_while_borrowed_panics() {
        use crate::builtins::OrdinaryObject;

        let context = &mut crate::test_context();
        let o = context
            .intrinsics()
            .templates()
            .ordinary_object()
            .create(OrdinaryObject, Vec::default());
        let _first = o.borrow_mut();
        let _second = o.borrow_mut();
    }
}
