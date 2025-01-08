use std::sync::Arc;
use std::sync::Mutex;

use crate::routines::future::*;

pub struct Promise<T, E> {
  data: Arc<Mutex<FutureData<T, E>>>,
}

impl<T, E> Promise<T, E> {
  pub fn new_link() -> (Self, Future<T, E>) {
    let future = Future::new();
    let promise = Promise::new(future.data.clone());
    (promise, future)
  }

  pub(crate) fn new(data: Arc<Mutex<FutureData<T, E>>>) -> Self {
    Promise { data: data }
  }

  pub fn resolve(self, result: T) {
    let mut data = self.data.lock().unwrap();
    data.result = Some(Ok(result));
    data.set_state(FutureState::Complete);
  }

  pub fn reject(self, error: E) {
    let mut data = self.data.lock().unwrap();
    data.result = Some(Err(error));
    data.set_state(FutureState::Fail);
  }
}
