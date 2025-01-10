use std::mem::MaybeUninit;
use std::ptr::addr_of_mut;
use std::sync::atomic::Ordering;
use std::sync::Mutex;

use corosensei::stack::DefaultStack;
use corosensei::Coroutine;
use corosensei::Yielder;

use crate::routines::promise::*;
use crate::routines::routine::*;
use crate::routines::scheduler::*;

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
      addr_of_mut!((*routine).state).write(RoutineState::Pending);
      addr_of_mut!((*routine).id).write(id);
      addr_of_mut!((*routine).wait_promises).write(Mutex::new(Vec::new()));
      addr_of_mut!((*routine).is_pending_resume).write(false);
      addr_of_mut!((*routine).context_id).write(context_id);
      addr_of_mut!((*routine).function).write(Some(Coroutine::with_stack(
        DefaultStack::new(stack_size).unwrap(),
        move |yielder, _| {
          (*routine).yielder = yielder as *const Yielder<(), ()>;
          f();
          (*routine).set_state(RoutineState::Complete);
        },
      )));
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

  fn advance(&mut self) {
    CURRENT_ROUTINE.with(|routine_cell| {
      let mut routine = routine_cell.borrow_mut();
      *routine = Some(self);
    });
    self.is_pending_resume = false;
    self.set_state(RoutineState::Running);
    self.function.as_mut().unwrap().resume(());
    CURRENT_ROUTINE.with(|routine_cell| {
      let mut routine = routine_cell.borrow_mut();
      *routine = None;
    });
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
