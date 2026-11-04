use std::collections::{HashMap, VecDeque};

use rustc_middle::{
    mir::{self, Operand, Place, ProjectionElem, Rvalue, StatementKind, TerminatorKind},
    ty::{self, TyCtxt},
};

#[derive(Default)]
pub(super) struct ReceiverIdentity<'tcx> {
    cache: HashMap<(ty::TypingEnv<'tcx>, ty::Instance<'tcx>), bool>,
    depth: usize,
}

impl<'tcx> ReceiverIdentity<'tcx> {
    pub(super) fn returns_receiver(
        &mut self,
        tcx: TyCtxt<'tcx>,
        env: ty::TypingEnv<'tcx>,
        instance: ty::Instance<'tcx>,
    ) -> bool {
        if let Some(result) = self.cache.get(&(env, instance)) {
            return *result;
        }
        // Polymorphic recursion can produce infinitely many distinct instances.
        if self.depth == 32
            || !matches!(instance.def, ty::InstanceKind::Item(_))
            || !tcx.is_mir_available(instance.def_id())
        {
            return false;
        }
        // A recursive call cannot establish its own receiver identity.
        self.cache.insert((env, instance), false);
        self.depth += 1;
        let result = self.analyze(tcx, env, instance);
        self.depth -= 1;
        self.cache.insert((env, instance), result);
        result
    }

    fn analyze(
        &mut self,
        tcx: TyCtxt<'tcx>,
        env: ty::TypingEnv<'tcx>,
        instance: ty::Instance<'tcx>,
    ) -> bool {
        let body = tcx.optimized_mir(instance.def_id());
        if body.arg_count == 0 {
            return false;
        }
        let receiver = mir::Local::from_u32(1);
        let Some((input, output)) = instance
            .try_instantiate_mir_and_normalize_erasing_regions(
                tcx,
                env,
                ty::EarlyBinder::bind(tcx, (body.local_decls[receiver].ty, body.return_ty())),
            )
            .ok()
        else {
            return false;
        };
        if !matches!(input.kind(), ty::Ref(_, _, rustc_hir::Mutability::Mut))
            || tcx.erase_and_anonymize_regions(input) != tcx.erase_and_anonymize_regions(output)
        {
            return false;
        }

        // Once reference storage is exposed, an indirect write can change its
        // target. Unrelated exposed locals do not invalidate the receiver.
        let mut exposed = vec![false; body.local_decls.len()];
        for data in body.basic_blocks.iter() {
            for statement in &data.statements {
                if let StatementKind::Assign(assignment) = &statement.kind
                    && let Rvalue::Ref(_, mir::BorrowKind::Mut { .. }, place)
                    | Rvalue::RawPtr(_, place) = &assignment.1
                    && place.projection.is_empty()
                    && matches!(body.local_decls[place.local].ty.kind(), ty::Ref(..))
                {
                    exposed[place.local.index()] = true;
                }
            }
        }
        let mut entry = vec![false; body.local_decls.len()];
        entry[receiver.index()] = !exposed[receiver.index()];
        let mut incoming = vec![None; body.basic_blocks.len()];
        incoming[mir::START_BLOCK.index()] = Some(entry);
        let mut pending = VecDeque::from([mir::START_BLOCK]);
        let mut returned = false;
        while let Some(block) = pending.pop_front() {
            let mut state = incoming[block.index()].clone().unwrap();
            let data = &body.basic_blocks[block];
            for statement in &data.statements {
                match &statement.kind {
                    StatementKind::Assign(assignment) => {
                        let (destination, value) = &**assignment;
                        let origin = match value {
                            Rvalue::Use(operand, _) => operand_origin(operand, &state),
                            Rvalue::Ref(_, _, place) => {
                                matches!(&place.projection[..], [ProjectionElem::Deref])
                                    && state[place.local.index()]
                            }
                            _ => false,
                        };
                        write(
                            *destination,
                            origin && !exposed[destination.local.index()],
                            &mut state,
                        );
                    }
                    StatementKind::StorageLive(local) | StatementKind::StorageDead(local) => {
                        state[local.index()] = false;
                    }
                    _ => {}
                }
            }
            let terminator = data.terminator();
            match &terminator.kind {
                TerminatorKind::Return => {
                    returned = true;
                    if !state[mir::RETURN_PLACE.index()] {
                        return false;
                    }
                }
                TerminatorKind::Call {
                    func,
                    args,
                    destination,
                    ..
                } => {
                    let origin = args
                        .first()
                        .is_some_and(|arg| operand_origin(&arg.node, &state))
                        && self.delegates(tcx, env, instance, body, func);
                    write(
                        *destination,
                        origin && !exposed[destination.local.index()],
                        &mut state,
                    );
                }
                TerminatorKind::InlineAsm { .. }
                | TerminatorKind::TailCall { .. }
                | TerminatorKind::Yield { .. } => return false,
                _ => {}
            }
            for successor in terminator.successors() {
                let changed = if let Some(previous) = &mut incoming[successor.index()] {
                    let mut changed = false;
                    for (previous, next) in previous.iter_mut().zip(&state) {
                        changed |= *previous && !next;
                        *previous &= next;
                    }
                    changed
                } else {
                    incoming[successor.index()] = Some(state.clone());
                    true
                };
                if changed {
                    pending.push_back(successor);
                }
            }
        }
        returned
    }

    fn delegates(
        &mut self,
        tcx: TyCtxt<'tcx>,
        env: ty::TypingEnv<'tcx>,
        instance: ty::Instance<'tcx>,
        body: &mir::Body<'tcx>,
        function: &Operand<'tcx>,
    ) -> bool {
        let Ok(ty) = instance.try_instantiate_mir_and_normalize_erasing_regions(
            tcx,
            env,
            ty::EarlyBinder::bind(tcx, function.ty(&body.local_decls, tcx)),
        ) else {
            return false;
        };
        let ty::FnDef(id, args) = *ty.kind() else {
            return false;
        };
        let Ok(Some(callee)) = ty::Instance::try_resolve(tcx, env, id, args.skip_binder()) else {
            return false;
        };
        self.returns_receiver(tcx, env, callee)
    }
}

fn operand_origin(operand: &Operand<'_>, state: &[bool]) -> bool {
    match operand {
        Operand::Copy(place) | Operand::Move(place) if place.projection.is_empty() => {
            state[place.local.index()]
        }
        _ => false,
    }
}

fn write(destination: Place<'_>, origin: bool, state: &mut [bool]) {
    if destination.projection.is_empty() {
        state[destination.local.index()] = origin;
    }
}
