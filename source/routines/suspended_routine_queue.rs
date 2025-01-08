use std::cell::RefCell;
use std::sync::MutexGuard;

use intrusive_collections::intrusive_adapter;
use intrusive_collections::{LinkedList, LinkedListLink, UnsafeRef};

use crate::routines::routine::*;

pub(crate) struct SuspendedRoutineNode {
  routine: RefCell<Option<*mut dyn Routine>>,
  link: LinkedListLink,
}

intrusive_adapter!(
pub(crate) SuspendedRoutineNodeAdapter =
  UnsafeRef<SuspendedRoutineNode>: SuspendedRoutineNode {
    link: LinkedListLink
  });

impl SuspendedRoutineNode {
  fn new() -> Self {
    SuspendedRoutineNode {
      routine: RefCell::new(Some(current_routine() as *mut dyn Routine)),
      link: LinkedListLink::new(),
    }
  }
}

pub(crate) type SuspendedRoutineQueue = LinkedList<SuspendedRoutineNodeAdapter>;

pub(crate) fn suspend<'a, T>(
  suspended_routines: &mut SuspendedRoutineQueue,
  guard: MutexGuard<'a, T>,
) {
  let current_routine = SuspendedRoutineNode::new();
  unsafe {
    (*current_routine.routine.borrow().unwrap()).pending_suspend();
    suspended_routines.push_back(UnsafeRef::from_box(Box::new(
      SuspendedRoutineNode {
        routine: current_routine.routine,
        link: LinkedListLink::new(),
      },
    )));
  }
  drop(guard);
  crate::routines::routine::suspend();
}

pub(crate) fn resume(suspended_routines: &mut SuspendedRoutineQueue) {
  let mut resumed_routines =
    SuspendedRoutineQueue::new(SuspendedRoutineNodeAdapter::new());
  std::mem::swap(suspended_routines, &mut resumed_routines);
  for routine in resumed_routines {
    crate::routines::routine::resume(&mut routine.routine.borrow_mut());
  }
}
