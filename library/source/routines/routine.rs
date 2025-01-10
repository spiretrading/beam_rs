use std::cell::RefCell;
use std::sync::atomic::AtomicU64;
use std::thread_local;

use crate::routines::external_routine::*;
use crate::routines::promise::*;
use crate::routines::scheduler::*;

pub(crate) static ROUTINE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

thread_local! {
  pub(crate) static CURRENT_ROUTINE: RefCell<Option<*mut dyn Routine>> =
    RefCell::new(None);
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RoutineState {
  Pending,
  Running,
  PendingSuspend,
  Suspended,
  Complete,
}

pub(crate) trait Routine {
  fn id(&self) -> u64;

  fn context_id(&self) -> usize;

  fn state(&self) -> RoutineState;

  fn is_pending_resume(&self) -> bool;

  fn set_pending_resume(&mut self, is_pending_resume: bool);

  fn wait(&mut self, result: Promise<(), ()>);

  fn defer(&mut self);

  fn pending_suspend(&mut self);

  fn suspend(&mut self);

  fn resume(&mut self);

  fn advance(&mut self);

  fn set_state(&mut self, state: RoutineState);
}

pub(crate) fn current_routine() -> &'static mut dyn Routine {
  CURRENT_ROUTINE.with(|routine_cell| {
    let mut routine = routine_cell.borrow_mut();
    if routine.is_none() {
      let external_routine = Box::new(ExternalRoutine::new());
      *routine = Some(Box::leak(external_routine) as *mut dyn Routine);
    }
    unsafe { &mut *routine.unwrap() }
  })
}

pub fn defer() {
  current_routine().defer();
}

pub fn wait(routine: u64) {
  get_scheduler().wait(routine);
}

pub(crate) fn suspend() {
  current_routine().suspend();
}

pub(crate) fn suspend_into(suspended_routine: &mut &'static mut dyn Routine) {
  *suspended_routine = current_routine();
  suspend();
}

pub(crate) fn resume(routine: &mut Option<*mut dyn Routine>) {
  if routine.is_none() {
    return;
  }
  let initial_routine = unsafe { &mut *std::mem::take(routine).unwrap() };
  initial_routine.resume();
}
