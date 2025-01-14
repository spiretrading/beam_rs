use std::sync::Arc;
use std::sync::Mutex;

use crate::routines::suspended_routine_queue::*;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum FutureState {
  Pending,
  Complete,
  Fail,
}

pub(crate) struct FutureData<T, E> {
  state: FutureState,
  suspended_routines: SuspendedRoutineQueue,
  pub(crate) result: Option<Result<T, E>>,
}

impl<T, E> FutureData<T, E> {
  pub fn set_state(&mut self, state: FutureState) {
    assert!(self.state == FutureState::Pending);
    assert!(state != FutureState::Pending);
    self.state = state;
    resume(&mut self.suspended_routines);
  }
}

pub struct Future<T, E> {
  pub(crate) data: Arc<Mutex<FutureData<T, E>>>,
}

impl<T, E> Future<T, E> {
  pub fn new() -> Self {
    Future {
      data: Arc::new(Mutex::new(FutureData {
        state: FutureState::Pending,
        suspended_routines: SuspendedRoutineQueue::new(
          SuspendedRoutineNodeAdapter::new(),
        ),
        result: None,
      })),
    }
  }

  pub fn result(self) -> Result<T, E> {
    let mut data = self.data.lock().unwrap();
    while data.state == FutureState::Pending {
      let suspended_routines = &mut data.suspended_routines as *mut _;
      suspend(unsafe { &mut *suspended_routines }, data);
      data = self.data.lock().unwrap();
    }
    return std::mem::replace(&mut data.result, None).unwrap();
  }

  pub fn state(&self) -> FutureState {
    self.data.lock().unwrap().state
  }
}
