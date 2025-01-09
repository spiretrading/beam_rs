use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::mem::MaybeUninit;
use std::num::NonZero;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::Once;
use std::thread::JoinHandle;

use crate::routines::promise::Promise;
use crate::routines::routine::*;

struct Context {
  is_running: bool,
  pending_routines: VecDeque<*mut dyn Routine>,
  suspended_routines: HashSet<*mut dyn Routine>,
  pending_routines_available: Condvar,
}

impl Context {
  fn new() -> Self {
    Context {
      is_running: true,
      pending_routines: VecDeque::new(),
      suspended_routines: HashSet::new(),
      pending_routines_available: Condvar::new(),
    }
  }
}

pub(crate) struct Scheduler {
  thread_count: usize,
  threads: Box<[Option<JoinHandle<()>>]>,
  routine_ids: Mutex<HashMap<u64, *mut dyn Routine>>,
  contexts: Box<[Mutex<Context>]>,
}

impl Scheduler {
  fn new() -> Box<Scheduler> {
    let mut scheduler_box = Box::new(MaybeUninit::<Self>::uninit());
    let scheduler = scheduler_box.as_mut_ptr();
    unsafe {
      (*scheduler).thread_count = std::thread::available_parallelism()
        .unwrap_or(NonZero::new(2).unwrap())
        .get();
      let mut threads = Vec::new();
      let mut contexts = Vec::new();
      for _ in 0..(*scheduler).thread_count {
        contexts.push(Mutex::new(Context::new()));
      }
      (*scheduler).contexts = contexts.into_boxed_slice();
      let scheduler_ptr = scheduler as usize;
      for i in 0..(*scheduler).thread_count {
        threads.push(Some(std::thread::spawn(move || {
          let scheduler = scheduler_ptr as *mut Scheduler;
          (*scheduler).run(&mut (*scheduler).contexts[i]);
        })));
      }
      (*scheduler).threads = threads.into_boxed_slice();
      (*scheduler).routine_ids = Mutex::new(HashMap::new());
      Box::from_raw(Box::into_raw(scheduler_box) as *mut Self)
    }
  }
}

impl Scheduler {
  pub(crate) fn thread_count(&self) -> usize {
    self.thread_count
  }

  pub(crate) fn has_pending_routines(&self, context_id: usize) -> bool {
    !self.contexts[context_id]
      .lock()
      .unwrap()
      .pending_routines
      .is_empty()
  }

  pub(crate) fn wait(&self, id: u64) {
    assert!(current_routine().id() != id);
    let (wait_promise, wait_future) = Promise::<(), ()>::new_link();
    let has_wait = {
      let routine_ids = self.routine_ids.lock().unwrap();
      if let Some(routine) = routine_ids.get(&id).as_ref() {
        unsafe { (***routine).wait(wait_promise) };
        true
      } else {
        false
      }
    };
    if has_wait {
      let _ = wait_future.result();
    }
  }

  pub(crate) fn spawn<F: FnOnce()>(
    &self,
    f: F,
    stack_size: usize,
    context_id: usize,
  ) -> u64
  where
    F: 'static,
  {
    let mut routine = ScheduledRoutine::new(f, stack_size, context_id);
    let id = routine.id();
    {
      let mut routine_ids = self.routine_ids.lock().unwrap();
      routine_ids.insert(id, routine.as_mut() as *mut dyn Routine);
    }
    self.queue(Box::leak(routine));
    id
  }

  pub(crate) fn queue(&self, routine: &mut dyn Routine) {
    let routine_ptr = unsafe {
      std::mem::transmute::<&mut dyn Routine, &'static mut dyn Routine>(routine)
    } as *mut dyn Routine;
    let context = &mut self.contexts[routine.context_id()].lock().unwrap();
    context.pending_routines.push_back(routine_ptr);
    if context.pending_routines.len() == 1 {
      context.pending_routines_available.notify_all();
    }
  }

  pub(crate) fn suspend(&self, routine: &mut dyn Routine) {
    let routine_ptr = unsafe {
      std::mem::transmute::<&mut dyn Routine, &'static mut dyn Routine>(routine)
    } as *mut dyn Routine;
    let context = &mut self.contexts[routine.context_id()].lock().unwrap();
    routine.set_state(RoutineState::Suspended);
    if routine.is_pending_resume() {
      routine.set_pending_resume(false);
      context.pending_routines.push_back(routine_ptr);
      context.pending_routines_available.notify_all();
      return;
    }
    context.suspended_routines.insert(routine_ptr);
  }

  pub(crate) fn resume(&self, routine: &mut dyn Routine) {
    let routine_ptr = unsafe {
      std::mem::transmute::<&mut dyn Routine, &'static mut dyn Routine>(routine)
    } as *mut dyn Routine;
    let context = &mut self.contexts[routine.context_id()].lock().unwrap();
    if let Some(routine) = context.suspended_routines.take(&routine_ptr) {
      context.pending_routines.push_back(routine);
      context.pending_routines_available.notify_all();
    } else {
      routine.set_pending_resume(true);
    }
  }

  fn run(&mut self, context: &mut Mutex<Context>) {
    loop {
      let routine = {
        let mut context = context.lock().unwrap();
        while context.pending_routines.is_empty() {
          if !context.is_running
            && context.pending_routines.is_empty()
            && context.suspended_routines.is_empty()
          {
            return;
          }
          let context_ptr = &mut context as *mut MutexGuard<'_, Context>;
          context = context
            .pending_routines_available
            .wait(unsafe { std::ptr::read(context_ptr) })
            .unwrap();
        }
        unsafe { &mut *context.pending_routines.pop_front().unwrap() }
      };
      //      routine.continue();
      match routine.state() {
        RoutineState::Complete => {
          self.routine_ids.lock().unwrap().remove(&routine.id());
          let _ = unsafe { Box::from_raw(routine as *mut dyn Routine) };
        }
        RoutineState::PendingSuspend => {
          self.suspend(routine);
        }
        _ => {
          self.queue(routine);
        }
      }
    }
  }

  fn stop(&mut self) {
    for i in 0..self.thread_count {
      let context = &mut self.contexts[i].lock().unwrap();
      context.is_running = false;
      context.pending_routines_available.notify_all();
    }
    for thread in std::mem::take(&mut self.threads) {
      let _ = thread.unwrap().join();
    }
    for i in 0..self.thread_count {
      let context = &mut self.contexts[i].lock().unwrap();
      context.is_running = false;
    }
  }
}

impl Drop for Scheduler {
  fn drop(&mut self) {
    self.stop();
  }
}

static mut SCHEDULER: Option<Box<Scheduler>> = None;
static SCHEDULER_INIT: Once = Once::new();

pub(crate) fn get_scheduler() -> &'static Scheduler {
  unsafe {
    SCHEDULER_INIT.call_once(|| {
      SCHEDULER = Some(Scheduler::new());
    });
    SCHEDULER.as_ref().unwrap().as_ref()
  }
}

pub fn spawn<F: FnOnce() + 'static>(f: F) -> u64 {
  get_scheduler().spawn(f, 1024 * 1024, usize::MAX)
}
