//! Direct bytecode differential tests (P5.2, secondary path).
//!
//! Hand-assembled [`CodeBlock`]s — including valid shapes the compiler never
//! emits (unreachable post-`Return` code) — executed under normal vs GC-stress
//! configurations with identical outcomes required. Every block is verified
//! ([`CodeBlock::verify`]) before execution, exactly as the plan's direct
//! path prescribes. Assembly uses [`BytecodeEmitter`] plus the
//! emit-then-patch protocol (`next_opcode_location`/`patch_jump`), never
//! hand-computed addresses.

use boa_gc::{Gc, StressGuard};
use thin_vec::thin_vec;

use super::{
    CallFrame, CodeBlock,
    call_frame::CallFrameFlags,
    code_block::Handler,
    opcode::{Address, BytecodeEmitter, IndexOperand, RegisterOperand},
    verify::verify,
};
use crate::{Context, JsResult, JsValue, environments::EnvironmentStack, js_string};

/// Dummy jump target for emit-then-patch (mirrors the compiler protocol).
const DUMMY: Address = Address::new(u32::MAX);

/// Evaluate a hand-built block the way [`Script::evaluate`] runs a compiled
/// one: push a top-level frame, instantiate globals, run to completion.
///
/// [`Script::evaluate`]: crate::Script::evaluate
fn eval_block(block: &Gc<CodeBlock>, context: &mut Context) -> JsResult<JsValue> {
    context.vm.push_frame_with_stack(
        CallFrame::new(
            block.clone(),
            None,
            EnvironmentStack::new(),
            context.realm().clone(),
        )
        .with_env_fp(0)
        .with_flags(CallFrameFlags::EXIT_EARLY),
        JsValue::undefined(),
        JsValue::null(),
    );
    context.realm().resize_global_env();
    if let Err(err) = context.global_declaration_instantiation(block) {
        context.vm.pop_frame();
        return Err(err);
    }
    let record = context.run();
    context.vm.pop_frame();
    record.consume()
}

/// Assemble a runnable block: emitted bytecode plus the header every shape
/// needs (register file sized, no constants/bindings/caches/handlers).
fn assemble(emitter: BytecodeEmitter, register_count: u32) -> Gc<CodeBlock> {
    let mut block = CodeBlock::new(js_string!("direct-test"), 0, false);
    block.bytecode = emitter.into_bytecode();
    block.register_count = register_count;
    Gc::new(block)
}

/// Verify, then run under normal vs GC-stress legs and require identical
/// outcomes. Returns the agreed value.
fn check_block(block: &Gc<CodeBlock>) -> JsValue {
    verify(block).expect("direct block must verify");

    let mut normal_ctx = Context::default();
    let normal = eval_block(block, &mut normal_ctx).expect("normal leg must complete");

    let guard = StressGuard::stress(std::num::NonZeroU64::new(10).expect("nonzero"));
    let mut stressed_ctx = Context::default();
    let stressed = eval_block(block, &mut stressed_ctx).expect("stressed leg must complete");
    drop(guard);

    assert_eq!(
        format!("{normal:?}"),
        format!("{stressed:?}"),
        "normal vs stressed diverged"
    );
    normal
}

#[test]
fn direct_arithmetic() {
    // r3 = 20 + 22; return r3.
    let mut emitter = BytecodeEmitter::new();
    emitter.emit_store_int32(RegisterOperand::new(1), 20);
    emitter.emit_store_int32(RegisterOperand::new(2), 22);
    emitter.emit_add(
        RegisterOperand::new(3),
        RegisterOperand::new(1),
        RegisterOperand::new(2),
    );
    emitter.emit_set_accumulator(RegisterOperand::new(3));
    emitter.emit_return();
    let block = assemble(emitter, 4);

    let value = check_block(&block);
    assert_eq!(value.as_i32(), Some(42));
}

#[test]
fn direct_jump_over_dead_code() {
    // Jump over an unreachable store (r5 is valid-but-unreachable: validity
    // is not reachability); return 7.
    let mut emitter = BytecodeEmitter::new();
    let jump = emitter.next_opcode_location();
    emitter.emit_jump(DUMMY);
    emitter.emit_store_zero(RegisterOperand::new(5));
    let live = emitter.next_opcode_location();
    emitter.patch_jump(jump, live);
    emitter.emit_store_int32(RegisterOperand::new(1), 7);
    emitter.emit_set_accumulator(RegisterOperand::new(1));
    emitter.emit_return();
    let block = assemble(emitter, 6);

    let value = check_block(&block);
    assert_eq!(value.as_i32(), Some(7));
}

#[test]
fn direct_unreachable_post_return() {
    // NON-JS-REACHABLE shape: the compiler never emits code after `Return`,
    // but trailing instructions still decode and the tail rule holds, so
    // the block is valid and halts at the first `Return`.
    let mut emitter = BytecodeEmitter::new();
    emitter.emit_store_int32(RegisterOperand::new(1), 9);
    emitter.emit_set_accumulator(RegisterOperand::new(1));
    emitter.emit_return();
    emitter.emit_store_zero(RegisterOperand::new(1));
    emitter.emit_return();
    let block = assemble(emitter, 2);

    let value = check_block(&block);
    assert_eq!(value.as_i32(), Some(9));
}

#[test]
fn direct_jump_table_dispatch() {
    // r1 = 1 selects the second table entry; return 200.
    let mut emitter = BytecodeEmitter::new();
    emitter.emit_store_int32(RegisterOperand::new(1), 1);
    // NOTE: +4 to jump past the index operand (matches the compiler protocol:
    // `patch_jump_table` reads the table length at `label + 1`).
    let table = emitter.next_opcode_location() + 4;
    emitter.emit_jump_table(1, thin_vec![DUMMY, DUMMY]);
    let arm0 = emitter.next_opcode_location();
    emitter.emit_store_int32(RegisterOperand::new(2), 100);
    let exit0 = emitter.next_opcode_location();
    emitter.emit_jump(DUMMY);
    let arm1 = emitter.next_opcode_location();
    emitter.emit_store_int32(RegisterOperand::new(2), 200);
    let exit1 = emitter.next_opcode_location();
    emitter.emit_jump(DUMMY);
    let end = emitter.next_opcode_location();
    emitter.patch_jump_table(table, &[arm0, arm1]);
    emitter.patch_jump(exit0, end);
    emitter.patch_jump(exit1, end);
    emitter.emit_set_accumulator(RegisterOperand::new(2));
    emitter.emit_return();
    let block = assemble(emitter, 3);

    let value = check_block(&block);
    assert_eq!(value.as_i32(), Some(200));
}

#[test]
fn direct_handler_catches_throw() {
    // Throw 5 inside a whole-body handler; the handler reads the pending
    // exception and returns it.
    let mut emitter = BytecodeEmitter::new();
    let start = emitter.next_opcode_location();
    emitter.emit_store_int32(RegisterOperand::new(1), 5);
    emitter.emit_throw(RegisterOperand::new(1));
    emitter.emit_return();
    let handler_pc = emitter.next_opcode_location();
    emitter.emit_exception(RegisterOperand::new(2));
    emitter.emit_set_accumulator(RegisterOperand::new(2));
    emitter.emit_return();
    let mut block = CodeBlock::new(js_string!("direct-test"), 0, false);
    block.bytecode = emitter.into_bytecode();
    block.register_count = 3;
    block.handlers.push(Handler {
        start,
        end: handler_pc,
        environment_count: 0,
    });
    let block = Gc::new(block);

    let value = check_block(&block);
    assert_eq!(value.as_i32(), Some(5));
}

#[test]
fn direct_template_pair() {
    // Valid lookup/create pair (miss path on fresh realms); return true.
    // A same-realm rerun exercises the cache-hit path and must agree.
    let mut emitter = BytecodeEmitter::new();
    let lookup = emitter.next_opcode_location();
    emitter.emit_template_lookup(DUMMY, 1, RegisterOperand::new(1));
    emitter.emit_store_undefined(RegisterOperand::new(2));
    emitter.emit_store_undefined(RegisterOperand::new(3));
    emitter.emit_template_create(1, RegisterOperand::new(1), thin_vec![2u32, 3u32]);
    let end = emitter.next_opcode_location();
    emitter.patch_jump(lookup, end);
    emitter.emit_store_true(RegisterOperand::new(4));
    emitter.emit_set_accumulator(RegisterOperand::new(4));
    emitter.emit_return();
    let block = assemble(emitter, 5);

    let value = check_block(&block);
    assert_eq!(value.as_boolean(), Some(true));

    // Same-realm rerun: the lookup now hits the template cache.
    let mut context = Context::default();
    let miss = eval_block(&block, &mut context).expect("miss leg must complete");
    let hit = eval_block(&block, &mut context).expect("hit leg must complete");
    assert_eq!(format!("{miss:?}"), format!("{hit:?}"));
}

#[test]
fn direct_counting_loop() {
    // r1 = 0; loop: r1 += 1; exit when r1 >= 10 (backward jump); return r1.
    let mut emitter = BytecodeEmitter::new();
    emitter.emit_store_int32(RegisterOperand::new(1), 0);
    emitter.emit_store_int32(RegisterOperand::new(10), 10);
    let again = emitter.next_opcode_location();
    emitter.emit_inc(RegisterOperand::new(1), RegisterOperand::new(1));
    let exit_jump = emitter.next_opcode_location();
    emitter.emit_jump_if_not_less_than(DUMMY, RegisterOperand::new(1), RegisterOperand::new(10));
    emitter.emit_jump(again);
    let end = emitter.next_opcode_location();
    emitter.patch_jump(exit_jump, end);
    emitter.emit_set_accumulator(RegisterOperand::new(1));
    emitter.emit_return();
    let block = assemble(emitter, 11);

    let value = check_block(&block);
    assert_eq!(value.as_i32(), Some(10));
}

#[test]
fn direct_def_use_registers() {
    // DefVar/DefInitVar/GetName over one global binding; return it. The
    // binding locator indexes the global declarative scope prepared by
    // instantiation (empty here — the point is the operand path verifies
    // and the init/get round-trips through the runtime binding).
    let mut emitter = BytecodeEmitter::new();
    emitter.emit_store_int32(RegisterOperand::new(1), 11);
    emitter.emit_def_init_var(RegisterOperand::new(1), IndexOperand::new(0));
    emitter.emit_get_name(RegisterOperand::new(2), IndexOperand::new(0));
    emitter.emit_set_accumulator(RegisterOperand::new(2));
    emitter.emit_return();
    let block = assemble(emitter, 3);

    // NOTE: intentionally *not* run — a hand-built block has no binding
    // locator for index 0 (`bindings` is empty), so this shape exercises
    // only the verifier's binding-OOB rejection, not execution.
    let err = verify(&block).expect_err("binding 0 must be out of bounds");
    assert!(
        format!("{err:?}").contains("BindingOutOfBounds"),
        "unexpected error: {err:?}"
    );
}
