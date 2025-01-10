use std::sync::atomic::Ordering;
use std::sync::Condvar;
use std::sync::Mutex;

use crate::routines::promise::*;
use crate::routines::routine::*;

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
      state: RoutineState::Running,
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

  fn advance(&mut self) {}

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
