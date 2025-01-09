use std::cell::RefCell;
use std::mem::MaybeUninit;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Condvar;
use std::sync::Mutex;
use std::thread_local;

use corosensei::stack::DefaultStack;
use corosensei::Coroutine;
use corosensei::Yielder;

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

  fn run(&mut self);

  fn defer(&mut self);

  fn pending_suspend(&mut self);

  fn suspend(&mut self);

  fn resume(&mut self);

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

pub fn wait(routine: u64) {}

pub(crate) fn suspend() {
  current_routine().suspend();
}

pub fn suspend_into(suspended_routine: &mut &'static mut dyn Routine) {
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

pub(crate) struct ExternalRoutine {
  state: RoutineState,
  id: u64,
  is_pending_resume: Mutex<bool>,
  suspended_condition: Condvar,
  wait_promises: Mutex<Vec<Promise<(), ()>>>,
}

impl ExternalRoutine {
  pub fn new() -> Self {
    ExternalRoutine {
      state: RoutineState::Pending,
      id: ROUTINE_ID_COUNTER.fetch_add(1, Ordering::SeqCst),
      is_pending_resume: Mutex::new(false),
      suspended_condition: Condvar::new(),
      wait_promises: Mutex::new(Vec::new()),
    }
  }
}

impl Routine for ExternalRoutine {
  fn id(&self) -> u64 {
    self.id
  }

  fn context_id(&self) -> usize {
    usize::MAX
  }

  fn state(&self) -> RoutineState {
    self.state
  }

  fn is_pending_resume(&self) -> bool {
    false
  }

  fn set_pending_resume(&mut self, _: bool) {}

  fn wait(&mut self, result: Promise<(), ()>) {
    let mut wait_promises = self.wait_promises.lock().unwrap();
    wait_promises.push(result);
  }

  fn run(&mut self) {
    self.set_state(RoutineState::Running);
  }

  fn defer(&mut self) {}

  fn pending_suspend(&mut self) {
    self.set_state(RoutineState::PendingSuspend);
  }

  fn suspend(&mut self) {
    let mut is_pending_resume = self.is_pending_resume.lock().unwrap();
    self.state = RoutineState::Suspended;
    if *is_pending_resume {
      *is_pending_resume = false;
      return;
    }
    while self.state() == RoutineState::Suspended {
      is_pending_resume =
        self.suspended_condition.wait(is_pending_resume).unwrap();
    }
  }

  fn resume(&mut self) {
    let mut is_pending_resume = self.is_pending_resume.lock().unwrap();
    if self.state() == RoutineState::PendingSuspend {
      *is_pending_resume = true;
      return;
    }
    self.state = RoutineState::Running;
    self.suspended_condition.notify_one();
  }

  fn set_state(&mut self, state: RoutineState) {
    self.state = state;
  }
}

impl Drop for ExternalRoutine {
  fn drop(&mut self) {
    self.state = RoutineState::Complete;
    let mut lock = self.wait_promises.lock().unwrap();
    let wait_promises = std::mem::take(&mut *lock);
    for promise in wait_promises.into_iter() {
      promise.resolve(());
    }
  }
}

pub(crate) struct ScheduledRoutine {
  state: RoutineState,
  id: u64,
  wait_promises: Mutex<Vec<Promise<(), ()>>>,
  is_pending_resume: bool,
  context_id: usize,
  function: Option<Coroutine<(), (), ()>>,
  yielder: *const Yielder<(), ()>,
}

impl ScheduledRoutine {
  pub(crate) fn new<F: FnOnce()>(
    f: F,
    stack_size: usize,
    mut context_id: usize,
  ) -> Box<Self>
  where
    F: 'static,
  {
    let id = ROUTINE_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
    if context_id == usize::MAX {
      context_id = id as usize % get_scheduler().thread_count();
    }
    let mut routine_box = Box::new(MaybeUninit::<Self>::uninit());
    let routine = routine_box.as_mut_ptr();
    unsafe {
      (*routine).state = RoutineState::Pending;
      (*routine).id = id;
      (*routine).wait_promises = Mutex::new(Vec::new());
      (*routine).is_pending_resume = false;
      (*routine).context_id = context_id;
      (*routine).function = Some(Coroutine::with_stack(
        DefaultStack::new(stack_size).unwrap(),
        move |yielder, _| {
          (*routine).yielder = yielder as *const Yielder<(), ()>;
          f();
        },
      ));
      Box::from_raw(Box::into_raw(routine_box) as *mut Self)
    }
  }
}

impl Routine for ScheduledRoutine {
  fn id(&self) -> u64 {
    self.id
  }

  fn context_id(&self) -> usize {
    self.context_id
  }

  fn state(&self) -> RoutineState {
    self.state
  }

  fn is_pending_resume(&self) -> bool {
    self.is_pending_resume
  }

  fn set_pending_resume(&mut self, is_pending_resume: bool) {
    self.is_pending_resume = is_pending_resume;
  }

  fn wait(&mut self, result: Promise<(), ()>) {
    let mut wait_promises = self.wait_promises.lock().unwrap();
    wait_promises.push(result);
  }

  fn run(&mut self) {
    self.function.as_mut().unwrap().resume(());
  }

  fn defer(&mut self) {
    CURRENT_ROUTINE.with(|routine_cell| {
      let mut routine = routine_cell.borrow_mut();
      *routine = None;
    });
    unsafe { (*self.yielder).suspend(()) };
  }

  fn pending_suspend(&mut self) {
    self.set_state(RoutineState::PendingSuspend);
  }

  fn suspend(&mut self) {
    CURRENT_ROUTINE.with(|routine_cell| {
      let mut routine = routine_cell.borrow_mut();
      *routine = None;
    });
    self.set_state(RoutineState::PendingSuspend);
    unsafe { (*self.yielder).suspend(()) };
  }

  fn resume(&mut self) {
    get_scheduler().resume(self);
  }

  fn set_state(&mut self, state: RoutineState) {
    self.state = state;
  }
}

impl Drop for ScheduledRoutine {
  fn drop(&mut self) {
    self.state = RoutineState::Complete;
    let mut lock = self.wait_promises.lock().unwrap();
    let wait_promises = std::mem::take(&mut *lock);
    for promise in wait_promises.into_iter() {
      promise.resolve(());
    }
  }
}
